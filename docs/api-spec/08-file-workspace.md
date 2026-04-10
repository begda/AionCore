# 08 - 文件与工作区

## 概述

管理文件系统操作（读写、列表、拷贝、删除、重命名）、文档格式转换（Word/Excel/PPT）、文件监听（单文件 + 工作区 Office 文件）、工作区快照（基于 git 的变更追踪）、以及 AI 助手的 Skill/Rule 资源管理。

**源码位置**：`process/bridge/fsBridge.ts`、`process/bridge/documentBridge.ts`、`process/bridge/fileWatchBridge.ts`、`process/bridge/workspaceSnapshotBridge.ts`、`process/services/conversionService.ts`、`process/services/WorkspaceSnapshotService.ts`

> **设计决策**：原实现中 `fsBridge.ts`（1673 行）混合了核心文件 I/O 和 Skill/Rule 资源管理两个完全不同的关注点。Rust 重写时应将 Skill/Rule 管理拆分到独立模块（建议归入 `13-extension` 扩展系统或独立为 `aionui-skill` crate），本文档按职责分节描述，并在各节标注归属建议。

## 子模块划分

| 子模块 | 原始源码 | Rust 归属建议 |
|--------|---------|--------------|
| 核心文件操作 | `fsBridge.ts`（部分） | `aionui-file` |
| 技能与规则管理 | `fsBridge.ts`（部分） | `aionui-extension` 或独立 `aionui-skill` |
| 文档格式转换 | `documentBridge.ts`、`conversionService.ts` | `aionui-office` |
| 文件监听 | `fileWatchBridge.ts` | `aionui-file` |
| 工作区快照 | `workspaceSnapshotBridge.ts`、`WorkspaceSnapshotService.ts` | `aionui-file` |

---

## A. 核心文件操作

### IPC 接口

#### 目录与文件列表

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.getFilesByDir` | HTTP | `{ dir: string, root: string }` | `IDirOrFile[]` | 获取指定目录下的文件和子目录（单层，带树形子节点） |
| `fs.listWorkspaceFiles` | HTTP | `{ root: string }` | `IWorkspaceFlatFile[]` | 递归列出工作区内所有文件（扁平列表，上限 20,000 个） |
| `fs.getFileMetadata` | HTTP | `{ path: string }` | `IFileMetadata` | 获取文件元数据（名称、大小、修改时间、类型） |

#### 文件读写

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.readFile` | HTTP | `{ path: string }` | `string \| null` | 读取文件内容（UTF-8 文本，上限 256MB） |
| `fs.readFileBuffer` | HTTP | `{ path: string }` | `ArrayBuffer \| null` | 读取文件为二进制（用于非文本文件） |
| `fs.writeFile` | HTTP | `{ path: string, data: Uint8Array \| string }` | `boolean` | 写入文件内容，成功后触发 `fileStream.contentUpdate` 事件 |

#### 文件管理

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.copyFilesToWorkspace` | HTTP | `{ filePaths: string[], workspace: string, sourceRoot?: string }` | `IBridgeResponse<{ copiedFiles, failedFiles }>` | 批量拷贝文件到工作区，保留目录结构 |
| `fs.removeEntry` | HTTP | `{ path: string }` | `IBridgeResponse` | 删除文件或目录，删除后触发 `fileStream.contentUpdate`（operation: `delete`） |
| `fs.renameEntry` | HTTP | `{ path: string, newName: string }` | `IBridgeResponse<{ newPath: string }>` | 重命名文件或目录 |
| `fs.createTempFile` | HTTP | `{ fileName: string }` | `string` | 在临时目录创建空文件，返回绝对路径 |

#### 图片处理

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.getImageBase64` | HTTP | `{ path: string }` | `string` | 将本地图片文件转为 base64 Data URL |
| `fs.fetchRemoteImage` | HTTP | `{ url: string }` | `string` | 下载远程图片并转为 base64 Data URL |

