# 适配层嵌入说明（M6）

本文说明如何在 **IDE 插件** 或 **HTTP 宿主** 中复用 Cubecode 六层链路，与终端 `llm-kit` 使用同一套 `ControlEvent` 协议。

设计背景见 [事件驱动与会话编排.md](./事件驱动与会话编排.md)。

---

## 架构要点

| 层 | 职责 | 本里程碑相关 crate |
|----|------|-------------------|
| ① | 原始 I/O → `ControlEvent` | `cubecode-adapter` |
| ② | 有界收件箱 | `cubecode-inbox` |
| ③～⑥ | 调度 / 编排 / 执行 / 输出 | `dispatch` / `orchestrator` / `step` / `sink` |

**扩展新输入源**：实现 `cubecode_adapter::Adapter` 的 `poll_events()`，或用 `HttpJsonAdapter::handle_request_body` 解析 JSON-RPC，再 `drain_adapter` / `push_events` 写入 ②。

---

## 内置适配器

| 适配器 | 场景 | 入口 |
|--------|------|------|
| `TerminalAdapter` | 交互式终端 | `poll()` → `TerminalPoll` |
| `HttpJsonAdapter` | HTTP / IDE JSON-RPC | `handle_request_body` + `poll_events()` |
| `MockAdapter` | 单元测试 | 预置事件队列 |

---

## HTTP：`llm-kit serve`

阻塞式占位服务（单线程顺序处理连接，无 `tokio`）：

```bash
# 默认 127.0.0.1:8787，⑤ 执行层占位
cargo run -p llm-cli --bin llm-kit -- serve

# 指定地址 / 使用真实 LLM
cargo run -p llm-cli --bin llm-kit -- serve --bind 0.0.0.0:8787 --llm
```

环境变量：`CUBECODE_SERVE_ADDR=127.0.0.1:8787`

### 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/health` | `{"ok":true}` |
| `GET` | `/` | 服务说明 JSON |
| `POST` | `/rpc` | JSON-RPC 2.0 正文 |

### JSON-RPC 方法

| method | params | 产出事件 |
|--------|--------|----------|
| `cubecode.user_turn` | `{ "text": "..." }` | `UserTurn` |
| `cubecode.echo_turn` | `{ "text": "..." }` | `UserTurn`（带 `[echo]` 前缀） |
| `cubecode.shutdown` | （可选 `{}`） | `Shutdown` |

响应 `result` 中含 `events` 数组（与 ② 入队结构一致的 JSON），便于联调。

### 示例

```bash
curl -s -X POST http://127.0.0.1:8787/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"cubecode.echo_turn","params":{"text":"hello"}}'
```

日志：`llm-kit logs -f`（另开终端）。

---

## IDE 嵌入（进程内）

推荐宿主（VS Code / Cursor 插件、桌面壳）**直接依赖 Rust crate**，不经过 HTTP：

1. 创建 `SessionId`、`Orchestrator`、`Inbox`（与 `llm-cli` 相同）。
2. 用户动作 → 构造 `ControlEvent::user_turn` → `cubecode_adapter::push_events`。
3. 调用 `cubecode_orchestrator::run_full_turn`（或自行 ② 出队 + `run_pipeline`，与现有 `flow` 一致）。
4. 助手输出走 `cubecode_sink::Sink` 的实现（可替换为向 Webview 推送的 `Sink`）。

若宿主非 Rust：通过子进程调用 `llm-kit serve`，用 HTTP `POST /rpc` 提交轮次；后续可改为 stdin JSON 行协议（未实现）。

---

## 与终端路径的差异

| 项目 | 终端 `llm-kit` | `serve` / IDE |
|------|----------------|---------------|
| ① | `TerminalAdapter::poll` | `HttpJsonAdapter` |
| 元命令 | `/exit`、`/cancel` 在 ① 消化 | 仅 JSON-RPC；无 `/cancel` |
| ⑤ 默认 | 真实 LLM | 占位（`--llm` 开启真实调用） |

---

## 后续（未实现）

- 长连接 / 多会话并发：最外层可考虑 `tokio` + async ② 消费（见 TODO「同步 vs async」决策）。
- WebSocket、鉴权、流式 HTTP 响应。
