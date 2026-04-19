use std::sync::Arc;

use aionui_api_types::WebSocketMessage;
use aionui_db::models::{MailboxMessageRow, TeamRow, TeamTaskRow};
use aionui_db::{DbError, ITeamRepository, UpdateTaskParams, UpdateTeamParams};
use aionui_realtime::EventBroadcaster;
use aionui_team::mcp::protocol::{read_frame, write_frame};
use aionui_team::{
    Mailbox, TaskBoard, TeamAgent, TeamMcpServer, TeammateManager, TeammateRole,
};
use serde_json::{json, Value};
use tokio::net::TcpStream;

// ---------------------------------------------------------------------------
// Test infrastructure (same pattern as scheduler_integration.rs)
// ---------------------------------------------------------------------------

struct RecordingBroadcaster {
    events: std::sync::Mutex<Vec<WebSocketMessage<Value>>>,
}

impl RecordingBroadcaster {
    fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(vec![]),
        }
    }
}

impl EventBroadcaster for RecordingBroadcaster {
    fn broadcast(&self, event: WebSocketMessage<Value>) {
        self.events.lock().unwrap().push(event);
    }
}

#[derive(Default)]
struct MockState {
    messages: Vec<MailboxMessageRow>,
    tasks: Vec<TeamTaskRow>,
}

struct MockTeamRepo {
    state: std::sync::Mutex<MockState>,
}

impl MockTeamRepo {
    fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(MockState::default()),
        }
    }
}

#[async_trait::async_trait]
impl ITeamRepository for MockTeamRepo {
    async fn create_team(&self, _row: &TeamRow) -> Result<(), DbError> {
        Ok(())
    }
    async fn list_teams(&self) -> Result<Vec<TeamRow>, DbError> {
        Ok(vec![])
    }
    async fn get_team(&self, _id: &str) -> Result<Option<TeamRow>, DbError> {
        Ok(None)
    }
    async fn update_team(&self, _id: &str, _p: &UpdateTeamParams) -> Result<(), DbError> {
        Ok(())
    }
    async fn delete_team(&self, _id: &str) -> Result<(), DbError> {
        Ok(())
    }

    async fn write_message(&self, row: &MailboxMessageRow) -> Result<(), DbError> {
        self.state.lock().unwrap().messages.push(row.clone());
        Ok(())
    }

    async fn read_unread_and_mark(
        &self,
        team_id: &str,
        to_agent_id: &str,
    ) -> Result<Vec<MailboxMessageRow>, DbError> {
        let mut state = self.state.lock().unwrap();
        let mut result = vec![];
        for msg in &mut state.messages {
            if msg.team_id == team_id && msg.to_agent_id == to_agent_id && !msg.read {
                msg.read = true;
                result.push(msg.clone());
            }
        }
        Ok(result)
    }

    async fn get_history(
        &self,
        team_id: &str,
        to_agent_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<MailboxMessageRow>, DbError> {
        let state = self.state.lock().unwrap();
        let iter = state
            .messages
            .iter()
            .filter(|m| m.team_id == team_id && m.to_agent_id == to_agent_id);
        let msgs: Vec<_> = match limit {
            Some(n) => iter.take(n as usize).cloned().collect(),
            None => iter.cloned().collect(),
        };
        Ok(msgs)
    }

    async fn delete_mailbox_by_team(&self, team_id: &str) -> Result<(), DbError> {
        self.state
            .lock()
            .unwrap()
            .messages
            .retain(|m| m.team_id != team_id);
        Ok(())
    }

    async fn create_task(&self, row: &TeamTaskRow) -> Result<(), DbError> {
        self.state.lock().unwrap().tasks.push(row.clone());
        Ok(())
    }

    async fn find_task_by_id(
        &self,
        team_id: &str,
        task_id: &str,
    ) -> Result<Option<TeamTaskRow>, DbError> {
        let state = self.state.lock().unwrap();
        let found = state
            .tasks
            .iter()
            .find(|t| t.team_id == team_id && t.id == task_id)
            .cloned();
        Ok(found)
    }

    async fn update_task(
        &self,
        task_id: &str,
        params: &UpdateTaskParams,
    ) -> Result<(), DbError> {
        let mut state = self.state.lock().unwrap();
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| DbError::NotFound(task_id.to_owned()))?;
        if let Some(ref s) = params.status {
            task.status = s.clone();
        }
        if let Some(ref d) = params.description {
            task.description = Some(d.clone());
        }
        if let Some(ref o) = params.owner {
            task.owner = Some(o.clone());
        }
        if let Some(ref b) = params.blocked_by {
            task.blocked_by = b.clone();
        }
        if let Some(ref m) = params.metadata {
            task.metadata = Some(m.clone());
        }
        task.updated_at = aionui_common::now_ms();
        Ok(())
    }