**远程图片安全策略**：
- 仅允许白名单主机：`github.com`、`raw.githubusercontent.com` 等
- 仅允许 HTTP/HTTPS 协议
- 最多跟踪 5 次重定向
- 单张图片大小上限 5MB
- 失败时返回占位 SVG 而非抛出错误

#### ZIP 打包

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.createZip` | HTTP | `{ path: string, requestId?: string, files: ZipFileEntry[] }` | `boolean` | 创建 ZIP 压缩文件，支持通过 `requestId` 取消 |
| `fs.cancelZip` | HTTP | `{ requestId: string }` | `boolean` | 取消正在进行的 ZIP 打包操作 |

```
ZipFileEntry {
  name: string          // ZIP 内路径
  content: string       // 文件内容（文本）
  // 或
  filePath: string      // 从磁盘读取的文件路径
}
```

### 限制与缓存

| 参数 | 值 | 说明 |
|------|-----|------|
| 文件读取上限 | 256 MB | `readFile` / `readFileBuffer` |
| 远程图片上限 | 5 MB | `fetchRemoteImage` |
| 工作区文件列表上限 | 20,000 个 | `listWorkspaceFiles` |
| 重定向最大次数 | 5 次 | `fetchRemoteImage` |

- `listWorkspaceFiles` 有内存缓存，文件变更时失效
- 超过 20,000 文件时返回前 20,000 个并截断

### 下行事件

| 事件名 | 数据 | 触发时机 |
|--------|------|---------|
| `fileStream.contentUpdate` | `{ filePath, content, workspace, relativePath, operation }` | `writeFile` 或 `removeEntry` 成功后 |

`operation` 取值：`write` \| `delete`

---

## B. 技能与规则管理

> **设计决策**：以下接口在原实现中位于 `fsBridge.ts`，但功能语义属于 AI 助手的资源管理（Skill = 可复用提示词模板，Rule = 助手行为约束）。Rust 重写时建议迁入扩展系统模块或独立模块，与文件 I/O 解耦。

### IPC 接口

#### 内置资源读取

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.readBuiltinRule` | HTTP | `{ fileName: string }` | `string` | 读取内置规则文件（.md，追加拼接模式） |
| `fs.readBuiltinSkill` | HTTP | `{ fileName: string }` | `string` | 读取内置技能文件（.md） |

#### 助手级 Rule/Skill CRUD

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.readAssistantRule` | HTTP | `{ assistantId: string, locale?: string }` | `string` | 读取助手规则，支持 locale 回退（zh-CN → en-US → 默认） |
| `fs.writeAssistantRule` | HTTP | `{ assistantId: string, content: string, locale?: string }` | `boolean` | 写入助手规则到用户目录 |
| `fs.deleteAssistantRule` | HTTP | `{ assistantId: string }` | `boolean` | 删除助手的所有 locale 版本规则 |
| `fs.readAssistantSkill` | HTTP | `{ assistantId: string, locale?: string }` | `string` | 读取助手技能，支持 locale 回退 |
| `fs.writeAssistantSkill` | HTTP | `{ assistantId: string, content: string, locale?: string }` | `boolean` | 写入助手技能到用户目录 |
| `fs.deleteAssistantSkill` | HTTP | `{ assistantId: string }` | `boolean` | 删除助手的所有 locale 版本技能 |

**Locale 回退逻辑**：
1. 尝试 `{assistantId}.{locale}.md`（如 `abc123.zh-CN.md`）
2. 回退到 `{assistantId}.md`（无 locale 后缀）
3. 未找到返回空字符串

#### 技能市场与外部技能

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.listAvailableSkills` | HTTP | 无 | `SkillListItem[]` | 列出所有可用技能（内置 + 用户自定义，去重） |
| `fs.readSkillInfo` | HTTP | `{ skillPath: string }` | `IBridgeResponse<{ name, description }>` | 读取 SKILL.md front matter（不导入） |
| `fs.importSkill` | HTTP | `{ skillPath: string }` | `IBridgeResponse<{ skillName }>` | 将技能目录拷贝到用户技能目录 |
| `fs.importSkillWithSymlink` | HTTP | `{ skillPath: string }` | `IBridgeResponse<{ skillName }>` | 通过 junction symlink 导入技能（实时同步） |
| `fs.exportSkillWithSymlink` | HTTP | `{ skillPath: string, targetDir: string }` | `IBridgeResponse` | 通过 symlink 导出技能到外部目录 |
| `fs.deleteSkill` | HTTP | `{ skillName: string }` | `IBridgeResponse` | 删除用户自定义技能（含安全校验） |
| `fs.scanForSkills` | HTTP | `{ folderPath: string }` | `IBridgeResponse<ScannedSkill[]>` | 扫描目录下含 SKILL.md 的子目录 |
| `fs.detectCommonSkillPaths` | HTTP | 无 | `IBridgeResponse<NamedPath[]>` | 检测常见技能路径（.agents、.gemini、.claude 等） |
| `fs.detectAndCountExternalSkills` | HTTP | 无 | `IBridgeResponse<ExternalSkillSource[]>` | 从多个来源发现外部技能并统计数量 |
| `fs.getSkillPaths` | HTTP | 无 | `{ userSkillsDir, builtinSkillsDir }` | 获取技能目录路径 |

