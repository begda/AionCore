# AionUi Backend Rewrite - Design Spec

**Date**: 2026-04-09
**Status**: Approved

## Goal

Rewrite the AionUi backend from TypeScript/Electron to Rust, based on the existing API interfaces (REST API + IPC), not by refactoring the source code. The Rust backend will be a standalone HTTP/WebSocket service, serving both Electron and browser clients.

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Approach | Source-driven interface extraction | Ensures no interfaces are missed |
| Granularity | Functional semantic level | Describes "what" not "how", avoids copying bad implementations |
| Architecture | Cargo Workspace + multi-crate | Enforces module decoupling, clear API boundaries |
| Protocol | HTTP + WebSocket | HTTP for request-response, WebSocket for streaming/realtime |
| Common types | Two-pass extraction | Tag candidates during module analysis, extract after all modules complete |
| Frontend | Both Electron and Web | Backend is protocol-agnostic, frontend is a thin client |

## Interface Extraction Process

### Step 1: Module Index
Produce `docs/api-spec/00-module-index.md` listing all modules, responsibilities, source locations, and analysis status.

### Step 2: Per-Module Analysis (Sequential)
For each module, extract from source code:
- REST API endpoints (method, URL, params, response, functional semantics, errors)
- IPC interfaces with target protocol tag (HTTP / WebSocket / HTTP+WebSocket)
- Data models involved
- Module dependencies
- Candidate common types

Analysis order follows dependency topology:
1. Database → Auth → System Settings
2. Conversation → AI Agent → Realtime
3. File & Workspace → Channel → Team
4. Cron → MCP → Extension → App Lifecycle

### Step 3: Common Types Extraction
After all modules are analyzed, review all candidate common types and produce `01-common-types.md`.

### Step 4: Rust Crate Mapping
Produce `99-rust-crate-mapping.md` with final Workspace structure, crate responsibilities, and dependency graph.

## Document Structure

```
docs/api-spec/
├── 00-module-index.md          # Master index with progress tracking
├── 01-common-types.md          # Common types (produced last)
├── 02-database.md              # Data model & storage
├── 03-auth.md                  # Auth & user management
├── 04-system-settings.md       # System settings
├── 05-conversation.md          # Conversation & messages
├── 06-ai-agent.md              # AI backend integration
├── 07-realtime.md              # WebSocket realtime
├── 08-file-workspace.md        # File & workspace
├── 09-channel.md               # Channel integration
├── 10-team.md                  # Team mode
├── 11-cron.md                  # Cron jobs
├── 12-mcp.md                   # MCP protocol
├── 13-extension.md             # Extension system
├── 14-app-lifecycle.md         # App lifecycle
└── 99-rust-crate-mapping.md    # Crate mapping (produced last)
```

## Per-Module Document Template

Each module document follows:

1. **Overview** - One-sentence responsibility
2. **REST API** - Per endpoint: method, URL, params (table), response (table), functional semantics, error scenarios
3. **IPC Interfaces** - Per channel: target protocol (HTTP/WebSocket/HTTP+WebSocket), params, functional semantics, dependencies
4. **Data Models** - Core data structures
5. **Module Dependencies** - Depends on / depended by
6. **Candidate Common Types** - Types that may belong in aionui-common

## Rust Workspace Structure (Preliminary)

```
aionui-backend/
├── Cargo.toml
├── crates/
│   ├── aionui-common/            # Common types, errors, utilities (zero business logic)
│   ├── aionui-db/                # Database layer (SQLite, migrations, Repository traits)
│   ├── aionui-api-types/         # HTTP/WS request/response DTOs
│   ├── aionui-auth/              # Auth & user management
│   ├── aionui-conversation/      # Conversation & message management
│   ├── aionui-ai-agent/          # AI backend integration
│   ├── aionui-realtime/          # WebSocket realtime communication
│   ├── aionui-file/              # File & workspace management
│   ├── aionui-channel/           # Channel integration
│   ├── aionui-team/              # Team mode
│   ├── aionui-cron/              # Cron jobs
│   ├── aionui-mcp/               # MCP protocol
│   ├── aionui-extension/         # Extension system
│   ├── aionui-system/            # System settings + app lifecycle
│   └── aionui-app/               # Top-level assembly
```

### Crate Dependency Direction

```
aionui-common (bottom, no deps)
    ↑
aionui-db (depends on common)
    ↑
aionui-api-types (depends on common)
    ↑
Business crates (depend on common + db + api-types)
    ↑
aionui-app (top, depends on all)
```

### Communication Principles
- Crates communicate through traits, not concrete implementations
- Dependency direction is strictly downward, no circular dependencies
- `aionui-app` is the sole assembler, handling dependency injection
- `aionui-common` has zero business logic

## Cross-Session Support

The `00-module-index.md` status column tracks progress. New sessions:
1. Read `00-module-index.md` to find next module
2. Read relevant AionUi-Bak source code
3. Reference completed module docs for format consistency
4. Produce next module document