    async fn list_tasks(&self, team_id: &str) -> Result<Vec<TeamTaskRow>, DbError> {
        let state = self.state.lock().unwrap();
        let tasks = state
            .tasks
            .iter()
            .filter(|t| t.team_id == team_id)
            .cloned()
            .collect();
        Ok(tasks)
    }

    async fn append_to_blocks(
        &self,
        task_id: &str,
        blocked_task_id: &str,
    ) -> Result<(), DbError> {
        let mut state = self.state.lock().unwrap();
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| DbError::NotFound(task_id.to_owned()))?;
        let mut blocks: Vec<String> = serde_json::from_str(&task.blocks).unwrap_or_default();
        blocks.push(blocked_task_id.to_owned());
        task.blocks = serde_json::to_string(&blocks).unwrap();
        Ok(())
    }

    async fn remove_from_blocked_by(
        &self,
        task_id: &str,
        unblocked_task_id: &str,
    ) -> Result<(), DbError> {
        let mut state = self.state.lock().unwrap();
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| DbError::NotFound(task_id.to_owned()))?;
        let mut blocked_by: Vec<String> =
            serde_json::from_str(&task.blocked_by).unwrap_or_default();
        blocked_by.retain(|id| id != unblocked_task_id);
        task.blocked_by = serde_json::to_string(&blocked_by).unwrap();
        Ok(())
    }

    async fn delete_tasks_by_team(&self, team_id: &str) -> Result<(), DbError> {
        self.state
            .lock()
            .unwrap()
            .tasks
            .retain(|t| t.team_id != team_id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_agents() -> Vec<TeamAgent> {
    vec![
        TeamAgent {
            slot_id: "lead-1".into(),
            name: "Leader".into(),
            role: TeammateRole::Lead,
            conversation_id: "conv-lead".into(),
            backend: "acp".into(),
            model: "claude".into(),
            custom_agent_id: None,
            status: None,
        },
        TeamAgent {
            slot_id: "worker-1".into(),
            name: "Worker".into(),
            role: TeammateRole::Teammate,
            conversation_id: "conv-worker".into(),
            backend: "acp".into(),
            model: "claude".into(),
            custom_agent_id: None,
            status: None,
        },
    ]
}

struct TestEnv {
    server: TeamMcpServer,
    _repo: Arc<MockTeamRepo>,
}

async fn setup() -> TestEnv {
    let repo = Arc::new(MockTeamRepo::new());
    let mailbox = Arc::new(Mailbox::new(repo.clone()));
    let task_board = Arc::new(TaskBoard::new(repo.clone()));
    let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
    let agents = make_agents();
    let scheduler = Arc::new(TeammateManager::new(
        "team-1".into(),
        &agents,
        mailbox,
        task_board,
        broadcaster,
    ));

    let server = TeamMcpServer::start("test-token-123".into(), scheduler)
        .await
        .unwrap();

    TestEnv {
        server,
        _repo: repo,
    }
}

async fn connect_and_init(port: u16, token: &str, slot_id: &str) -> TcpStream {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "auth_token": token,
            "slot_id": slot_id,
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0" }
        }
    });
    send_request(&mut stream, &init_req).await;
    let resp = read_response(&mut stream).await;
    assert!(resp["result"]["serverInfo"]["name"].is_string());

    stream
}

async fn send_request(stream: &mut TcpStream, request: &Value) {
    let data = serde_json::to_vec(request).unwrap();
    write_frame(stream, &data).await.unwrap();
}