#### 自定义外部路径管理

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fs.getCustomExternalPaths` | HTTP | 无 | `NamedPath[]` | 获取用户自定义的外部技能路径列表 |
| `fs.addCustomExternalPath` | HTTP | `{ name: string, path: string }` | `IBridgeResponse` | 添加自定义外部技能路径 |
| `fs.removeCustomExternalPath` | HTTP | `{ path: string }` | `IBridgeResponse` | 移除自定义外部技能路径 |
| `fs.enableSkillsMarket` | HTTP | 无 | `IBridgeResponse` | 注入 aionui-skills 市场技能 |
| `fs.disableSkillsMarket` | HTTP | 无 | `IBridgeResponse` | 移除 aionui-skills 市场技能 |

### 数据模型

```
SkillListItem {
  name: string              // 技能名称
  description: string       // 技能描述
  location: string          // 技能目录路径
  isCustom: boolean         // true=用户自定义，false=内置
}

ScannedSkill {
  name: string              // 从 SKILL.md front matter 提取
  description: string
  path: string              // 技能目录路径
}

NamedPath {
  name: string              // 路径显示名
  path: string              // 绝对路径
}

ExternalSkillSource {
  name: string              // 来源名
  path: string              // 来源路径
  skillCount: number        // 含技能数量
  skills: ScannedSkill[]
}
```

---

## C. 文档格式转换

### IPC 接口

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `document.convert` | HTTP | `DocumentConversionRequest` | `DocumentConversionResponse` | 统一入口：按目标格式路由到对应转换器 |

### 请求与响应

```
DocumentConversionRequest {
  filePath: string                       // 源文件绝对路径
  to: 'markdown' | 'excel-json' | 'ppt-json'   // 目标格式
}

DocumentConversionResponse {
  to: string                             // 回显目标格式
  result: ConversionResult<T>            // 转换结果
}

ConversionResult<T> {
  success: boolean
  data?: T                               // 转换后的数据
  error?: string                         // 失败原因
}
```

### 支持的转换

| 源格式 | 目标格式 | 转换器方法 | 说明 |
|--------|---------|-----------|------|
| `.doc` / `.docx` | `markdown` | `wordToMarkdown` | Word → Markdown（基于 mammoth） |
| `.xls` / `.xlsx` | `excel-json` | `excelToJson` | Excel → 结构化 JSON（含合并单元格、图片） |
| `.ppt` / `.pptx` | `ppt-json` | `pptToJson` | PPT → 结构化 JSON（基于 pptx2json） |

**反向转换**（conversionService 中存在但未通过 IPC 暴露）：

| 方法 | 说明 | 状态 |
|------|------|------|
| `jsonToExcel` | JSON → XLSX 重建 | 内部使用 |
| `markdownToWord` | Markdown → Word | 基础实现，需完善 |
| `htmlToPdf` | HTML → PDF | 依赖 Electron BrowserWindow.printToPDF |
| `markdownToPdf` | Markdown → PDF | 包装 htmlToPdf |

> **设计决策**：`htmlToPdf` 和 `markdownToPdf` 依赖 Electron 的 `BrowserWindow.printToPDF`，Rust 后端需要替代方案（如 headless Chrome / wkhtmltopdf / weasyprint）。反向转换如果 Rust 后端需要暴露，应作为独立 API 端点。

### 数据模型

```
ExcelWorkbookData {
  sheets: ExcelSheetData[]
}

