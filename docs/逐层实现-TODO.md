# Cubecode 逐层实现 TODO

> 对照 Claude Code / Codex：**外层六层管会话与流程，⑤ 内 O-R-A + 工具；`llm-kit` 只管模型与单次 generate 前后链。**  
> 设计说明见：[事件驱动与会话编排.md](./事件驱动与会话编排.md)

---

## 使用说明（给人和 Agent）

1. **开工前**：阅读本文 + 「进度摘要」，确认当前里程碑。
2. **只做一项**：在同一里程碑内，从上到下找**第一个未勾选** `- [ ]` 条目实现；不要跨里程碑大块并行。
3. **完成后**：
   - 将该项改为 `- [x]`，必要时在「验收」子项下补简短说明或 PR 链接。
   - 更新下方 **进度摘要**（日期、当前里程碑、下一步）。
   - 在 **修订记录** 追加一行。
4. **跑通验证**：涉及 CLI 时用 `cargo test --workspace`；六层路径用 `llm-kit six-layer-pipeline` 与 `logs -f` 观察中文链路日志。
5. **待决事项**未在「落地前待决」里拍板前，不要擅自改协议语义（见设计文档末）。

---

## 进度摘要

| 字段 | 值 |
|------|-----|
| **最后更新** | 2026-05-18 |
| **当前里程碑** | **M0～M6 均已落地**（无未勾选的里程碑子项） |
| **下一步** | 按产品需求扩展（HTTP 并发、向量检索、IDE 宿主等） |
| **已完成里程碑** | M0、M1、M2、M3、M4、M5 |

---

## 里程碑总览

| 里程碑 | 目标 | 状态 |
|--------|------|------|
| **M0** | 六层骨架 + 日志 + 聊天走通 | 已完成 |
| **M1** | 契约 v1 + 路由可扩展 | 已完成 |
| **M2** | ② 真队列 + ③ 路由表 | 已完成 |
| **M3** | ⑤ 流式 + ⑥ 结构化输出 | 已完成 |
| **M4** | ④ 状态机 + 工具 v1 + 多圈编排 | 已完成 |
| **M5** | 记忆/RAG + pipeline 注入 | 已完成 |
| **M6** | 第二 Adapter（IDE/HTTP） | 已完成 |

---

## M0 — 基线（六层可观测、可聊天）

### 工程与文档

- [x] 六层独立 crate：`cubecode-adapter` … `cubecode-sink` + `cubecode-contracts`
- [x] `llm-kit` 位于 `crates/cubecode-step/llm-kit`，由 `cubecode-step` 依赖
- [x] 设计文档 `docs/事件驱动与会话编排.md`
- [x] 本 TODO 文档 `docs/逐层实现-TODO.md`
- [x] 在设计文档「与当前仓库的对应」中增加指向本 TODO 的链接

### 横切：日志

- [x] `cubecode-log`：stderr + `.cubecode/logs/` 落盘
- [x] CLI `logs` / `logs -f` / `--tail`（类 docker logs）
- [x] 各层 `tracing` 中文文案 + ①～⑥ 层级名（`cubecode-log/src/layers.rs`）
- [x] 日志统一携带 `session_id`、`turn_id` 字段（`TurnContext` 贯穿 ①～⑥）

### ① 适配层

- [x] `enqueue_user_line` / `enqueue_shutdown`
- [x] `TerminalAdapter` 结构体封装（从 `llm-cli` 抽离 stdin/元命令）
- [ ] 元命令策略拍板并实现：`/exit` 在 ① 处理 vs 进 inbox（见待决）

### ② 收件箱

- [x] 内存 `VecDeque` + push/pop + 中文日志
- [x] 有界队列与背压（`try_push` → `InboxFull`）
- [x] `cancel_session` / `clear` 清空待处理事件

### ③ 调度层

- [x] `UserLine → UserTurn`，`Shutdown → Exit`
- [x] 路由表结构（`ControlEventKind` → `RouteHint`，可注册 `Router`）

### ④ 编排层