async fn read_response(stream: &mut TcpStream) -> Value {
    let frame = read_frame(stream).await.unwrap();
    serde_json::from_slice(&frame).unwrap()
}

async fn call_tool(stream: &mut TcpStream, id: u64, tool: &str, args: Value) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": args
        }
    });
    send_request(stream, &req).await;
    read_response(stream).await
}

fn extract_text(resp: &Value) -> String {
    resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn is_error_response(resp: &Value) -> bool {
    resp["result"]["isError"].as_bool().unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests: Connection & Authentication (MC-1, MC-2, MC-3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mc1_correct_token_connects() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    send_request(&mut stream, &req).await;
    let resp = read_response(&mut stream).await;
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 8);

    env.server.stop();
}

#[tokio::test]
async fn mc2_wrong_token_rejected() {
    let env = setup().await;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", env.server.port()))
        .await
        .unwrap();

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "auth_token": "wrong-token", "slot_id": "s1" }
    });
    send_request(&mut stream, &init_req).await;
    let resp = read_response(&mut stream).await;
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Authentication failed"));

    env.server.stop();
}

#[tokio::test]
async fn mc3_no_token_rejected() {
    let env = setup().await;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", env.server.port()))
        .await
        .unwrap();

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    send_request(&mut stream, &init_req).await;
    let resp = read_response(&mut stream).await;
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Authentication failed"));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: tools/list (TTL-1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tools_list_returns_all_8_tools() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/list"
    });
    send_request(&mut stream, &req).await;
    let resp = read_response(&mut stream).await;
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 8);

    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"team_send_message"));
    assert!(names.contains(&"team_spawn_agent"));
    assert!(names.contains(&"team_task_create"));
    assert!(names.contains(&"team_task_update"));
    assert!(names.contains(&"team_task_list"));
    assert!(names.contains(&"team_members"));
    assert!(names.contains(&"team_rename_agent"));
    assert!(names.contains(&"team_shutdown_agent"));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: team_send_message (TS-1, TS-2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ts1_send_message_to_agent() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_send_message",
        json!({"to": "worker-1", "message": "Hello worker"}),
    )
    .await;

    assert!(!is_error_response(&resp));
    let text = extract_text(&resp);
    assert!(text.contains("worker-1"));

    env.server.stop();
}

#[tokio::test]
async fn ts2_broadcast_message() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_send_message",
        json!({"to": "*", "message": "Attention all"}),
    )
    .await;

    assert!(!is_error_response(&resp));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: team_spawn_agent (SP-1, SP-2, SP-3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sp2_non_whitelisted_backend_rejected() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_spawn_agent",
        json!({"name": "X", "backend": "malicious"}),
    )
    .await;

    assert!(is_error_response(&resp));
    let text = extract_text(&resp);
    assert!(text.contains("not allowed"));

    env.server.stop();
}

#[tokio::test]
async fn sp3_teammate_cannot_spawn() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "worker-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_spawn_agent",
        json!({"name": "Helper", "backend": "claude"}),
    )
    .await;

    assert!(is_error_response(&resp));
    let text = extract_text(&resp);
    assert!(text.contains("Only Lead"));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: team_task_create / team_task_list (TTC-1, TTL-1, TTL-2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ttc1_create_basic_task() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_task_create",
        json!({"subject": "Implement feature X"}),
    )
    .await;

    assert!(!is_error_response(&resp));
    let text = extract_text(&resp);
    assert!(text.contains("Implement feature X"));

    env.server.stop();
}

#[tokio::test]
async fn ttl2_task_list_empty() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(&mut stream, 2, "team_task_list", json!({})).await;

    assert!(!is_error_response(&resp));
    let text = extract_text(&resp);
    let tasks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert!(tasks.is_empty());

    env.server.stop();
}