ExcelSheetData {
  name: string                // 工作表名
  data: any[][]               // 二维单元格数据
  merges?: CellRange[]        // 合并单元格区域
  images?: ExcelSheetImage[]  // 嵌入图片
}

CellRange {
  s: { r: number, c: number }    // 起始行列
  e: { r: number, c: number }    // 结束行列
}

ExcelSheetImage {
  row: number
  col: number
  src: string         // Data URL
  width?: number      // 像素
  height?: number
  alt?: string
}

PPTJsonData {
  slides: PPTSlideData[]
  raw?: any               // 原始 PPTX JSON 结构
}

PPTSlideData {
  slideNumber: number
  content: any            // PPTX JSON 结构
}
```

---

## D. 文件监听

### IPC 接口

#### 单文件监听

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fileWatch.startWatch` | HTTP | `{ filePath: string }` | `IBridgeResponse` | 开始监听指定文件变更 |
| `fileWatch.stopWatch` | HTTP | `{ filePath: string }` | `IBridgeResponse` | 停止监听指定文件 |
| `fileWatch.stopAllWatches` | HTTP | 无 | `IBridgeResponse` | 停止所有文件监听 |

#### 工作区 Office 文件监听

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `workspaceOfficeWatch.start` | HTTP | `{ workspace: string }` | `IBridgeResponse` | 监听工作区内新增的 Office 文件 |
| `workspaceOfficeWatch.stop` | HTTP | `{ workspace: string }` | `IBridgeResponse` | 停止工作区 Office 文件监听 |

### 下行事件

| 事件名 | 数据 | 触发时机 |
|--------|------|---------|
| `fileWatch.fileChanged` | `{ filePath: string, eventType: string }` | 被监听文件发生变更 |
| `workspaceOfficeWatch.fileAdded` | `{ filePath: string, workspace: string }` | 工作区新增 `.pptx` / `.docx` / `.xlsx` 文件 |

### 实现细节

- 使用 `fs.watch()` 监听文件变更（非递归模式在 Linux 上不支持子目录）
- 工作区监听在 macOS/Windows 上支持递归（`recursive: true`）
- 工作区 Office 监听过滤正则：`/\.(pptx|docx|xlsx)$/i`
- 事件去重：使用 `Set` 防止同一文件重复触发

> **设计决策**：Linux 不支持 `fs.watch` 递归模式。Rust 重写建议使用 `notify` crate，它在所有平台上统一支持递归监听，并有更好的事件合并（debounce）机制。

---

## E. 工作区快照

基于 git 的工作区变更追踪系统。支持两种模式：已有 git 仓库直接使用；非 git 目录自动创建临时仓库。

### IPC 接口

