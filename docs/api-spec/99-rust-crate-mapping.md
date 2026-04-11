# 99 - Rust Crate 映射

## 概述

本文档是 AionUi 后端 Rust 重写的最终架构设计。基于模块 01–17 的接口梳理结果，将所有功能映射到 Cargo Workspace 的 crate 结构，明确每个 crate 的职责边界、对外导出、crate 间依赖和推荐的外部依赖。

**核心原则**：

- crate 间通过 trait 通信，不直接依赖具体实现
- 依赖方向严格向下，禁止循环依赖
- `aionui-app` 是唯一知道所有 crate 的顶层组装点
- `aionui-common` 是最底层基础 crate，零业务逻辑
- 业务 crate 之间通过 `Arc<dyn Trait>` 依赖注入

---

## Cargo Workspace 结构

```
aionui-backend/
├── Cargo.toml                        # workspace root
├── crates/
│   ├── aionui-common/                # L0 — 基础类型、错误、工具函数
│   ├── aionui-db/                    # L1 — 数据库层（SQLite、迁移、Repository trait）
│   ├── aionui-api-types/             # L1 — HTTP/WS 请求响应 DTO
│   ├── aionui-realtime/              # L2 — WebSocket 实时通信基础设施
│   ├── aionui-auth/                  # L2 — 认证与用户管理
│   ├── aionui-system/                # L2 — 系统设置 + 模型提供商管理
│   ├── aionui-file/                  # L2 — 文件 I/O、工作区快照、文件监听
│   ├── aionui-ai-agent/              # L3 — AI Agent 进程管理、技能系统、API 客户端
│   ├── aionui-mcp/                   # L3 — MCP 配置管理与 Agent CLI 同步
│   ├── aionui-conversation/          # L3 — 会话与消息管理
│   ├── aionui-channel/               # L4 — 第三方 IM 通道集成
│   ├── aionui-team/                  # L4 — 多 Agent 团队协作
│   ├── aionui-cron/                  # L4 — 定时任务调度
│   ├── aionui-extension/             # L3 — 扩展系统（清单解析、贡献注册、沙箱）
│   ├── aionui-office/                # L2 — Office 文档预览 + 格式转换
│   ├── aionui-shell/                 # L2 — Shell 操作 + 语音转文字
│   └── aionui-app/                   # L5 — 顶层组装：路由注册、依赖注入、启动入口
```

> `Lx` 标注层级：低层级 crate 不依赖高层级 crate。同层级允许在不产生循环的前提下单向依赖。

---

## 分层依赖图

```
L0  aionui-common
        ↑
L1  aionui-db          aionui-api-types
        ↑                    ↑
L2  aionui-auth    aionui-system    aionui-realtime    aionui-file    aionui-office    aionui-shell
        ↑               ↑                ↑                 ↑
L3  aionui-ai-agent    aionui-mcp    aionui-conversation    aionui-extension
        ↑               ↑                ↑
L4  aionui-channel     aionui-team      aionui-cron
        ↑
L5  aionui-app  ← 唯一知道所有 crate 的组装点
```

---

## 各 Crate 详细设计

### 1. aionui-common（L0）

**职责**：零业务逻辑的底层基础设施。所有 crate 都依赖此 crate。

**来源模块**：01-common-types §A（基础类型）、§B（业务枚举）、§C（业务结构体）、§G（常量）

**内容**：

| 分类 | 项目 |
|------|------|
| 错误类型 | `AppError`（NotFound / BadRequest / Unauthorized / Forbidden / Conflict / RateLimited / Internal / BadGateway / Timeout），`impl IntoResponse` |
| 分页 | `PaginatedResult<T>` |
| ID 生成 | `generate_id()` → UUID v7，`generate_prefixed_id(prefix)` |
| 时间戳 | `TimestampMs = i64`，`now_ms()` |
| 加密工具 | `encrypt_string()` / `decrypt_string()`（AES-GCM） |
| 业务枚举 | `AgentType`、`AcpBackend`、`ConversationStatus`、`ConversationSource`、`MessageType`、`MessagePosition`、`MessageStatus`、`ProtocolType`、`RemoteAgentProtocol`、`RemoteAgentAuthType`、`RemoteAgentStatus`、`AgentKillReason`、`PreviewContentType`、`FileChangeOperation` |
| 业务结构体 | `ProviderWithModel`、`Confirmation` / `ConfirmationOption`、`VersionInfo` |
| 常量 | 文件处理、WebSocket、认证、服务器、图片处理常量 |

**对外导出**：上述所有类型和函数。

**外部依赖**：

