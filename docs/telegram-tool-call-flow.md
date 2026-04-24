# Telegram Tool 调用全流程

## 架构概览

```
用户输入 → Tauri Backend (Rust) → Claude API (with tools) → tool_use → Frontend 执行 → tool_result → Claude API → 最终回复
```

整个系统由三层组成：

| 层 | 技术 | 职责 |
|---|---|---|
| Frontend | TypeScript (telegramTools.ts) | 持有 Telegram 全局状态，执行实际查询/操作 |
| Backend | Rust (tauri/src/openclaw/mod.rs) | 会话管理、模型调用、工具循环编排 |
| LLM | Claude API (Anthropic) | 理解用户意图，决定调用哪个工具，分析结果并生成回复 |

---

## 完整流程（以"给张三发消息"为例）

### 第 1 步：用户发送消息

用户在 OpenClaw 聊天界面输入：

> 帮我给张三发一条消息：明天下午3点开会

前端调用 Tauri command `cowork_start_session`，传入 prompt、title 等参数。

### 第 2 步：创建会话 & 存储用户消息

`cowork_start_session` (mod.rs:515)：

1. 生成 session UUID，写入 `cowork_sessions` 表（status = `running`）
2. 生成 user message UUID，写入 `cowork_messages` 表（sequence = 1）
3. 通过 `app.emit("cowork_stream_message", ...)` 通知前端 UI 显示用户消息
4. 返回 session 对象给前端
5. `tauri::async_runtime::spawn` 启动异步任务处理模型调用

### 第 3 步：获取工具定义

`call_model_with_tools` (mod.rs:258) 被调用，首先获取可用工具列表：

```rust
let result = telegram_query(
    app.clone(),
    "getToolDefinitions".to_string(),
    json!({}),
).await;
```

这会触发 **事件桥接** 流程（详见第 5 步），前端 `telegramTools.ts` 中的 `getToolDefinitions` handler 返回所有工具定义：

```json
[
  { "name": "telegram_search_user", "description": "...", "input_schema": {...} },
  { "name": "telegram_get_user", "description": "...", "input_schema": {...} },
  { "name": "telegram_get_messages", "description": "...", "input_schema": {...} },
  { "name": "telegram_send_message", "description": "...", "input_schema": {...} },
  ...
]
```

### 第 4 步：首次调用 Claude API

Backend 构建请求发送到 Claude API：

```json
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "system": "<system_prompt>",
  "messages": [
    { "role": "user", "content": "帮我给张三发一条消息：明天下午3点开会" }
  ],
  "tools": [ /* 第 3 步获取的工具定义 */ ]
}
```

Claude 分析用户意图，判断需要先搜索用户"张三"，返回：

```json
{
  "content": [
    { "type": "text", "text": "我来帮你找到张三并发送消息。" },
    {
      "type": "tool_use",
      "id": "toolu_01abc...",
      "name": "telegram_search_user",
      "input": { "query": "张三" }
    }
  ],
  "stop_reason": "tool_use"
}
```

### 第 5 步：执行工具调用（事件桥接）

Backend 解析到 `tool_use`，执行工具调用。这里是整个架构的核心——**Tauri 事件桥接**：

```
┌─────────────────────────────────────────────────────────────────┐
│  Rust Backend                                                   │
│                                                                 │
│  1. 生成 queryId (AtomicU64 自增)                                │
│  2. 创建 mpsc::channel (tx, rx)                                  │
│  3. 将 tx 存入 TELEGRAM_QUERY_CHANNELS HashMap                   │
│  4. app.emit("telegram-query", {                                │
│       queryId, queryType: "executeTool",                        │
│       params: { toolName, toolInput }                           │
│     })                                                          │
│  5. rx.recv_timeout(10s) 阻塞等待                                │
│                                                                 │
│         ┌──── emit ────┐                                        │
│         │              ▼                                        │
│  ┌──────┴──────────────────────────────────────────────┐        │
│  │  Frontend (telegramTools.ts)                        │        │
│  │                                                     │        │
│  │  listen("telegram-query") 收到事件                    │        │
│  │  ↓                                                  │        │
│  │  queryHandlers["executeTool"](params)               │        │
│  │  ↓                                                  │        │
│  │  toolMapping["telegram_search_user"]                │        │
│  │    → handler: "searchUsers"                         │        │
│  │    → mapParams: { query: "张三" }                    │        │
│  │  ↓                                                  │        │
│  │  queryHandlers["searchUsers"]({ query: "张三" })     │        │
│  │  ↓                                                  │        │
│  │  遍历 global.users.byId，模糊匹配 firstName/        │        │
│  │  lastName/username                                  │        │
│  │  ↓                                                  │        │
│  │  返回 sanitizeUser(user) 数组                        │        │
│  │  ↓                                                  │        │
│  │  invoke("telegram_query_response", {                │        │
│  │    queryId, result: { success: true, data: [...] }  │        │
│  │  })                                                 │        │
│  └──────┬──────────────────────────────────────────────┘        │
│         │              ▲                                        │
│         └── invoke ────┘                                        │
│                                                                 │
│  6. telegram_query_response 从 HashMap 取出 tx                   │
│  7. tx.send(result) → rx 解除阻塞                                │
│  8. 返回 Result<serde_json::Value>                               │
└─────────────────────────────────────────────────────────────────┘
```

同时，Backend 将 tool_use 和 tool_result 消息写入数据库并通知前端 UI：