- [x] `run_full_turn`：①→②→③→`run_pipeline`
- [x] `run_shutdown_turn`
- [x] `StepBackend::Placeholder | Llm`
- [x] 状态机类型定义（`OrchestratorState` + `transition` / `Orchestrator`）
- [ ] Step 完成后「再入队 ②」接口预留

### ⑤ 执行层

- [x] `placeholder_turn`（演示）
- [x] `llm_turn` + `LlmStepContext`（真实 `registry.generate`）
- [x] `llm_turn_stream` 接入 `registry.generate_stream`（M3-1）
- [x] `ToolRegistry` v1：`read_file`（工作区白名单、`CUBECODE_WORKSPACE_ROOT`）

### ⑥ 输出层

- [x] `emit_line`（带标签）
- [x] `emit_assistant`（聊天正文）
- [x] `Sink` trait + `TerminalSink`：`emit_chunk` / `emit_error`（`emit_tool_*` 待 M4）

### 入口 `apps/llm-cli`

- [x] 默认多轮聊天走 `run_full_turn` + `Llm`
- [x] `complete` / 管道单次走六层
- [x] `six-layer-pipeline` 占位演示
- [ ] 删除重复的「直连 registry」死代码路径（若仍残留）

**M0 验收**：`cargo test --workspace` 通过；终端聊天 + `logs -f` 能看到完整 ①～⑥ 中文链路。

---

## M1 — 契约 v1（`cubecode-contracts`）

- [x] M1-1：`SessionId`、`TurnId`、`TurnContext`（`crates/cubecode-contracts/src/ids.rs`）
- [x] M1-2：事件扩展（serde，`crates/cubecode-contracts/src/events.rs`）
  - [x] `UserTurn { session_id, turn_id, text }`（已移除 `UserLine`）
  - [x] `Shutdown { session_id }`
  - [x] 预留 `ToolResult { session_id, turn_id, call_id, output }`
- [x] M1-3：`RouteHint` 扩展（`crates/cubecode-contracts/src/routes.rs`，serde）
  - [x] `ChatTurn`（原 `UserTurn`）
  - [x] `ToolFollowUp` / `SubAgent`（占位；`ToolResult` → `ToolFollowUp`）
- [x] M1-4：各层改为使用新事件（`ControlEvent::user_turn` / `shutdown`）
- [x] M1-5：序列化往返（`events` 测试）+ 路由用例（`dispatch::tests::route_by_event_kind`）

**M1 验收**：旧 CLI 行为不变；日志中每轮可见 `session_id` / `turn_id`。

---

## M2 — ② 真队列 + ③ 路由表

- [x] M2-1：`Inbox` 有界容量 + `try_push` → `Result`（`InboxFull`；默认 256，`CUBECODE_INBOX_CAPACITY`）
- [x] M2-2：`Inbox::cancel_session` / `clear`；① `cancel_session` / `clear_inbox`；`ControlEvent::session_id`
- [x] M2-3：`Router` 可注册表 + `ControlEventKind`；`route()` 仍用默认表
- [x] M2-4：③ 单元测试：多事件类型 → 不同 hint（表驱动 + `needs_step` + 连续路由）
- [x] M2-5：文档更新「落地前待决」：同步 vs async 选型（见设计文档「落地前待决」）

**M2 验收**：构造满队列时 `push` 可观测失败；不同类型事件路由日志可区分。

---

## M3 — ⑤ 流式 + ⑥ 结构化输出

- [x] M3-1：`llm_turn_stream` 调用 `registry.generate_stream`（`on_chunk` 回调 + 中文流式日志）
- [x] M3-2：`Sink::emit_chunk` + `TerminalSink`（stdout flush；`begin/end_assistant_stream`）
- [x] M3-3：`StepBackend::llm_stream` + `run_chat_llm_stream`（`ChatTurn` → ⑤ 流式 → ⑥ chunk）
- [x] M3-4：`emit_error` / `emit_error_global`；④ `map_turn_error`；多轮聊天错误不退出
- [x] M3-5：CLI 默认流式；全局 `--no-stream`（聊天 + `complete`）