| crate | 用途 |
|-------|------|
| `thiserror` | 错误类型派生 |
| `serde` / `serde_json` | 序列化 |
| `uuid` | UUID v7 生成 |
| `aes-gcm` | AES-GCM 加解密 |
| `axum` | `IntoResponse` 实现 |

---

### 2. aionui-db（L1）

**职责**：SQLite 数据库层。Schema 定义、迁移、Repository trait 及其 SQLite 实现。

**来源模块**：02-database（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| 表映射结构体 | `User`、`ConversationRow`、`MessageRow`、`CronJobRow`、`RemoteAgentRow`、`ChannelPluginRow`、`TeamRow`、`MailboxMessage`、`TeamTask`、`AssistantUser`、`AssistantSession`、`PairingCode` |
| Repository trait | `IUserRepository`、`IConversationRepository`、`IChannelRepository`、`ITeamRepository`（含 Mailbox + Task）、`ICronRepository` |
| SQLite 实现 | 每个 trait 的 `Sqlite*Repository` 实现 |
| 迁移 | `migrations/` 目录，版本号驱动 |
| 流式缓冲 | `StreamingMessageBuffer`（debounce 批量写入） |
| 数据库生命周期 | 创建/打开（单例）、pragma 配置、损坏恢复 |

**对外导出**：所有表结构体、Repository trait、数据库初始化函数。

**依赖**：`aionui-common`

**外部依赖**：

| crate | 用途 |
|-------|------|
| `sqlx` (sqlite) 或 `rusqlite` | SQLite 驱动 |
| `refinery` 或 `sqlx::migrate!` | Schema 迁移 |

---

### 3. aionui-api-types（L1）

**职责**：HTTP/WebSocket 请求响应 DTO。定义所有与客户端交互的数据结构。

**来源模块**：01-common-types §D（API DTO）、各模块的请求/响应类型

**内容**：

| 分类 | 项目 |
|------|------|
| 统一信封 | `ApiResponse<T>`、`ErrorResponse` |
| WebSocket 格式 | `WebSocketMessage<T>` |
| 各模块 DTO | 见下表 |

**各模块 DTO 索引**：

| 模块 | 请求 DTO | 响应 DTO |
|------|---------|---------|
| 03 - 认证 | `LoginRequest`、`ChangePasswordRequest`、`QrLoginRequest` | `LoginResponse`、`AuthStatusResponse` |
| 04 - 系统设置 | `UpdateSettingsRequest`、`CreateProviderRequest`、`DetectProtocolRequest` | `ProviderResponse`、`ProtocolDetectionResponse` |
| 05 - 会话 | `CreateConversationRequest`、`SendMessageRequest`、`ConfirmRequest`、`CloneConversationRequest` | `ConversationResponse`、`MessageSearchResponse`、`SideQuestionResponse` |
| 06 - AI 后端 | `BedrockTestRequest`、`CreateRemoteAgentRequest` | `RemoteAgentResponse`、`McpConnectionTestResult` |
| 08 - 文件 | `CopyFilesRequest`、`DocumentConversionRequest` | `FileMetadataResponse`、`ConversionResponse` |
| 09 - 通道 | `EnablePluginRequest`、`ApprovePairingRequest` | `PluginStatusResponse`、`PairingRequestResponse` |
| 10 - 团队 | `CreateTeamRequest`、`SendTeamMessageRequest`、`AddAgentRequest` | `TeamResponse`、`TeamAgentResponse` |
| 11 - 定时任务 | `CreateCronJobRequest`、`SaveCronSkillRequest` | `CronJobResponse`、`CronJobExecutedEvent` |
| 12 - MCP | `SyncToAgentsRequest`、`TestMcpConnectionRequest`、`OAuthLoginRequest` | `McpSyncResult`、`DetectedMcpServerResponse` |
| 13 - 扩展 | `InstallExtensionRequest` | `ExtensionSummaryResponse`、`HubExtensionListResponse`、`PermissionSummaryResponse` |
| 14 - 生命周期 | `UpdateCheckRequest`、`UpdateDownloadRequest` | `UpdateCheckResult`、`SystemInfoResponse`、`WebUIStatusResponse`、`QRTokenResult` |
| 16 - Office 预览 | `StartPreviewRequest`、`SaveSnapshotRequest` | `PreviewUrlResponse`、`SnapshotListResponse` |
| 17 - Shell 与语音 | `SpeechToTextRequest` | `SpeechToTextResult` |

**对外导出**：上述所有 DTO 类型。

**依赖**：`aionui-common`

**外部依赖**：`serde`、`serde_json`

---

### 4. aionui-realtime（L2）

**职责**：WebSocket 连接管理、心跳保活、消息路由、事件广播基础设施。