```rust
// 写入 tool_use 消息 (type = "tool_use")
sqlx::query("INSERT INTO cowork_messages ...").bind("tool_use")...

// emit 给前端 UI 展示
app.emit("cowork_stream_message", { type: "tool_use", metadata: { toolName, toolInput, toolUseId } });

// 写入 tool_result 消息 (type = "tool_result")
sqlx::query("INSERT INTO cowork_messages ...").bind("tool_result")...

// emit 给前端 UI 展示
app.emit("cowork_stream_message", { type: "tool_result", metadata: { toolUseId, toolName, isError } });
```

### 第 6 步：将工具结果回传 Claude API

Backend 将工具结果追加到 messages 数组，再次调用 Claude API：

```json
{
  "messages": [
    { "role": "user", "content": "帮我给张三发一条消息：明天下午3点开会" },
    {
      "role": "assistant",
      "content": [
        { "type": "text", "text": "我来帮你找到张三并发送消息。" },
        { "type": "tool_use", "id": "toolu_01abc...", "name": "telegram_search_user", "input": { "query": "张三" } }
      ]
    },
    {
      "role": "user",
      "content": [{
        "type": "tool_result",
        "tool_use_id": "toolu_01abc...",
        "content": "{\"success\":true,\"data\":[{\"id\":\"12345\",\"firstName\":\"三\",\"lastName\":\"张\",\"username\":\"zhangsan\"}]}"
      }]
    }
  ],
  "tools": [...]
}
```

Claude 分析搜索结果，找到张三的 chatId，决定发送消息：

```json
{
  "content": [
    { "type": "text", "text": "找到了张三，现在发送消息。" },
    {
      "type": "tool_use",
      "id": "toolu_02def...",
      "name": "telegram_send_message",
      "input": { "chat_id": "12345", "text": "明天下午3点开会" }
    }
  ],
  "stop_reason": "tool_use"
}
```

### 第 7 步：执行发送消息

再次走事件桥接流程，前端 `executeTool` → `sendMessage` handler：

```typescript
// telegramTools.ts 中的 sendMessage handler
'sendMessage': (params) => {
    const chatId = String(params.chatId || params.chat_id || '');
    const text = String(params.text || '');

    // 参数校验
    if (!chatId) return { success: false, error: 'chatId is required' };
    if (!text) return { success: false, error: 'text is required' };

    // 验证 chat 存在
    const global = getGlobal();
    const chat = selectChat(global, chatId);
    if (!chat) return { success: false, error: `Chat not found: ${chatId}` };

    // 调用 Telegram 发送消息 action
    getActions().sendMessage({
        messageList: { chatId, threadId: MAIN_THREAD_ID, type: 'thread' },
        text,
    });

    return { success: true, data: { chatId, text } };
}
```

返回结果：`{ success: true, data: { chatId: "12345", text: "明天下午3点开会" } }`

### 第 8 步：最终模型回复

Backend 将发送结果再次回传 Claude API，此时 Claude 不再调用工具，返回最终文本：

```json
{
  "content": [
    { "type": "text", "text": "已经成功给张三发送了消息「明天下午3点开会」。" }
  ],
  "stop_reason": "end_turn"
}
```

### 第 9 步：保存 & 展示最终回复

Backend 检测到 `tool_uses.is_empty()`，退出工具循环：

1. 删除之前的 assistant 占位消息
2. 创建新的 assistant 消息（sequence 在所有 tool 消息之后）
3. `app.emit("cowork_stream_message", ...)` 通知前端显示最终回复
4. 更新 session status 为 `completed`
5. `app.emit("cowork_stream_complete", ...)` 通知前端会话结束

---

## 工具循环控制

- 最大循环次数：**6 轮**（`if steps > 6 { break; }`）
- 每轮可包含多个 tool_use（Claude 可在一次响应中调用多个工具）
- 超时控制：事件桥接等待 **10 秒**超时
- 错误处理：工具执行失败时 `is_error: true` 回传给 Claude，由模型决定是否重试

## 数据流向总结

```
用户输入
  │
  ▼
cowork_start_session (Rust)
  │
  ├─ 写入 session + user message → DB
  ├─ emit → 前端 UI 显示
  │
  ▼
call_model_with_tools (Rust)
  │
  ├─ telegram_query("getToolDefinitions") → 前端返回工具列表
  │
  ▼
Claude API 请求 (带 tools)
  │
  ├─ 返回 tool_use ──────────────────────────────┐
  │                                               │
  │   ┌───────────────────────────────────────────┘
  │   │
  │   ▼
  │   telegram_query("executeTool") ──→ 前端执行
  │   │                                    │
  │   │  ┌─────────────────────────────────┘
  │   │  │
  │   │  ▼
  │   │  tool_result 写入 DB + emit UI
  │   │
  │   ▼
  │   追加 tool_result 到 messages
  │   │
  │   ▼
  │   Claude API 请求 (带 tool_result)
  │   │
  │   ├─ 返回 tool_use → 继续循环 (最多 6 轮)
  │   └─ 返回 text → 退出循环
  │
  ▼
最终回复写入 DB + emit UI
session status → completed
```

## 关键文件

| 文件 | 作用 |
|---|---|
| `src/util/tauri/telegramTools.ts` | 前端工具实现：查询 handlers、sanitize 函数、事件监听 |
| `tauri/src/openclaw/mod.rs` | 后端核心：会话管理、模型调用、工具循环、事件桥接 |
| `src/util/tauri/setupTauriListeners.ts` | 应用启动时调用 `setupTelegramTools()` 注册事件监听 |