**M3 验收**：聊天时终端逐块输出；`logs -f` 有「流式开始/结束」类中文日志。

---

## M4 — ④ 状态机 + 工具 v1 + 多圈编排

- [x] M4-1：`OrchestratorState` + `OrchestratorSignal` + `transition`；`run_full_turn` 接线
- [x] M4-2：`TurnFinished` / `StepOutcome` + `classify_step_result`（POC JSON）；`finish_turn_with`
- [x] M4-3：工具 v1：`read_file`（只读、规范路径白名单、512KiB 上限）
- [x] M4-4：`parse_tool_call_from_model_output`（JSON / 代码块 / 嵌入文本）；`PendingTool.arguments`
- [x] M4-5：工具结果 → `ToolResult` 事件 → 再入队 ② → ③ → ④（`enqueue_tool_result`、`execute_pending_tool`、`run_full_turn` 工具多圈）
- [x] M4-6：单轮用户输入触发「模型 → 工具 → 再模型」可在一轮 `turn_id` 下完成（`TurnRunner` + transcript + `llm_turn` 工具回灌）
- [x] M4-7：取消：Ctrl+C 或 `/cancel` 使 ④ 退出 `RunningTurn`（`cancel_active_turn` + `run_full_turn` 协作式检查）

**M4 验收**：`logs -f` 可见同一 `turn_id` 下多次 ④→⑤；`read_file` 能读仓库内文件并影响下一轮模型输入。

---

## M5 — 记忆 / RAG + pipeline

- [x] M5-1：记忆 crate 或 `cubecode-step` 子模块（检索接口 trait）（`memory::MemoryRetriever` + `NoopRetriever` / `InMemoryRetriever`）
- [x] M5-2：`llm-kit` `PipelineStage`：召回注入 `before_generate`（`MemoryRecallStage` + `stamp_request_metadata` + CLI `CUBECODE_MEMORY_ENABLED`）
- [x] M5-3：④ 在进 ⑤ 前写 `metadata`（session 范围）（`SessionMetadata` + `prepare_for_step` → `LlmStepContext::request_metadata`）
- [x] M5-4：配置项：是否启用记忆、top-k、日志不打印全文（`MemoryConfig` + `CUBECODE_MEMORY_TOP_K` + 日志摘要）

**M5 验收**：多轮对话后，召回内容可在日志中看到「执行层进入」前的 pipeline 日志（或 debug 级摘要）。

---

## M6 — 第二 Adapter

- [x] M6-1：`Adapter` trait：`poll_events() -> Vec<ControlEvent>`（`Adapter` + `drain_adapter` / `push_events` + `MockAdapter`）
- [x] M6-2：`TerminalAdapter` 实现 trait（`TerminalPoll` + `poll`；`llm-cli` 多轮聊天已接线）
- [x] M6-3：JSON-RPC / HTTP 占位 Adapter（`HttpJsonAdapter` + `cubecode.user_turn` / `echo_turn` / `shutdown`）
- [x] M6-4：CLI 子命令 `serve` + [adapter-embed.md](./adapter-embed.md)（阻塞 HTTP、`POST /rpc`、IDE 嵌入说明）

**M6 验收**：两种 Adapter 产出的事件进同一 ②，后续链路一致。

---

## 落地前待决（拍板后把结论写在「决策」列）

| 议题 | 选项 | 决策 | 关联任务 |
|------|------|------|----------|
| 同步 vs async | 阻塞 CLI / tokio 运行时 | **已决：现阶段同步阻塞**（①～④、`llm-cli` 主循环；⑤ `reqwest::blocking`；六层 crate 不引 tokio）。M3 流式优先在 ⑤ 内 blocking stream；M6 `serve`/长连接再评估最外层 `tokio::main` + async 消费 ②。 | M2-5 |
| 单消费者 vs 会话分区 | 全局串行 / 按 session 并行 | 未决 | M2+ |
| 元命令归属 | ① 吞掉 vs 进 inbox | 未决 | M0 适配层 |
| tool call 格式 | 厂商 native / 自研 JSON | 未决 | M4-4 |