**来源模块**：07-realtime（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| 连接管理 | `WebSocketManager`（`DashMap<ConnectionId, ClientInfo>`） |
| 心跳 | 30s ping / 60s 超时检测 |
| 认证 | Token 提取（Header / Cookie / Sec-WebSocket-Protocol）、JWT 验证委托 |
| 广播 | `EventBroadcaster` trait + `tokio::sync::broadcast` 实现 |
| 消息路由 | 上行消息 `match name { ... }` 分发 |
| 背压控制 | per-connection bounded `mpsc` 通道 |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `EventBroadcaster` trait | 所有需要推送 WebSocket 事件的业务 crate |
| `WebSocketManager` | `aionui-app`（启动时初始化） |

**依赖**：`aionui-common`、`aionui-api-types`

**外部依赖**：

| crate | 用途 |
|-------|------|
| `axum` (ws) | WebSocket 协议升级与消息收发 |
| `tokio` | 异步运行时、broadcast/mpsc 通道、定时任务 |
| `dashmap` | 并发安全的客户端连接表 |

---

### 5. aionui-auth（L2）

**职责**：用户认证、JWT 管理、速率限制、安全中间件。

**来源模块**：03-auth（全部）、14-app-lifecycle §安全中间件

**内容**：

| 分类 | 项目 |
|------|------|
| 用户服务 | 注册、登录、登出、改密、初始引导 |
| JWT | 签发、验证、黑名单、Secret 管理 |
| QR 码登录 | Token 生成/验证/过期（5 分钟 TTL） |
| 速率限制 | 认证端点（5/15min）、通用 API（60/min）、文件操作（30/min）、已认证操作（20/min） |
| 安全中间件 | CSRF（Double Submit Cookie）、安全响应头、Cookie 配置 |
| 密码策略 | bcrypt salt=12、8-128 字符、弱密码黑名单 |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `AuthMiddleware`（axum Layer） | `aionui-app`（路由中间件） |
| `AuthService` trait | WebSocket 模块（Token 验证）、其他需认证的模块 |

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`

**外部依赖**：

| crate | 用途 |
|-------|------|
| `jsonwebtoken` | JWT 签发/验证 |
| `bcrypt` | 密码哈希 |
| `tower` | 中间件层 |
| `tower-http` | 安全头、CORS |
| `dashmap` | Token 黑名单 |

---

### 6. aionui-system（L2）

**职责**：系统偏好设置（键值存储）、模型提供商 CRUD、协议检测、版本更新检查。

**来源模块**：04-system-settings（全部）、14-app-lifecycle §版本更新 + §系统信息

**内容**：

| 分类 | 项目 |
|------|------|
| 后端设置 | `SystemSettings`（语言、通知开关、命令队列、上传路径） |
| 客户端偏好 | 通用键值存储（`ClientPreference` 表） |
| 提供商管理 | `IProvider` CRUD、模型列表拉取（多平台适配）、API Key 加密存储 |
| 协议检测 | URL 推断 + Key 推断 + 多 URL 并行探测 |
| 版本更新 | GitHub Releases API 检查、安装包下载 + 进度推送 |
| 系统信息 | 目录路径、平台、架构查询 |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `IProvider` | conversation、ai-agent、channel、cron |
| `ModelCapability` / `ModelType` | conversation、extension |
| `BedrockConfig` | ai-agent |
| `SystemSettings` | cron（通知开关）、app |
| `VersionInfo` | extension（兼容性校验） |

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-realtime`（语言变更广播）

**外部依赖**：

| crate | 用途 |
|-------|------|
| `reqwest` | HTTP 探测、模型列表拉取、GitHub API |
| `aws-sdk-bedrock` | Bedrock 模型列表 |
| `semver` | 版本比较 |

---

### 7. aionui-file（L2）

**职责**：文件 I/O、目录浏览、图片处理、ZIP 打包、文件监听、工作区快照。

**来源模块**：08-file-workspace §A（核心文件操作）、§D（文件监听）、§E（工作区快照）

**内容**：

| 分类 | 项目 |
|------|------|
| 文件 CRUD | 读/写/删/重命名/拷贝、文件元数据 |
| 目录浏览 | 单层树形、递归扁平（上限 20,000） |
| 图片处理 | 本地 → base64、远程下载 → base64（白名单 + 大小限制） |
| ZIP | 流式打包、可取消 |
| 文件监听 | 单文件监听、工作区 Office 文件监听 |
| 工作区快照 | git-repo / snapshot 双模式、stage/unstage/discard/diff |

**对外导出**：文件操作 trait、文件监听事件。

