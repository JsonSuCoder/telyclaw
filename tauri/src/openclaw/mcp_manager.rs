//! MCP Server Manager - manages MCP server processes and tool execution
//!
//! This module provides functionality to:
//! - Start/stop MCP server processes (stdio, sse, http transports)
//! - Discover available tools from connected servers
//! - Execute tools on MCP servers

use rmcp::{
    model::{CallToolRequestParams, CallToolResult},
    service::RunningService,
    transport::TokioChildProcess,
    RoleClient, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;

/// Configuration for an MCP server from database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub transport_type: String, // "stdio" | "sse" | "http"
    pub config_json: Value,
}

/// Tool information discovered from an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub server_id: String,
    pub server_name: String,
}

/// A running MCP server instance
struct ManagedMcpServer {
    config: McpServerConfig,
    client: RunningService<RoleClient, ()>,
    tools: Vec<McpToolInfo>,
}

/// MCP Server Manager - singleton that manages all MCP server connections
pub struct McpServerManager {
    servers: RwLock<HashMap<String, ManagedMcpServer>>,
}

impl McpServerManager {
    pub fn new() -> Self {
        Self {
            servers: RwLock::new(HashMap::new()),
        }
    }

    /// Start all enabled MCP servers from configs
    pub async fn start_servers(&self, configs: Vec<McpServerConfig>) -> Result<(), String> {
        for config in configs {
            if !config.enabled {
                continue;
            }

            match self.start_server(config.clone()).await {
                Ok(_) => {
                    log::info!("Started MCP server: {}", config.name);
                }
                Err(e) => {
                    log::error!("Failed to start MCP server {}: {}", config.name, e);
                }
            }
        }
        Ok(())
    }

    /// Start a single MCP server
    async fn start_server(&self, config: McpServerConfig) -> Result<(), String> {
        let client = match config.transport_type.as_str() {
            "stdio" => self.start_stdio_server(&config).await?,
            "sse" => {
                // SSE transport not yet implemented
                return Err("SSE transport not yet implemented".to_string());
            }
            "http" => {
                // HTTP transport not yet implemented
                return Err("HTTP transport not yet implemented".to_string());
            }
            _ => {
                return Err(format!("Unknown transport type: {}", config.transport_type));
            }
        };

        // Discover tools from the server
        let tools = self.discover_tools(&client, &config).await?;

        let managed = ManagedMcpServer {
            config: config.clone(),
            client,
            tools,
        };

        self.servers.write().await.insert(config.id.clone(), managed);
        Ok(())
    }

    /// Start a stdio-based MCP server
    async fn start_stdio_server(
        &self,
        config: &McpServerConfig,
    ) -> Result<RunningService<RoleClient, ()>, String> {
        let command = config
            .config_json
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'command' in stdio config")?;

        let args: Vec<String> = config
            .config_json
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let env: HashMap<String, String> = config
            .config_json
            .get("env")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let mut cmd = Command::new(command);
        cmd.args(&args);
        for (key, value) in env {
            cmd.env(key, value);
        }

        let transport = TokioChildProcess::new(cmd).map_err(|e| e.to_string())?;

        let client = ().serve(transport).await.map_err(|e| e.to_string())?;

        Ok(client)
    }

    /// Discover tools from a connected MCP server
    async fn discover_tools(
        &self,
        client: &RunningService<RoleClient, ()>,
        config: &McpServerConfig,
    ) -> Result<Vec<McpToolInfo>, String> {
        let tools_result = client.list_all_tools().await.map_err(|e| e.to_string())?;

        let tools: Vec<McpToolInfo> = tools_result
            .into_iter()
            .map(|tool| McpToolInfo {
                name: tool.name.to_string(),
                description: tool.description.map(|s| s.to_string()),
                input_schema: serde_json::to_value(&tool.input_schema).unwrap_or(json!({})),
                server_id: config.id.clone(),
                server_name: config.name.clone(),
            })
            .collect();

        Ok(tools)
    }