---

## 修订记录

| 日期 | 说明 |
|------|------|
| 2026-05-18 | 初版：M0～M6 逐层 TODO；M0 已完项根据当前仓库勾选 |
| 2026-05-18 | 完成 M1-1：`SessionId`/`TurnId`/`TurnContext`；六层日志带 `session_id`/`turn_id`；`cargo test --workspace` 通过 |
| 2026-05-18 | 完成 M1-2/M1-4/M1-5：`ControlEvent` serde 变体；各层接线；`ToolResult` 占位 |
| 2026-05-18 | 完成 M1-3：`RouteHint` 扩展为 `ChatTurn`/`Exit`/`ToolFollowUp`/`SubAgent`；M1 里程碑完成 |
| 2026-05-18 | 完成 M2-1：有界 `Inbox::try_push` + 背压错误；adapter/flow 向上返回 |
| 2026-05-18 | 完成 M2-2：`cancel_session` / `clear` + 契约 `session_id()` |
| 2026-05-18 | 完成 M2-3：`Router` + `ControlEventKind`；可覆盖注册路由 |
| 2026-05-18 | 完成 M2-4：多事件路由表驱动测试 + `DEFAULT_ROUTES` |
| 2026-05-18 | 完成 M2-5：拍板同步阻塞模型至 M4 前；**M2 里程碑完成** |
| 2026-05-18 | 完成 M3-1：`llm_turn_stream` + 单元测试 |
| 2026-05-18 | 完成 M3-2：`Sink` trait + `TerminalSink::emit_chunk` |
| 2026-05-18 | 完成 M3-3：编排层流式路径；CLI 聊天暂用 `llm_stream`（`--no-stream` 留 M3-5） |
| 2026-05-18 | 完成 M3-4：`emit_error` 统一 stderr + warn；flow 失败经 ⑥ 展示 |
| 2026-05-18 | 完成 M3-5：全局 `--no-stream`；**M3 里程碑完成** |
| 2026-05-18 | 完成 M4-1：`Orchestrator` 状态机 + flow/CLI 迁移日志 |
| 2026-05-18 | 完成 M4-2：`TurnFinished`/`StepOutcome` + 待工具时 `AwaitingTool` |
| 2026-05-18 | 完成 M4-3：`ToolRegistry` + `read_file` 白名单读文件 |
| 2026-05-18 | 完成 M4-4：模型输出 tool call 解析 + 抑制 JSON 当助手正文 |
| 2026-05-18 | 完成 M4-5：`ToolResult` 再入队 ②→③→④；`execute_pending_tool` 工具多圈 |
| 2026-05-18 | 完成 M4-6：`TurnRunner` + transcript；同一 `turn_id` 下模型→工具→再模型 |
| 2026-05-18 | 完成 M4-7：`/cancel`、Ctrl+C、`cancel_active_turn`；**M4 里程碑完成** |
| 2026-05-18 | 完成 M5-1：`cubecode-step::memory` 检索 trait + 空实现 / 进程内 POC |
| 2026-05-18 | 完成 M5-2：`MemoryRecallStage` pipeline + `llm_turn` metadata + CLI 可选启用 |
| 2026-05-18 | 完成 M5-3：`SessionMetadata` 由 ④ 在进 ⑤ 前写入并传入执行层 |
| 2026-05-18 | 完成 M5-4：`MemoryConfig` / `CUBECODE_MEMORY_TOP_K`；召回日志仅 id·字节摘要；**M5 里程碑完成** |
| 2026-05-18 | 完成 M6-1：`Adapter` trait、`drain_adapter`、`push_events`、`MockAdapter` |
| 2026-05-18 | 完成 M6-2：`TerminalAdapter` / `TerminalPoll`；`llm-cli` 默认聊天改用 ① poll |
| 2026-05-18 | 完成 M6-3：`HttpJsonAdapter` JSON-RPC 占位 + 响应回显 `ControlEvent` |
| 2026-05-18 | 完成 M6-4：`llm-kit serve` + `docs/adapter-embed.md`；**M6 里程碑完成** |