| 通道名 | 目标协议 | 参数 | 返回值 | 功能语义 |
|--------|---------|------|--------|---------|
| `fileSnapshot.init` | HTTP | `{ workspace: string }` | `SnapshotInfo` | 初始化快照（自动检测 git-repo 或 snapshot 模式） |
| `fileSnapshot.getInfo` | HTTP | `{ workspace: string }` | `SnapshotInfo` | 获取快照模式和分支信息 |
| `fileSnapshot.compare` | HTTP | `{ workspace: string }` | `CompareResult` | 获取已暂存/未暂存的文件变更列表 |
| `fileSnapshot.getBaselineContent` | HTTP | `{ workspace: string, filePath: string }` | `string \| null` | 获取文件的基线内容（用于 diff 对比） |
| `fileSnapshot.stageFile` | HTTP | `{ workspace: string, filePath: string }` | `void` | 暂存单个文件（仅 git-repo 模式） |
| `fileSnapshot.stageAll` | HTTP | `{ workspace: string }` | `void` | 暂存所有变更文件 |
| `fileSnapshot.unstageFile` | HTTP | `{ workspace: string, filePath: string }` | `void` | 取消暂存单个文件 |
| `fileSnapshot.unstageAll` | HTTP | `{ workspace: string }` | `void` | 取消暂存所有文件 |
| `fileSnapshot.discardFile` | HTTP | `{ workspace: string, filePath: string, operation: FileChangeOperation }` | `void` | 丢弃文件变更（恢复到基线） |
| `fileSnapshot.resetFile` | HTTP | `{ workspace: string, filePath: string, operation: FileChangeOperation }` | `void` | 重置文件到基线状态 |
| `fileSnapshot.getBranches` | HTTP | `{ workspace: string }` | `string[]` | 获取 git 分支列表（仅 git-repo 模式） |
| `fileSnapshot.dispose` | HTTP | `{ workspace: string }` | `void` | 清理快照资源（删除临时仓库） |

### 运行模式

| 模式 | 检测条件 | 基线 | 说明 |
|------|---------|------|------|
| `git-repo` | 目录含 `.git` | HEAD ref | 使用已有 git 仓库，支持 stage/unstage/branch 等全部操作 |
| `snapshot` | 目录无 `.git` | 自动创建的初始 commit | 在 `/tmp/aionui-snapshot-*` 创建临时 git 仓库 |

### 数据模型

```
SnapshotInfo {
  mode: 'git-repo' | 'snapshot'
  branch: string | null         // 当前分支名（snapshot 模式为 null）
}

CompareResult {
  staged: FileChangeInfo[]      // 已暂存的变更
  unstaged: FileChangeInfo[]    // 未暂存的变更
}

FileChangeInfo {
  filePath: string              // 绝对路径
  relativePath: string          // 相对于工作区根的路径
  operation: FileChangeOperation
}

FileChangeOperation = 'create' | 'modify' | 'delete'
```

### 实现细节

- 使用 `git status --porcelain` 解析变更状态（X 列=暂存区，Y 列=工作树）
- Git 操作通过 `child_process.execFile` 调用 git CLI
- Snapshot 模式自动生成 `.gitignore`（忽略 node_modules、dist、build、target、.venv 等）
- 启动时自动清理残留的 `aionui-snapshot-*` 临时目录

> **设计决策**：原实现直接调用 git CLI。Rust 重写可选择 `git2` crate（libgit2 绑定）获得更好的性能和错误处理，但 git CLI 调用更简单且行为与用户 git 一致。建议核心操作（status、diff）用 `git2`，复杂操作（stage、restore）可回退到 CLI。

---

## 数据模型汇总

### 文件系统核心类型

```
IDirOrFile {
  name: string              // 文件/目录名
  fullPath: string          // 绝对路径
  relativePath: string      // 相对于 root 的路径
  isDir: boolean
  isFile: boolean
  children?: IDirOrFile[]   // 子节点（仅目录）
}

IWorkspaceFlatFile {
  name: string              // 文件名
  fullPath: string          // 绝对路径
  relativePath: string      // 相对于 root 的路径
}

IFileMetadata {
  name: string              // 文件名
  path: string              // 绝对路径
  size: number              // 字节数
  type: string              // MIME 类型推断
  lastModified: number      // 修改时间戳 (ms)
  isDirectory?: boolean
}
```

### 通用响应

```
IBridgeResponse<D = {}> {
  success: boolean
  data?: D
  msg?: string              // 错误时的描述信息
}
```

---

## 模块依赖

- **依赖**：
  - `03-auth`：文件操作需要认证（通过 WebSocket/HTTP 中间件，非模块内部调用）
  - `04-system-settings`：获取系统目录路径（`getSystemDir`、`getAssistantsDir`、`getSkillsDir`）