**依赖**：`aionui-common`、`aionui-realtime`（文件变更事件广播）

**外部依赖**：

| crate | 用途 |
|-------|------|
| `tokio::fs` | 异步文件 I/O |
| `walkdir` + `ignore` | 目录遍历 |
| `notify` | 跨平台文件监听 |
| `zip` | ZIP 打包 |
| `git2` | libgit2 绑定（快照系统） |
| `reqwest` | 远程图片下载 |
| `mime_guess` | MIME 推断 |

---

### 8. aionui-ai-agent（L3）

**职责**：AI Agent 进程编排（CLI 子进程 + Rust crate 直连）、技能系统、API 客户端层、消息中间件。

**来源模块**：06-ai-agent（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| Agent 实现 | `IAgentManager` trait + 6 种实现（ACP、Gemini、Aionrs、OpenClaw、Nanobot、Remote） |
| 任务管理 | `IWorkerTaskManager`（`DashMap<String, Arc<dyn IAgentManager>>`） |
| CLI 进程管理 | 通用 `CliAgentProcess`（tokio::process::Command + stdin/stdout） |
| Remote Agent | WebSocket 协议层（`tokio-tungstenite`）、CRUD REST API、连接测试、设备配对 |
| 技能系统 | `AcpSkillManager`（发现、索引、延迟加载、`[LOAD_SKILL]` 协议） |
| API 客户端 | `RotatingApiClient`（多密钥轮转 + failover）、OpenAI/Gemini/Anthropic 子类 |
| 消息中间件 | think 标签清理、Cron 命令检测 |
| 空闲超时 | ACP 类型 5 分钟空闲清理 |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `IAgentManager` trait | conversation（会话消息收发） |
| `IWorkerTaskManager` trait | conversation、channel、cron |
| `RemoteAgentConfig` | conversation（创建远程会话） |
| `SkillDefinition` / `SkillIndex` | extension（技能贡献解析）、cron（Skill 文件管理） |
| `AcpSessionMcpServer` | mcp（构建注入列表） |
| `AcpMcpCapabilities` | mcp（过滤支持的 transport） |

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-realtime`、`aionui-system`（提供商配置）

**外部依赖**：

| crate | 用途 |
|-------|------|
| `tokio::process` | CLI 子进程管理 |
| `tokio-tungstenite` | Remote Agent WebSocket |
| `ed25519-dalek` | 设备密钥生成（OpenClaw 配对） |
| `reqwest` | API 客户端 HTTP 调用 |
| `regex` | Cron 命令解析、think 标签清理 |
| `aws-sdk-bedrockruntime` | Bedrock 连接测试 |

---

### 9. aionui-mcp（L3）

**职责**：MCP 服务器配置管理、多 Agent CLI 同步、连接测试、OAuth 认证、ACP session 注入。

**来源模块**：12-mcp（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| 配置 CRUD | `IMcpServer` 增删改查（持久化到 DB） |
| Agent 适配器 | `McpAgentAdapter` trait + 9 个实现（Claude、Gemini、Qwen 等） |
| 连接测试 | 临时 MCP Client（connect → listTools → close） |
| Agent 同步 | 服务级锁 + Agent 级锁串行化 |
| OAuth | OAuth 2.0 PKCE 流程、Token 存储 |
| ACP 注入 | `loadBuiltinSessionMcpServers()`、能力过滤、格式转换 |
| 内置 MCP Server | 图片生成（stdio 二进制） |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `IMcpServer` | extension（MCP 服务器贡献）、ai-agent（session 注入） |
| `IMcpServerTransport` | extension、ai-agent |
| `IMcpTool` | extension |
| `McpSource` | extension、ai-agent |

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-system`

**外部依赖**：

| crate | 用途 |
|-------|------|
| `rmcp` 或自实现 | MCP 协议（连接测试） |
| `oauth2` | OAuth 2.0 PKCE |
| `toml` | Aionrs TOML 配置读写 |
| `tokio::process` | CLI 命令执行 |
| `tokio::sync::Mutex` | 串行化锁 |

---

### 10. aionui-conversation（L3）

**职责**：会话 CRUD、消息收发与流式响应、工具调用确认系统、消息搜索、ACP 后端管理。

**来源模块**：05-conversation（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| 会话 CRUD | 创建、查询、更新、删除、克隆 |
| 消息收发 | 发送用户消息 → 异步 Agent 处理 → WebSocket 流式推送 |
| 确认系统 | 待确认列表管理、`alwaysAllow` 审批记忆 |
| 辅助查询 | Side-question（仅 ACP Claude） |
| 消息搜索 | 跨会话全文检索 |
| ACP 管理 | CLI 检测、Agent 列表、健康检查、会话模式/模型切换 |
| 工作区浏览 | 会话关联目录浏览 |