#[tokio::test]
async fn ttl1_task_list_after_create() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    call_tool(
        &mut stream,
        2,
        "team_task_create",
        json!({"subject": "Task A"}),
    )
    .await;

    let resp = call_tool(&mut stream, 3, "team_task_list", json!({})).await;
    let text = extract_text(&resp);
    let tasks: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["subject"], "Task A");

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: team_members (TM-1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tm1_list_all_members() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(&mut stream, 2, "team_members", json!({})).await;

    assert!(!is_error_response(&resp));
    let text = extract_text(&resp);
    let members: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(members.len(), 2);

    let names: Vec<&str> = members.iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Leader"));
    assert!(names.contains(&"Worker"));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: team_rename_agent (TRA-1, TRA-2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tra1_rename_existing_agent() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_rename_agent",
        json!({"slotId": "worker-1", "newName": "Senior Worker"}),
    )
    .await;

    assert!(!is_error_response(&resp));
    let text = extract_text(&resp);
    assert!(text.contains("renamed"));

    env.server.stop();
}

#[tokio::test]
async fn tra2_rename_nonexistent_agent() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_rename_agent",
        json!({"slotId": "nonexistent", "newName": "X"}),
    )
    .await;

    assert!(is_error_response(&resp));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: team_shutdown_agent (TSA-1, TSA-4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tsa1_lead_sends_shutdown_request() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_shutdown_agent",
        json!({"slotId": "worker-1", "reason": "Task complete"}),
    )
    .await;

    assert!(!is_error_response(&resp));
    let text = extract_text(&resp);
    assert!(text.contains("Shutdown request sent"));

    env.server.stop();
}

#[tokio::test]
async fn tsa4_non_lead_cannot_shutdown() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "worker-1").await;

    let resp = call_tool(
        &mut stream,
        2,
        "team_shutdown_agent",
        json!({"slotId": "lead-1"}),
    )
    .await;

    assert!(is_error_response(&resp));
    let text = extract_text(&resp);
    assert!(text.contains("Only Lead"));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: Unknown method / non-initialize first request
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_method_returns_error() {
    let env = setup().await;
    let mut stream = connect_and_init(env.server.port(), "test-token-123", "lead-1").await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "unknown/method"
    });
    send_request(&mut stream, &req).await;
    let resp = read_response(&mut stream).await;
    assert!(resp["error"]["code"].as_i64().unwrap() == -32601);

    env.server.stop();
}

#[tokio::test]
async fn non_initialize_first_request_rejected() {
    let env = setup().await;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", env.server.port()))
        .await
        .unwrap();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });
    send_request(&mut stream, &req).await;
    let resp = read_response(&mut stream).await;
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("initialize"));

    env.server.stop();
}

// ---------------------------------------------------------------------------
// Tests: Server stop (SS-2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ss2_stop_server_closes_listener() {
    let env = setup().await;
    let port = env.server.port();

    let _stream = connect_and_init(port, "test-token-123", "lead-1").await;
    env.server.stop();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let result = TcpStream::connect(format!("127.0.0.1:{port}")).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Tests: stdio bridge config (SB-1, SB-3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sb1_bridge_config_generation() {
    let env = setup().await;
    let config = aionui_team::TeamMcpStdioConfig::new(
        env.server.port(),
        env.server.auth_token().to_string(),
        "lead-1".into(),
    );

    let env_map = config.to_env_map();
    assert_eq!(
        env_map["TEAM_MCP_PORT"],
        env.server.port().to_string()
    );
    assert_eq!(env_map["TEAM_MCP_TOKEN"], "test-token-123");
    assert_eq!(env_map["TEAM_AGENT_SLOT_ID"], "lead-1");

    env.server.stop();
}

#[tokio::test]
async fn sb3_different_agents_get_different_slot_ids() {
    let env = setup().await;
    let port = env.server.port();
    let token = env.server.auth_token().to_string();

    let cfg_lead = aionui_team::TeamMcpStdioConfig::new(port, token.clone(), "lead-1".into());
    let cfg_worker =
        aionui_team::TeamMcpStdioConfig::new(port, token, "worker-1".into());

    assert_eq!(
        cfg_lead.to_env_map()["TEAM_MCP_PORT"],
        cfg_worker.to_env_map()["TEAM_MCP_PORT"]
    );
    assert_ne!(
        cfg_lead.to_env_map()["TEAM_AGENT_SLOT_ID"],
        cfg_worker.to_env_map()["TEAM_AGENT_SLOT_ID"]
    );

    env.server.stop();
}