- **被依赖**：
  - `05-conversation`：AI 对话中读取/写入工作区文件
  - `06-ai-agent`：Agent 执行时操作文件系统
  - `07-realtime`：`fileStream.contentUpdate`、`fileWatch.fileChanged`、`workspaceOfficeWatch.fileAdded` 事件通过 WebSocket 广播
  - `16-office-preview`：使用文档转换能力

---

## 候选公共类型

| 类型 | 说明 |
|------|------|
| `IDirOrFile` | 文件树节点，文件浏览相关接口共用 |
| `IWorkspaceFlatFile` | 扁平文件列表项 |
| `IFileMetadata` | 文件元数据 |
| `IBridgeResponse<D>` | 通用操作响应（已在多个模块出现） |
| `FileChangeOperation` | 变更操作枚举（`create` / `modify` / `delete`） |
| `ConversionResult<T>` | 转换结果包装 |

---

## 错误处理策略

| 场景 | 原实现行为 | 说明 |
|------|-----------|------|
| 文件不存在 | 返回 `null` 而非抛错 | `readFile`、`readFileBuffer`、`getBaselineContent` |
| 远程图片获取失败 | 返回占位 SVG | 优雅降级，前端无需特殊处理 |
| 超大文件 | 返回空/截断 | 256MB 以上文件不读取 |
| ZIP 取消 | 静默终止并清理 | 通过 `requestId` 匹配 |
| Skill 路径非法 | 返回 `{ success: false, msg }` | 安全校验阻止目录穿越 |
| Git 操作失败 | 抛出错误 | Snapshot 相关操作依赖 git CLI 可用 |

---

## Rust 迁移备注

### 技术选型

| 组件 | 建议 | 说明 |
|------|------|------|
| 文件 I/O | `tokio::fs` | 异步文件操作 |
| 目录遍历 | `walkdir` + `ignore` | 递归遍历并尊重 `.gitignore` |
| 文件监听 | `notify` crate | 跨平台统一的文件系统事件，支持递归监听和 debounce |
| ZIP 打包 | `zip` crate | 流式写入，支持取消（通过 `CancellationToken`） |
| Git 操作 | `git2` / git CLI | 快照系统的变更检测和暂存操作 |
| 文档转换 | 外部进程或 WASM | Word/Excel/PPT 转换缺乏成熟 Rust 原生库，可调用 Python/Node 工具或使用 WASM 版 |
| 远程图片下载 | `reqwest` | HTTP 客户端，支持重定向策略和大小限制 |
| MIME 类型推断 | `mime_guess` | 根据文件扩展名推断 MIME 类型 |

### 架构建议

1. **文件 I/O 与 Skill 管理分离**：当前 `fsBridge.ts` 的 1673 行中约一半是 Skill/Rule 管理。Rust 中应拆分为 `aionui-file`（纯文件操作）和 Skill 管理模块，两者通过 trait 共享文件读写能力

2. **文件操作安全**：
   - 所有路径参数需做规范化（canonicalize）和沙箱校验，防止路径穿越
   - 写入操作应限制在工作区/用户目录范围内
   - 远程图片下载严格白名单 + 大小限制

3. **文档转换策略**：
   - Word → Markdown：考虑 `docx-rs` 或调用外部 `pandoc`
   - Excel → JSON：考虑 `calamine` crate（读取 xlsx/xls/ods）
   - PPT → JSON：Rust 生态无成熟方案，建议 sidecar 进程（Python `python-pptx` 或 Node `pptx2json`）
   - PDF 生成：`printpdf` 或调用 `weasyprint`/`wkhtmltopdf`

4. **工作区快照优化**：
   - Snapshot 模式的临时 git 仓库创建开销较大，可考虑内存级 diff（如 `similar` crate）作为轻量替代
   - Git-repo 模式可直接使用 `git2` 避免进程调用开销

5. **文件内容缓存**：原实现对 `listWorkspaceFiles` 有内存缓存。Rust 中结合 `notify` 的文件监听事件做缓存失效，比轮询更高效