**Service trait**：

```rust
trait IConversationService {
    fn create(&self, params: CreateConversationParams) -> Result<Conversation>;
    fn get(&self, id: &str) -> Result<Option<Conversation>>;
    fn update(&self, id: &str, updates: UpdateConversation) -> Result<Conversation>;
    fn delete(&self, id: &str) -> Result<()>;
    fn list(&self, filters: ConversationFilters) -> Result<PaginatedResult<Conversation>>;
    fn clone_create(&self, params: CloneConversationParams) -> Result<Conversation>;
}
```

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-realtime`、`aionui-system`、`aionui-ai-agent`（`IWorkerTaskManager`）、`aionui-file`（附件处理）

---

### 11. aionui-extension（L3）

**职责**：扩展清单解析、贡献注册、依赖管理、沙箱隔离、Hub 市场。

**来源模块**：13-extension（全部）、08-file-workspace §B（技能与规则管理）

**内容**：

| 分类 | 项目 |
|------|------|
| 清单解析 | `ExtensionManifest` Zod → serde 校验 |
| 贡献解析 | 10 种解析器（ACP 适配器、MCP 服务器、助手、代理、技能、主题、频道插件、WebUI、设置选项卡、模型提供商） |
| 依赖管理 | semver 版本匹配、拓扑排序、循环检测 |
| 沙箱 | WASM（`wasmtime`）或进程隔离 |
| 生命周期 | onInstall / onActivate / onDeactivate / onUninstall |
| 热重载 | 文件监听 → 增量或全量重建 |
| Hub 市场 | 索引管理、下载、安装、更新 |
| 权限系统 | 声明式权限 + 风险等级分析 |
| 技能管理 | 内置/用户技能 CRUD、技能市场、外部路径管理 |
| 规则管理 | 助手级 Rule/Skill CRUD（含 locale 回退） |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `ResolvedModelProvider` | system-settings（合并扩展提供商） |
| `ResolvedAssistant` | conversation（助手选择） |
| `ResolvedAcpAdapter` | ai-agent（扩展 ACP 后端） |
| `ExtMcpServer` | mcp（合并到配置列表） |
| `ExtensionManifest` | 内部 |

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-realtime`

**外部依赖**：

| crate | 用途 |
|-------|------|
| `semver` | 版本范围匹配 |
| `wasmtime` | WASM 沙箱（可选） |
| `notify` | 扩展目录热重载监听 |
| `tokio::process` | 生命周期钩子执行 |

---

### 12. aionui-channel（L4）

**职责**：第三方 IM 平台接入（Telegram、飞书、钉钉、微信），统一消息协议，配对授权，per-chat 会话隔离。

**来源模块**：09-channel（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| 插件抽象 | `ChannelPlugin` trait（initialize / start / stop / sendMessage / editMessage） |
| 平台实现 | Telegram（长轮询）、飞书（WebSocket）、钉钉（WebSocket Stream）、微信（长轮询） |
| 统一消息 | `IUnifiedIncomingMessage` / `IUnifiedOutgoingMessage` |
| Action 系统 | platform / system / chat 三类动作路由 |
| 配对授权 | 6 位配对码、10 分钟有效期、配对审批 |
| 微信登录 | SSE 推送 QR 码扫码状态 |
| 设置同步 | Agent/模型配置同步到运行时 |

**Rust feature flags**：

```toml
[features]
default = ["telegram"]
telegram = ["dep:reqwest"]   # grammY → reqwest 长轮询
lark = ["dep:reqwest"]
dingtalk = ["dep:reqwest", "dep:tokio-tungstenite"]
weixin = ["dep:reqwest"]
```

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-realtime`、`aionui-conversation`、`aionui-ai-agent`

**外部依赖**：`reqwest`、`tokio-tungstenite`（钉钉 WebSocket）

---

### 13. aionui-team（L4）

**职责**：多 Agent 团队协作——邮箱通信、任务板、Agent 调度状态机、Team MCP Server。

**来源模块**：10-team（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| 团队管理 | 创建/删除/重命名、Agent 增删改 |
| 会话协调 | `TeamSession`（懒初始化） |
| 调度引擎 | `TeammateManager` 状态机（idle → working → finalizeTurn） |
| 邮箱 | `Mailbox`（原子读取+标记已读） |
| 任务板 | `TaskManager`（blockedBy/blocks 双向链接、依赖解除） |
| MCP Server | TCP MCP Server（127.0.0.1 随机端口）+ stdio 桥接 |
| MCP 工具 | `team_send_message`、`team_spawn_agent`、`team_task_*`、`team_members`、`team_rename_agent`、`team_shutdown_agent` |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `TTeam` | conversation（teamId 关联） |
| `TeamAgent` | conversation（团队会话） |
| `TeammateRole` / `TeammateStatus` | WebSocket 事件载荷 |

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-realtime`、`aionui-conversation`、`aionui-ai-agent`

