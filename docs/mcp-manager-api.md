# MCP Server Manager API 文档

## 概述

`McpServerManager` 是 TelyClaw 中管理 MCP (Model Context Protocol) 服务器的核心模块，基于 Rust 实现。它负责：

- 启动/停止 MCP 服务器进程
- 发现已连接服务器的可用工具
- 在 MCP 服务器上执行工具调用

## 数据结构

### McpServerConfig

MCP 服务器配置，通常从数据库加载。

```rust
pub struct McpServerConfig {
    pub id: String,           // 服务器唯一标识
    pub name: String,         // 服务器显示名称
    pub enabled: bool,        // 是否启用
    pub transport_type: String, // 传输类型: "stdio" | "sse" | "http"
    pub config_json: Value,   // 传输配置 JSON
}
```

**config_json 示例 (stdio)**:
```json
{
  "command": "npx",
  "args": ["-y", "@anthropic/mcp-server-filesystem"],
  "env": {
    "HOME": "/Users/xxx"
  }
}
```

### McpToolInfo

从 MCP 服务器发现的工具信息。

```rust
pub struct McpToolInfo {
    pub name: String,              // 工具名称
    pub description: Option<String>, // 工具描述
    pub input_schema: Value,       // 输入参数 JSON Schema
    pub server_id: String,         // 所属服务器 ID
    pub server_name: String,       // 所属服务器名称
}
```

## API 方法

### 服务器生命周期

#### `start_servers(configs: Vec<McpServerConfig>) -> Result<(), String>`

批量启动所有已启用的 MCP 服务器。

```rust
let configs = vec![
    McpServerConfig {
        id: "telegram".to_string(),
        name: "Telegram MCP".to_string(),
        enabled: true,
        transport_type: "stdio".to_string(),
        config_json: json!({
            "command": "npx",
            "args": ["-y", "telegram-mcp"]
        }),
    }
];

manager.start_servers(configs).await?;
```

#### `stop_server(server_id: &str) -> Result<(), String>`

停止指定的 MCP 服务器。

```rust
manager.stop_server("telegram").await?;
```

#### `stop_all()`

停止所有运行中的 MCP 服务器。

```rust
manager.stop_all().await;
```

#### `is_server_running(server_id: &str) -> bool`

检查指定服务器是否正在运行。

```rust
if manager.is_server_running("telegram").await {
    println!("Telegram MCP is running");
}
```

#### `get_running_servers() -> Vec<String>`

获取所有运行中的服务器 ID 列表。

```rust
let servers = manager.get_running_servers().await;
// ["telegram", "filesystem", ...]
```

### 工具发现

#### `get_all_tools() -> Vec<McpToolInfo>`

获取所有已连接服务器的工具列表。

```rust
let tools = manager.get_all_tools().await;
for tool in tools {
    println!("{}: {}", tool.name, tool.description.unwrap_or_default());
}
```

#### `get_tool_manifest() -> Vec<Value>`

获取工具清单，格式化为 AI 模型可消费的 JSON 格式。

```rust
let manifest = manager.get_tool_manifest().await;
// 返回格式:
// [
//   {
//     "name": "telegram-send-message",
//     "description": "Send a message to a Telegram chat",
//     "inputSchema": { ... },
//     "serverId": "telegram",
//     "serverName": "Telegram MCP"
//   },
//   ...
// ]
```

### 工具执行

#### `call_tool(server_id: &str, tool_name: &str, arguments: Value) -> Result<CallToolResult, String>`

在指定服务器上执行工具。

```rust
let result = manager.call_tool(
    "telegram",
    "telegram-send-message",
    json!({
        "chatId": "123456",
        "text": "Hello!"
    })
).await?;
```

#### `call_tool_by_name(tool_name: &str, arguments: Value) -> Result<CallToolResult, String>`

按工具名称执行，自动查找对应的服务器。

```rust
let result = manager.call_tool_by_name(
    "telegram-send-message",
    json!({
        "chatId": "123456",
        "text": "Hello!"
    })
).await?;
```

## 全局实例

### `get_mcp_manager() -> Arc<McpServerManager>`

获取全局 MCP Manager 单例。

```rust
use crate::openclaw::mcp_manager::get_mcp_manager;

let manager = get_mcp_manager();
let tools = manager.get_all_tools().await;
```

## 辅助函数

### `tool_result_to_json(result: &CallToolResult) -> Value`

将工具执行结果转换为 JSON 格式。

```rust
let result = manager.call_tool_by_name("some-tool", json!({})).await?;
let json = tool_result_to_json(&result);
// {
//   "isError": false,
//   "content": [
//     { "type": "text", "text": "..." }
//   ]
// }
```

**支持的内容类型**:
- `text` - 文本内容
- `image` - 图片 (base64 + mimeType)
- `audio` - 音频 (base64 + mimeType)
- `resource` - 资源引用
- `resource_link` - 资源链接 (uri + name)

## 传输类型支持

| 类型 | 状态 | 说明 |
|------|------|------|
| stdio | ✅ 已实现 | 通过子进程 stdin/stdout 通信 |
| sse | ⏳ 待实现 | Server-Sent Events |
| http | ⏳ 待实现 | HTTP/REST API |

## 使用示例

### 完整流程

```rust
use crate::openclaw::mcp_manager::{get_mcp_manager, McpServerConfig, tool_result_to_json};
use serde_json::json;

async fn example() -> Result<(), String> {
    let manager = get_mcp_manager();
    
    // 1. 启动服务器
    let configs = vec![
        McpServerConfig {
            id: "telegram".to_string(),
            name: "Telegram MCP".to_string(),
            enabled: true,
            transport_type: "stdio".to_string(),
            config_json: json!({
                "command": "npx",
                "args": ["-y", "telegram-mcp"]
            }),
        }
    ];
    manager.start_servers(configs).await?;
    
    // 2. 获取工具列表
    let tools = manager.get_tool_manifest().await;
    println!("Available tools: {:?}", tools);
    
    // 3. 执行工具
    let result = manager.call_tool_by_name(
        "telegram-send-message",
        json!({
            "chatId": "123456",
            "text": "Hello from MCP!"
        })
    ).await?;
    
    // 4. 处理结果
    let json_result = tool_result_to_json(&result);
    println!("Result: {}", json_result);
    
    // 5. 停止服务器
    manager.stop_all().await;
    
    Ok(())
}
```

## 依赖

- `rmcp` - Rust MCP 客户端库
- `tokio` - 异步运行时
- `serde_json` - JSON 序列化