    /// Get all available tools from all connected servers
    pub async fn get_all_tools(&self) -> Vec<McpToolInfo> {
        let servers = self.servers.read().await;
        servers
            .values()
            .flat_map(|s| s.tools.clone())
            .collect()
    }

    /// Get tool manifest for AI model consumption
    pub async fn get_tool_manifest(&self) -> Vec<Value> {
        self.get_all_tools()
            .await
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": tool.input_schema,
                    "serverId": tool.server_id,
                    "serverName": tool.server_name
                })
            })
            .collect()
    }

    /// Execute a tool on an MCP server
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<CallToolResult, String> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_id)
            .ok_or_else(|| format!("Server not found: {}", server_id))?;

        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(args) = arguments.as_object() {
            params = params.with_arguments(args.clone());
        }

        let result = server
            .client
            .call_tool(params)
            .await
            .map_err(|e| e.to_string())?;

        Ok(result)
    }

    /// Execute a tool by name (finds the server automatically)
    pub async fn call_tool_by_name(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<CallToolResult, String> {
        // Find which server has this tool
        let servers = self.servers.read().await;
        for (_server_id, server) in servers.iter() {
            if server.tools.iter().any(|t| t.name == tool_name) {
                let mut params = CallToolRequestParams::new(tool_name.to_string());
                if let Some(args) = arguments.as_object() {
                    params = params.with_arguments(args.clone());
                }

                return server
                    .client
                    .call_tool(params)
                    .await
                    .map_err(|e| e.to_string());
            }
        }

        Err(format!("Tool not found: {}", tool_name))
    }

    /// Stop a specific MCP server
    pub async fn stop_server(&self, server_id: &str) -> Result<(), String> {
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.remove(server_id) {
            // The client will be dropped, which should close the connection
            drop(server);
            log::info!("Stopped MCP server: {}", server_id);
        }
        Ok(())
    }

    /// Stop all MCP servers
    pub async fn stop_all(&self) {
        let mut servers = self.servers.write().await;
        servers.clear();
        log::info!("Stopped all MCP servers");
    }

    /// Check if a server is running
    pub async fn is_server_running(&self, server_id: &str) -> bool {
        self.servers.read().await.contains_key(server_id)
    }

    /// Get list of running server IDs
    pub async fn get_running_servers(&self) -> Vec<String> {
        self.servers.read().await.keys().cloned().collect()
    }
}

/// Global MCP manager instance
static MCP_MANAGER: std::sync::OnceLock<Arc<McpServerManager>> = std::sync::OnceLock::new();

/// Get or initialize the global MCP manager
pub fn get_mcp_manager() -> Arc<McpServerManager> {
    MCP_MANAGER
        .get_or_init(|| Arc::new(McpServerManager::new()))
        .clone()
}

/// Convert CallToolResult to JSON for API response
pub fn tool_result_to_json(result: &CallToolResult) -> Value {
    use rmcp::model::RawContent;

    let content: Vec<Value> = result
        .content
        .iter()
        .map(|c| {
            match &c.raw {
                RawContent::Text(text) => json!({
                    "type": "text",
                    "text": text.text
                }),
                RawContent::Image(img) => json!({
                    "type": "image",
                    "data": img.data,
                    "mimeType": img.mime_type
                }),
                RawContent::Audio(audio) => json!({
                    "type": "audio",
                    "data": audio.data,
                    "mimeType": audio.mime_type
                }),
                RawContent::Resource(res) => json!({
                    "type": "resource",
                    "resource": res.resource
                }),
                RawContent::ResourceLink(link) => json!({
                    "type": "resource_link",
                    "uri": link.uri,
                    "name": link.name
                }),
            }
        })
        .collect();

    json!({
        "isError": result.is_error.unwrap_or(false),
        "content": content
    })
}