**外部依赖**：`tokio`（TCP server、通道、定时器）、`rmcp` 或自实现（MCP Server）

---

### 14. aionui-cron（L4）

**职责**：定时任务调度——cron 表达式 / 固定间隔 / 一次性，忙碌重试，Skill 文件管理。

**来源模块**：11-cron（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| 调度核心 | `CronService`（三种调度类型：at / every / cron） |
| 任务执行 | `WorkerTaskManagerJobExecutor`（existing / new_conversation 模式） |
| 忙碌守卫 | `CronBusyGuard`（30s 重试 × 3 次） |
| Skill 管理 | Skill 文件读写/验证/删除 |
| Skill 建议 | `SkillSuggestWatcher`（SHA-256 去重） |
| 系统恢复 | 休眠唤醒后错过任务检测与补执行 |

**对外导出**：

| 导出 | 消费方 |
|------|--------|
| `CronJob` | conversation（关联查询） |
| `CronSchedule` | api-types（创建请求） |

**依赖**：`aionui-common`、`aionui-db`、`aionui-api-types`、`aionui-realtime`、`aionui-conversation`、`aionui-ai-agent`、`aionui-system`（通知开关）

**外部依赖**：

| crate | 用途 |
|-------|------|
| `cron` | Cron 表达式解析 |
| `tokio::time` | interval / sleep_until |

---

### 15. aionui-office（L2）

**职责**：Office 文档预览（officecli 子进程管理）、格式转换、快照历史、Star Office 探测。

**来源模块**：16-office-preview（全部）、08-file-workspace §C（文档格式转换）

**内容**：

| 分类 | 项目 |
|------|------|
| 预览管理 | `OfficecliWatchManager`（统一 Word/Excel/PPT 子进程管理） |
| 反向代理 | SSRF 防护（端口白名单）、HTML 注入导航守卫、Location 重写 |
| 格式转换 | Word → Markdown、Excel → JSON、PPT → JSON |
| 快照历史 | SHA-1 目录、index.json 索引、上限 50 个快照 |
| Star Office | 端口扫描（±24）、并发探测（6 workers）、结果缓存 |
| officecli 管理 | 自动安装/更新检查 |

**依赖**：`aionui-common`、`aionui-api-types`、`aionui-realtime`

**外部依赖**：

| crate | 用途 |
|-------|------|
| `tokio::process` | officecli 子进程 |
| `reqwest` | 反向代理、Star Office 探测 |
| `calamine` | Excel 读取 |
| `sha1` | 快照目录 SHA-1 |

---

### 16. aionui-shell（L2）

**职责**：Shell 操作（打开文件/目录/URL、工具检测）、语音转文字（STT API 代理）。

**来源模块**：17-shell-voice（全部）

**内容**：

| 分类 | 项目 |
|------|------|
| Shell 操作 | `open-file`、`show-item-in-folder`、`open-external`、`check-tool-installed`、`open-folder-with` |
| STT 服务 | `SpeechToTextProvider` trait + OpenAI / Deepgram 实现 |

**依赖**：`aionui-common`、`aionui-api-types`、`aionui-system`（STT 配置）

**外部依赖**：

| crate | 用途 |
|-------|------|
| `open` | 跨平台打开文件/URL |
| `which` | 工具检测 |
| `reqwest` | STT API 调用 |

---

### 17. aionui-app（L5）

**职责**：顶层组装——HTTP 路由注册、依赖注入（DI）、启动入口、优雅关闭。

**来源模块**：14-app-lifecycle §服务器配置，以及所有模块的路由定义

**内容**：

| 分类 | 项目 |
|------|------|
| 路由注册 | 将所有模块的 REST API 挂载到 axum Router |
| 依赖注入 | 构造所有 Service/Repository 实例，注入到路由 handler |
| 中间件 | 认证、CSRF、安全头、速率限制、错误处理、CORS、请求体大小限制 |
| WebSocket | WebSocket 升级处理，挂载到 `axum::Router` |
| 静态文件 | 扩展 WebUI 静态资源服务 |
| 启动配置 | 端口、监听地址、远程访问、TLS（可选） |
| 优雅关闭 | 信号处理（SIGTERM/SIGINT）、WebSocket 连接关闭、子进程清理、数据库关闭 |
| 初始化 | 数据库迁移、Cron 调度启动、扩展加载、通道插件启动 |

