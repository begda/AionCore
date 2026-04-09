# AionUi Backend - Module Index

## Overview

This document serves as the master index for the AionUi backend API specification. The goal is to catalog all interfaces (REST API + IPC) from the original TypeScript/Electron codebase, describe their functional semantics, and guide the Rust rewrite.

**Source project**: `../AionUi-Bak` (Electron + TypeScript)
**Target project**: `aionui-backend` (Rust, Cargo Workspace)

## Approach

- **Source-driven**: Extract interfaces directly from source code, module by module
- **Granularity**: Functional semantic level (what it does, not how it's implemented)
- **Protocol mapping**: Each IPC interface is tagged with target protocol (HTTP / WebSocket / HTTP+WebSocket)
- **Common types**: Candidate common types are tagged inline during module analysis, then extracted into `01-common-types.md` after all modules are complete
- **Cross-session**: This index tracks progress; new sessions read this file to resume

## Document Template

Each module document follows a unified structure:

1. **Overview** - One-sentence module responsibility
2. **REST API** - Endpoint, method, params, response, functional semantics, error scenarios
3. **IPC Interfaces** - Channel name, target protocol, params, functional semantics, dependencies
4. **Data Models** - Core data structures involved
5. **Module Dependencies** - What this module depends on and what depends on it
6. **Candidate Common Types** - Types that may belong in the common crate

## Module List

| # | Module | Document | Source Location | Status |
|---|--------|----------|----------------|--------|
| 1 | Common Types & Traits | 01-common-types.md | (extracted from all modules) | ⬜ Pending (after all modules) |
| 2 | Data Model & Storage | 02-database.md | `src/process/services/database/` | ⬜ Not Started |
| 3 | Auth & User Management | 03-auth.md | `src/process/webserver/auth/`, `src/process/bridge/authBridge.ts` | ⬜ Not Started |
| 4 | System Settings | 04-system-settings.md | `src/process/bridge/systemSettingsBridge.ts` | ⬜ Not Started |
| 5 | Conversation & Messages | 05-conversation.md | `src/process/bridge/conversationBridge.ts`, `src/process/task/` | ⬜ Not Started |
| 6 | AI Agent Integration | 06-ai-agent.md | `src/process/agent/`, `src/process/task/*AgentManager.ts` | ⬜ Not Started |
| 7 | Realtime (WebSocket) | 07-realtime.md | `src/process/webserver/websocket/` | ⬜ Not Started |
| 8 | File & Workspace | 08-file-workspace.md | `src/process/bridge/fsBridge.ts`, `src/process/bridge/documentBridge.ts` | ⬜ Not Started |
| 9 | Channel Integration | 09-channel.md | `src/process/channels/` | ⬜ Not Started |
| 10 | Team Mode | 10-team.md | `src/process/team/` | ⬜ Not Started |
| 11 | Cron Jobs | 11-cron.md | `src/process/services/cron/` | ⬜ Not Started |
| 12 | MCP Protocol | 12-mcp.md | `src/process/services/mcpServices/` | ⬜ Not Started |
| 13 | Extension System | 13-extension.md | `src/process/extensions/` | ⬜ Not Started |
| 14 | App Lifecycle | 14-app-lifecycle.md | `src/process/bridge/updateBridge.ts`, `src/process/bridge/applicationBridge.ts` | ⬜ Not Started |
| 99 | Rust Crate Mapping | 99-rust-crate-mapping.md | (derived from all modules) | ⬜ Pending (after all modules) |

## Analysis Order

Modules are ordered by dependency topology (foundations first):

```
Database (2) → Auth (3) → System Settings (4)
    → Conversation (5) → AI Agent (6) → Realtime (7)
    → File & Workspace (8) → Channel (9) → Team (10)
    → Cron (11) → MCP (12) → Extension (13) → App Lifecycle (14)
    → Common Types (1) → Crate Mapping (99)
```

## Rust Workspace Structure (Preliminary)

```
aionui-backend/
├── Cargo.toml                    # workspace root
├── crates/
│   ├── aionui-common/            # Common types, errors, utilities
│   ├── aionui-db/                # Database layer (SQLite, migrations, Repository traits)
│   ├── aionui-api-types/         # HTTP/WS request/response DTOs
│   ├── aionui-auth/              # Auth & user management
│   ├── aionui-conversation/      # Conversation & message management
│   ├── aionui-ai-agent/          # AI backend integration
│   ├── aionui-realtime/          # WebSocket realtime communication
│   ├── aionui-file/              # File & workspace management
│   ├── aionui-channel/           # Channel integration (Telegram, Slack, etc.)
│   ├── aionui-team/              # Team mode
│   ├── aionui-cron/              # Cron jobs
│   ├── aionui-mcp/               # MCP protocol
│   ├── aionui-extension/         # Extension system
│   ├── aionui-system/            # System settings + app lifecycle
│   └── aionui-app/               # Top-level assembly: routing, startup
```

> This structure is preliminary. Final mapping will be decided in `99-rust-crate-mapping.md` after all modules are analyzed.

## Inter-Crate Communication Principles

- Crates communicate through traits, not concrete implementations
- Dependency direction is strictly downward (no circular dependencies)
- `aionui-app` is the only crate that knows all other crates; it handles dependency injection and assembly
- `aionui-common` is the bottom layer with zero business logic
