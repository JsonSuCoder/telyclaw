use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use super::mcp_manager::{get_mcp_manager, tool_result_to_json};

#[derive(Clone)]
pub struct McpBridgeState {
    pub secret: String,
    pub port: u16,
    pub app_handle: AppHandle,
    pub pending_ask_user: Arc<Mutex<HashMap<String, oneshot::Sender<AskUserResponse>>>>,
    pub pending_telegram_query: Arc<Mutex<HashMap<String, oneshot::Sender<TelegramQueryResponse>>>>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AskUserRequest {
    pub questions: Value,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AskUserResponse {
    pub behavior: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answers: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TelegramQueryRequest {
    pub selector: String,
    #[serde(default)]
    pub params: HashMap<String, Value>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TelegramQueryResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TelegramQueryEventPayload {
    pub request_id: String,
    pub selector: String,
    pub params: HashMap<String, Value>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AskUserEventPayload {
    pub request_id: String,
    pub tool_name: String,
    pub tool_input: AskUserRequest,
}

pub async fn start_mcp_bridge_server(app_handle: AppHandle, secret: String) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    
    let port = listener.local_addr().unwrap().port();

    let state = McpBridgeState {
        secret,
        port,
        app_handle,
        pending_ask_user: Arc::new(Mutex::new(HashMap::new())),
        pending_telegram_query: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/askuser", post(handle_ask_user))
        .route("/telegram/query", post(handle_telegram_query))
        .route("/mcp/execute", post(handle_mcp_execute))
        .route("/mcp/tools", get(handle_mcp_list_tools))
        .with_state(state.clone());

    // Spawn server in background
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Store state in Tauri app for command access
    state.app_handle.manage(state.clone());

    Ok(port)
}

async fn check_auth(headers: &HeaderMap, secret: &str) -> bool {
    if let Some(auth_header) = headers.get("x-mcp-bridge-secret").or_else(|| headers.get("x-ask-user-secret")) {
        if let Ok(auth_str) = auth_header.to_str() {
            return auth_str == secret;
        }
    }
    false
}

async fn handle_ask_user(
    State(state): State<McpBridgeState>,
    headers: HeaderMap,
    Json(payload): Json<AskUserRequest>,
) -> Result<Json<AskUserResponse>, (StatusCode, String)> {
    if !check_auth(&headers, &state.secret).await {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    let request_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();

    state.pending_ask_user.lock().await.insert(request_id.clone(), tx);

    let event_payload = AskUserEventPayload {
        request_id: request_id.clone(),
        tool_name: "AskUserQuestion".to_string(),
        tool_input: payload,
    };

    if let Err(e) = state.app_handle.emit("cowork:stream:permission", json!({ "sessionId": "__askuser__", "request": event_payload })) {
        state.pending_ask_user.lock().await.remove(&request_id);
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to emit event: {}", e)));
    }

    // Wait for response or timeout (120s)
    match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
        Ok(Ok(response)) => Ok(Json(response)),
        _ => {
            state.pending_ask_user.lock().await.remove(&request_id);
            let _ = state.app_handle.emit("cowork:stream:permissionDismiss", json!({ "requestId": request_id }));
            Ok(Json(AskUserResponse {
                behavior: "deny".to_string(),
                answers: None,
            }))
        }
    }
}

async fn handle_telegram_query(
    State(state): State<McpBridgeState>,
    headers: HeaderMap,
    Json(payload): Json<TelegramQueryRequest>,
) -> Result<Json<TelegramQueryResponse>, (StatusCode, String)> {
    if !check_auth(&headers, &state.secret).await {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    let request_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();

    state.pending_telegram_query.lock().await.insert(request_id.clone(), tx);

    let event_payload = TelegramQueryEventPayload {
        request_id: request_id.clone(),
        selector: payload.selector,
        params: payload.params,
    };

    if let Err(e) = state.app_handle.emit("telegram:query", event_payload) {
        state.pending_telegram_query.lock().await.remove(&request_id);
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to emit event: {}", e)));
    }

    // Wait for response or timeout (30s)
    match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
        Ok(Ok(response)) => Ok(Json(response)),
        _ => {
            state.pending_telegram_query.lock().await.remove(&request_id);
            Ok(Json(TelegramQueryResponse {
                success: false,
                data: None,
                error: Some("Query timeout".to_string()),
            }))
        }
    }
}

/// Request payload for /mcp/execute
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct McpExecuteRequest {
    pub server_id: Option<String>,
    pub tool_name: String,
    pub arguments: Value,
}

async fn handle_mcp_execute(
    State(state): State<McpBridgeState>,
    headers: HeaderMap,
    Json(payload): Json<McpExecuteRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !check_auth(&headers, &state.secret).await {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    let manager = get_mcp_manager();

    let result = if let Some(server_id) = payload.server_id {
        manager
            .call_tool(&server_id, &payload.tool_name, payload.arguments)
            .await
    } else {
        manager
            .call_tool_by_name(&payload.tool_name, payload.arguments)
            .await
    };

    match result {
        Ok(tool_result) => Ok(Json(tool_result_to_json(&tool_result))),
        Err(e) => Ok(Json(json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!("Error: {}", e) }]
        }))),
    }
}

async fn handle_mcp_list_tools(
    State(state): State<McpBridgeState>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !check_auth(&headers, &state.secret).await {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    let manager = get_mcp_manager();
    let tools = manager.get_tool_manifest().await;

    Ok(Json(json!({
        "success": true,
        "tools": tools
    })))
}

#[tauri::command]
pub async fn resolve_ask_user(
    state: tauri::State<'_, McpBridgeState>,
    request_id: String,
    response: AskUserResponse,
) -> Result<(), String> {
    if let Some(tx) = state.pending_ask_user.lock().await.remove(&request_id) {
        let _ = tx.send(response);
    }
    Ok(())
}

#[tauri::command]
pub async fn resolve_telegram_query(
    state: tauri::State<'_, McpBridgeState>,
    request_id: String,
    response: TelegramQueryResponse,
) -> Result<(), String> {
    if let Some(tx) = state.pending_telegram_query.lock().await.remove(&request_id) {
        let _ = tx.send(response);
    }
    Ok(())
}