**依赖**：**所有 crate**（这是唯一允许依赖全部 crate 的地方）

**外部依赖**：

| crate | 用途 |
|-------|------|
| `axum` | HTTP 框架 |
| `tower` / `tower-http` | 中间件层 |
| `tokio` | 异步运行时 |
| `tracing` / `tracing-subscriber` | 结构化日志 |
| `clap` | 命令行参数解析 |

---

## Pet 系统（不需要后端 crate）

**来源模块**：15-pet

Pet 系统完全由前端实现，后端无需新增 crate。所需能力已在其他模块覆盖：

| 需求 | 已有模块 |
|------|---------|
| 设置持久化（开关/尺寸/DnD/确认） | 04 - 系统设置（客户端键值存储） |
| AI 活动事件（thinking/working/done/error） | 07 - WebSocket 事件广播 |
| 工具调用确认 | 05 - 会话确认系统 |

---

## Crate 间依赖矩阵

> `→` 表示左侧依赖右侧。空格表示无依赖。

| | common | db | api-types | realtime | auth | system | file | ai-agent | mcp | conversation | extension | channel | team | cron | office | shell |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **db** | → | | | | | | | | | | | | | | | |
| **api-types** | → | | | | | | | | | | | | | | | |
| **realtime** | → | | → | | | | | | | | | | | | | |
| **auth** | → | → | → | | | | | | | | | | | | | |
| **system** | → | → | → | → | | | | | | | | | | | | |
| **file** | → | | | → | | | | | | | | | | | | |
| **office** | → | | → | → | | | | | | | | | | | | |
| **shell** | → | | → | | | → | | | | | | | | | | |
| **ai-agent** | → | → | → | → | | → | | | | | | | | | | |
| **mcp** | → | → | → | | | → | | | | | | | | | | |
| **conversation** | → | → | → | → | | → | → | → | | | | | | | | |
| **extension** | → | → | → | → | | | | | | | | | | | | |
| **channel** | → | → | → | → | | | | → | | → | | | | | | |
| **team** | → | → | → | → | | | | → | | → | | | | | | |
| **cron** | → | → | → | → | | → | | → | | → | | | | | | |
| **app** | → | → | → | → | → | → | → | → | → | → | → | → | → | → | → | → |

---

## serde 序列化策略

| 层 | 风格 | 原因 |
|----|------|------|
| API DTO（`aionui-api-types`） | `#[serde(rename_all = "camelCase")]` | 与前端 JavaScript 命名一致 |
| 数据库模型（`aionui-db`） | `snake_case` | 与 SQL 列名一致 |
| 内部结构体 | `snake_case` | Rust 惯例 |

Service 层负责 DB 模型 ↔ API DTO 的转换。

---

## Crate 间通信模式

### 1. trait + `Arc<dyn Trait>` 注入

```rust
// aionui-ai-agent 定义 trait
pub trait IWorkerTaskManager: Send + Sync {
    fn get_or_build_task(&self, id: &str, opts: BuildOptions) -> Result<Arc<dyn IAgentManager>>;
    fn kill(&self, id: &str, reason: Option<AgentKillReason>) -> Result<()>;
}

// aionui-conversation 通过 trait 使用，不直接依赖具体实现
pub struct ConversationService {
    task_manager: Arc<dyn IWorkerTaskManager>,
    // ...
}

// aionui-app 负责组装
let task_manager = Arc::new(WorkerTaskManagerImpl::new(/* deps */));
let conversation_service = ConversationService::new(task_manager.clone());
```

### 2. 事件广播（`tokio::sync::broadcast`）

```rust
// aionui-realtime 定义 trait
pub trait EventBroadcaster: Send + Sync {
    fn broadcast(&self, event: WebSocketMessage<serde_json::Value>);
}

// 任何业务模块通过此 trait 发布 WebSocket 事件
```

### 3. 数据库 Repository（`Arc<dyn IRepository>`）

```rust
// aionui-db 定义 trait
pub trait IConversationRepository: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<ConversationRow>>;
    fn create(&self, row: &ConversationRow) -> Result<()>;
    // ...
}

// 业务模块通过 trait 操作数据库
```

---

## 外部依赖汇总

### 核心框架

| crate | 版本 | 用途 |
|-------|------|------|
| `tokio` | 1.x | 异步运行时 |
| `axum` | 0.8.x | HTTP/WebSocket 框架 |
| `tower` / `tower-http` | 0.5.x | 中间件 |
| `serde` / `serde_json` | 1.x | 序列化 |
| `tracing` | 0.1.x | 结构化日志 |
| `clap` | 4.x | CLI 参数 |

### 数据库

| crate | 用途 |
|-------|------|
| `sqlx` (sqlite) 或 `rusqlite` | SQLite 驱动 |
| `refinery` | Schema 迁移 |

### 认证/安全

| crate | 用途 |
|-------|------|
| `jsonwebtoken` | JWT |
| `bcrypt` | 密码哈希 |
| `aes-gcm` | AES-GCM 加密 |
| `ed25519-dalek` | Ed25519 密钥（OpenClaw 配对） |

### 网络/协议

| crate | 用途 |
|-------|------|
| `reqwest` | HTTP 客户端 |
| `tokio-tungstenite` | WebSocket 客户端 |

### 工具

| crate | 用途 |
|-------|------|
| `uuid` | UUID v7 |
| `semver` | 版本比较 |
| `regex` | 正则匹配 |
| `dashmap` | 并发 HashMap |
| `cron` | Cron 表达式 |
| `notify` | 文件监听 |
| `git2` | Git 操作 |
| `calamine` | Excel 读取 |
| `zip` | ZIP 打包 |
| `thiserror` | 错误类型 |
| `open` | 跨平台打开文件/URL |
| `which` | 可执行文件查找 |
| `mime_guess` | MIME 推断 |

### AI/协议

| crate | 用途 |
|-------|------|
| `aws-sdk-bedrock` / `aws-sdk-bedrockruntime` | Bedrock 集成 |
| `rmcp` 或自实现 | MCP 协议 |
| `oauth2` | OAuth 2.0 |
| `toml` | TOML 配置读写 |

---

## 设计决策

### 1. 为什么 Pet 不需要后端 crate

Pet 系统的所有逻辑（状态机、空闲计时、眼球追踪、拖拽、动画渲染）都是纯前端行为。后端只需提供设置持久化（已在 `aionui-system`）和 AI 事件推送（已在 `aionui-realtime`），无需专用 crate。

### 2. 为什么 extension 和 file 中的技能管理合并到 extension

原实现中 `fsBridge.ts` 的技能/规则管理功能属于 AI 助手的资源管理，与文件 I/O 解耦后归入 `aionui-extension`。`aionui-file` 只负责通用文件操作。

### 3. 为什么 system 和 app-lifecycle 合并

系统设置（模块 04）和应用生命周期（模块 14）在 Rust 后端中高度重叠——服务器配置、版本管理、系统信息都属于"系统级"功能。合并为 `aionui-system` 减少 crate 数量，简化依赖。版本更新（GitHub Releases 检查/下载）也归入此 crate。

### 4. 为什么 channel 使用 feature flags

四个 IM 平台的 SDK 差异大且各有独立的外部依赖。使用 Cargo feature flags 允许按需编译，减小二进制体积和编译时间。不需要某个平台时完全移除其代码和依赖。

### 5. 为什么 Aionrs 直接集成而非子进程

Aionrs 本身是 Rust 实现，作为 Cargo workspace 的 crate 依赖直接引入，避免子进程调用的 IPC 开销。其他 CLI（Claude、Gemini、OpenClaw、Nanobot）保持子进程模式，因为它们是第三方二进制。

### 6. SQLite vs 多数据库

选择 SQLite 作为唯一数据库引擎：
- 嵌入式部署，零运维
- 原实现即使用 SQLite，迁移成本低
- WAL 模式下并发读写性能足够
- 如需未来支持 PostgreSQL，Repository trait 抽象已预留扩展点

### 7. axum vs actix-web

选择 `axum`：
- 基于 `tower`，中间件生态丰富
- 与 `tokio` 深度集成
- 类型安全的 extractor 模式
- 内置 WebSocket 支持
- 社区活跃度和文档质量更高

---

## 实施路径建议

按依赖拓扑从底到上实施，每个阶段可独立编译和测试：

| 阶段 | Crate | 说明 |
|------|-------|------|
| 1 | `aionui-common` | 基础类型，可立即开始 |
| 2 | `aionui-db`、`aionui-api-types` | 数据层 + DTO，可并行 |
| 3 | `aionui-realtime`、`aionui-auth`、`aionui-system`、`aionui-file`、`aionui-office`、`aionui-shell` | L2 层 crate，可并行 |
| 4 | `aionui-ai-agent`、`aionui-mcp`、`aionui-conversation`、`aionui-extension` | L3 层核心业务 |
| 5 | `aionui-channel`、`aionui-team`、`aionui-cron` | L4 层高级功能 |
| 6 | `aionui-app` | 顶层组装，最后实施 |
