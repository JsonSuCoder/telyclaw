pub mod db;
pub mod mcp_bridge;

use db::OpenClawDb;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use sqlx::Row;
use chrono;
use reqwest::header::HeaderMap;
use reqwest::Method;

pub struct OpenClawState {
    pub db: OpenClawDb,
}

#[tauri::command]
pub async fn openclaw_engine_get_status() -> serde_json::Value {
    // Tauri version uses direct API calls instead of Gateway process,
    // so we always report as "ready" to enable the UI.
    json!({
        "success": true,
        "status": {
            "phase": "ready",
            "version": "tauri-native",
            "message": null,
            "canRetry": false
        }
    })
}

#[tauri::command]
pub async fn cowork_list_sessions(
    state: State<'_, OpenClawState>,
    agentId: Option<String>,
) -> Result<serde_json::Value, String> {
    let _ = agentId;
    let rows = sqlx::query("SELECT * FROM cowork_sessions ORDER BY updated_at DESC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(json!({
            "id": row.get::<String, _>("id"),
            "title": row.get::<String, _>("title"),
            "claudeSessionId": row.get::<Option<String>, _>("claude_session_id"),
            "status": row.get::<String, _>("status"),
            "pinned": row.get::<i32, _>("pinned") != 0,
            "cwd": row.get::<String, _>("cwd"),
            "systemPrompt": row.get::<String, _>("system_prompt"),
            "executionMode": row.get::<Option<String>, _>("execution_mode").unwrap_or_else(|| "local".to_string()),
            "activeSkillIds": json!([]),
            "agentId": "",
            "messages": json!([]),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }));
    }
    Ok(json!({ "success": true, "sessions": sessions }))
}

async fn load_session_messages(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    session_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let rows = sqlx::query("SELECT * FROM cowork_messages WHERE session_id = ? ORDER BY sequence ASC, created_at ASC")
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut messages = Vec::new();
    for row in rows {
        let metadata_raw = row.get::<Option<String>, _>("metadata");
        let metadata = metadata_raw
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
        messages.push(json!({
            "id": row.get::<String, _>("id"),
            "type": row.get::<String, _>("type"),
            "content": row.get::<String, _>("content"),
            "timestamp": row.get::<i64, _>("created_at"),
            "metadata": metadata
        }));
    }
    Ok(messages)
}

#[tauri::command]
pub async fn cowork_get_session(state: State<'_, OpenClawState>, sessionId: String) -> Result<serde_json::Value, String> {
    let row = sqlx::query("SELECT * FROM cowork_sessions WHERE id = ?")
        .bind(&sessionId)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    match row {
        Some(row) => {
            let messages = load_session_messages(&state.db.pool, &sessionId).await?;
            Ok(json!({ "success": true, "session": json!({
            "id": row.get::<String, _>("id"),
            "title": row.get::<String, _>("title"),
            "claudeSessionId": row.get::<Option<String>, _>("claude_session_id"),
            "status": row.get::<String, _>("status"),
            "pinned": row.get::<i32, _>("pinned") != 0,
            "cwd": row.get::<String, _>("cwd"),
            "systemPrompt": row.get::<String, _>("system_prompt"),
            "executionMode": row.get::<Option<String>, _>("execution_mode").unwrap_or_else(|| "local".to_string()),
            "activeSkillIds": json!([]),
            "agentId": "",
            "messages": messages,
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
            }) }))
        }
        None => Ok(json!({ "success": false, "error": "Session not found" })),
    }
}

#[derive(Clone)]
struct SimpleApiConfig {
    api_key: String,
    base_url: String,
    model: String,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn read_app_config(app: &AppHandle) -> Option<serde_json::Value> {
    let path = app.path().app_data_dir().ok()?.join("store.json");
    let content = std::fs::read_to_string(path).ok()?;
    let store: std::collections::HashMap<String, serde_json::Value> = serde_json::from_str(&content).ok()?;
    store.get("app_config").cloned()
}

fn extract_simple_api_config(app: &AppHandle) -> Option<SimpleApiConfig> {
    let app_config = read_app_config(app)?;
    let api_key = app_config.pointer("/api/key")?.as_str()?.to_string();
    let base_url = app_config.pointer("/api/baseUrl")?.as_str()?.to_string();
    let model = app_config.pointer("/model/defaultModel")?.as_str()?.to_string();
    if api_key.trim().is_empty() || base_url.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    Some(SimpleApiConfig { api_key, base_url, model })
}

async fn call_model_once(
    cfg: &SimpleApiConfig,
    system_prompt: Option<String>,
    messages: Vec<(String, String)>,
) -> Result<String, String> {
    let base = cfg.base_url.trim_end_matches('/').to_string();

    let anthropic_url = format!("{}/v1/messages", base);
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("x-api-key", cfg.api_key.parse().unwrap());
    headers.insert("anthropic-version", "2023-06-01".parse().unwrap());

    let anthropic_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    let mut body = json!({
        "model": cfg.model,
        "max_tokens": 1024,
        "messages": anthropic_messages
    });
    if let Some(sp) = system_prompt.clone().filter(|s| !s.trim().is_empty()) {
        if let serde_json::Value::Object(map) = &mut body {
            map.insert("system".to_string(), json!(sp));
        }
    }

    let client = reqwest::Client::new();
    let res = client
        .request(Method::POST, anthropic_url)
        .headers(headers)
        .json(&body)
        .send()
        .await;

    if let Ok(resp) = res {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.is_success() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
                    let mut out = String::new();
                    for item in content {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                                out.push_str(t);
                            }
                        }
                    }
                    if !out.trim().is_empty() {
                        return Ok(out);
                    }
                }
            }
            return Ok(text);
        }
    }

    let openai_url = format!("{}/v1/chat/completions", base);
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("authorization", format!("Bearer {}", cfg.api_key).parse().unwrap());

    let mut openai_messages: Vec<serde_json::Value> = Vec::new();
    if let Some(sp) = system_prompt.filter(|s| !s.trim().is_empty()) {
        openai_messages.push(json!({ "role": "system", "content": sp }));
    }
    for (role, content) in messages {
        openai_messages.push(json!({ "role": role, "content": content }));
    }

    let body = json!({
        "model": cfg.model,
        "messages": openai_messages,
        "temperature": 0.7
    });

    let resp = client
        .request(Method::POST, openai_url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(text);
    }
    let v = serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string())?;
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if content.trim().is_empty() {
        return Err("Empty model response".to_string());
    }
    Ok(content)
}

#[tauri::command]
pub async fn cowork_start_session(
    app: AppHandle,
    state: State<'_, OpenClawState>,
    prompt: String,
    title: String,
    cwd: Option<String>,
    systemPrompt: Option<String>,
    activeSkillIds: Option<Vec<String>>,
    agentId: Option<String>,
    imageAttachments: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let title = if title.trim().is_empty() { "New Session".to_string() } else { title };
    let cwd = cwd.unwrap_or_default();
    let system_prompt = systemPrompt.unwrap_or_default();
    let execution_mode = "local";
    let active_skill_ids = activeSkillIds.unwrap_or_default();
    let agent_id = agentId.unwrap_or_default();
    let image_attachments = imageAttachments;
    let active_skill_ids_val = json!(active_skill_ids.clone());
    let image_attachments_val = image_attachments.clone();
    let now = now_ms();

    sqlx::query(
        "INSERT INTO cowork_sessions (id, title, status, cwd, system_prompt, execution_mode, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&title)
    .bind("running")
    .bind(&cwd)
    .bind(&system_prompt)
    .bind(execution_mode)
    .bind(now)
    .bind(now)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    let user_message_id = uuid::Uuid::new_v4().to_string();
    let mut user_metadata = serde_json::Map::new();
    user_metadata.insert("skillIds".to_string(), active_skill_ids_val.clone());
    if let Some(attachments) = image_attachments_val.clone() {
        user_metadata.insert("imageAttachments".to_string(), attachments);
    }
    let user_metadata_str = serde_json::Value::Object(user_metadata).to_string();
    sqlx::query(
        "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&user_message_id)
    .bind(&id)
    .bind("user")
    .bind(&prompt)
    .bind(user_metadata_str)
    .bind(now)
    .bind(1)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    let _ = app.emit("cowork_stream_message", json!({
        "sessionId": id,
        "message": {
            "id": user_message_id,
            "type": "user",
            "content": prompt,
            "timestamp": now,
            "metadata": {
                "skillIds": active_skill_ids_val,
                "imageAttachments": image_attachments_val
            }
        }
    }));

    let session_for_return = json!({
        "id": id,
        "title": title,
        "claudeSessionId": null,
        "status": "running",
        "pinned": false,
        "cwd": cwd,
        "systemPrompt": system_prompt,
        "executionMode": execution_mode,
        "activeSkillIds": active_skill_ids,
        "agentId": agent_id,
        "messages": [],
        "createdAt": now,
        "updatedAt": now
    });

    let app_clone = app.clone();
    let pool = state.db.pool.clone();
    let session_id = session_for_return.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    tauri::async_runtime::spawn(async move {
        let assistant_message_id = uuid::Uuid::new_v4().to_string();
        let ts = now_ms();
        let _ = sqlx::query(
            "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&assistant_message_id)
        .bind(&session_id)
        .bind("assistant")
        .bind("")
        .bind(Option::<String>::None)
        .bind(ts)
        .bind(2)
        .execute(&pool)
        .await;

        let _ = app_clone.emit("cowork_stream_message", json!({
            "sessionId": session_id,
            "message": {
                "id": assistant_message_id,
                "type": "assistant",
                "content": "",
                "timestamp": ts,
                "metadata": { "isStreaming": true }
            }
        }));

        let cfg = extract_simple_api_config(&app_clone);
        let reply = if let Some(cfg) = cfg {
            let history = vec![("user".to_string(), prompt.clone())];
            call_model_once(&cfg, Some(system_prompt.clone()), history)
                .await
                .unwrap_or_else(|e| format!("Error: {}", e))
        } else {
            "API config missing. Please open Settings -> Model and set your API key.".to_string()
        };

        let _ = sqlx::query("UPDATE cowork_messages SET content = ? WHERE id = ?")
            .bind(&reply)
            .bind(&assistant_message_id)
            .execute(&pool)
            .await;

        let _ = app_clone.emit("cowork_stream_message_update", json!({
            "sessionId": session_id,
            "messageId": assistant_message_id,
            "content": reply
        }));

        let _ = sqlx::query("UPDATE cowork_sessions SET status = ?, updated_at = ? WHERE id = ?")
            .bind("completed")
            .bind(now_ms())
            .bind(&session_id)
            .execute(&pool)
            .await;

        let _ = app_clone.emit("cowork_stream_complete", json!({ "sessionId": session_id }));
        let _ = app_clone.emit("cowork_sessions_changed", json!({}));
    });

    Ok(json!({ "success": true, "session": session_for_return }))
}

#[tauri::command]
pub async fn cowork_delete_session(state: State<'_, OpenClawState>, sessionId: String) -> Result<serde_json::Value, String> {
    sqlx::query("DELETE FROM cowork_messages WHERE session_id = ?")
        .bind(&sessionId)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM cowork_sessions WHERE id = ?")
        .bind(&sessionId)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn cowork_list_messages(state: State<'_, OpenClawState>, session_id: String) -> Result<Vec<serde_json::Value>, String> {
    let rows = sqlx::query("SELECT * FROM cowork_messages WHERE session_id = ? ORDER BY sequence ASC, created_at ASC")
        .bind(session_id)
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(json!({
            "id": row.get::<String, _>("id"),
            "sessionId": row.get::<String, _>("session_id"),
            "type": row.get::<String, _>("type"),
            "content": row.get::<String, _>("content"),
            "metadata": serde_json::from_str::<serde_json::Value>(&row.get::<String, _>("metadata")).unwrap_or(json!({})),
            "createdAt": row.get::<i64, _>("created_at"),
            "sequence": row.get::<Option<i32>, _>("sequence")
        }));
    }
    Ok(result)
}

#[tauri::command]
pub async fn cowork_add_message(state: State<'_, OpenClawState>, session_id: String, message: serde_json::Value) -> Result<serde_json::Value, String> {
    let id = message.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let msg_type = message.get("type").and_then(|v| v.as_str()).unwrap_or("user");
    let content = message.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let metadata = message.get("metadata").map(|v| v.to_string()).unwrap_or_else(|| "{}".to_string());
    let now = message.get("createdAt").and_then(|v| v.as_i64()).unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let sequence = message.get("sequence").and_then(|v| v.as_i64()).map(|s| s as i32);

    sqlx::query(
        "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&session_id)
    .bind(msg_type)
    .bind(content)
    .bind(metadata)
    .bind(now)
    .bind(sequence)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    // Update session updated_at
    sqlx::query("UPDATE cowork_sessions SET updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(session_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({ "id": id }))
}

#[tauri::command]
pub async fn cowork_update_message(state: State<'_, OpenClawState>, message_id: String, updates: serde_json::Value) -> Result<(), String> {
    if let Some(content) = updates.get("content").and_then(|v| v.as_str()) {
        sqlx::query("UPDATE cowork_messages SET content = ? WHERE id = ?")
            .bind(content)
            .bind(&message_id)
            .execute(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
    }

    if let Some(metadata) = updates.get("metadata") {
        sqlx::query("UPDATE cowork_messages SET metadata = ? WHERE id = ?")
            .bind(metadata.to_string())
            .bind(&message_id)
            .execute(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn agents_list(state: State<'_, OpenClawState>) -> Result<Vec<serde_json::Value>, String> {
    let rows = sqlx::query("SELECT * FROM agents ORDER BY name ASC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "description": row.get::<String, _>("description"),
            "systemPrompt": row.get::<String, _>("system_prompt"),
            "identity": row.get::<String, _>("identity"),
            "model": row.get::<String, _>("model"),
            "icon": row.get::<String, _>("icon"),
            "skillIds": serde_json::from_str::<Vec<String>>(&row.get::<String, _>("skill_ids")).unwrap_or_default(),
            "enabled": row.get::<i32, _>("enabled") != 0,
            "isDefault": row.get::<i32, _>("is_default") != 0,
            "source": row.get::<String, _>("source"),
            "presetId": row.get::<String, _>("preset_id"),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }));
    }
    Ok(result)
}

#[tauri::command]
pub async fn agents_get(state: State<'_, OpenClawState>, id: String) -> Result<Option<serde_json::Value>, String> {
    let row = sqlx::query("SELECT * FROM agents WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    match row {
        Some(row) => Ok(Some(json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "description": row.get::<String, _>("description"),
            "systemPrompt": row.get::<String, _>("system_prompt"),
            "identity": row.get::<String, _>("identity"),
            "model": row.get::<String, _>("model"),
            "icon": row.get::<String, _>("icon"),
            "skillIds": serde_json::from_str::<Vec<String>>(&row.get::<String, _>("skill_ids")).unwrap_or_default(),
            "enabled": row.get::<i32, _>("enabled") != 0,
            "isDefault": row.get::<i32, _>("is_default") != 0,
            "source": row.get::<String, _>("source"),
            "presetId": row.get::<String, _>("preset_id"),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }))),
        None => Ok(None)
    }
}

#[tauri::command]
pub async fn agents_create(state: State<'_, OpenClawState>, request: serde_json::Value) -> Result<serde_json::Value, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let name = request.get("name").and_then(|v| v.as_str()).unwrap_or("New Agent");
    let description = request.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let system_prompt = request.get("systemPrompt").and_then(|v| v.as_str()).unwrap_or("");
    let identity = request.get("identity").and_then(|v| v.as_str()).unwrap_or("");
    let model = request.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let icon = request.get("icon").and_then(|v| v.as_str()).unwrap_or("");
    let skill_ids = request.get("skillIds").map(|v| v.to_string()).unwrap_or_else(|| "[]".to_string());
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        "INSERT INTO agents (id, name, description, system_prompt, identity, model, icon, skill_ids, enabled, is_default, source, preset_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, 0, 'custom', '', ?, ?)"
    )
    .bind(&id)
    .bind(name)
    .bind(description)
    .bind(system_prompt)
    .bind(identity)
    .bind(model)
    .bind(icon)
    .bind(skill_ids)
    .bind(now)
    .bind(now)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    agents_get(state, id).await.map(|v| v.unwrap_or(json!({})))
}

#[tauri::command]
pub async fn mcp_list(state: State<'_, OpenClawState>) -> Result<serde_json::Value, String> {
    let rows = sqlx::query("SELECT * FROM mcp_servers ORDER BY name ASC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "description": row.get::<String, _>("description"),
            "enabled": row.get::<i32, _>("enabled") != 0,
            "transportType": row.get::<String, _>("transport_type"),
            "configJson": serde_json::from_str::<serde_json::Value>(&row.get::<String, _>("config_json")).unwrap_or(json!({})),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }));
    }
    Ok(json!({ "success": true, "servers": result }))
}

#[tauri::command]
pub async fn mcp_create(state: State<'_, OpenClawState>, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("New MCP Server");
    let description = data.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let transport_type = data.get("transportType").and_then(|v| v.as_str()).unwrap_or("stdio");
    let config_json = data.get("configJson").map(|v| v.to_string()).unwrap_or_else(|| "{}".to_string());
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        "INSERT INTO mcp_servers (id, name, description, enabled, transport_type, config_json, created_at, updated_at)
         VALUES (?, ?, ?, 1, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(name)
    .bind(description)
    .bind(transport_type)
    .bind(config_json)
    .bind(now)
    .bind(now)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    mcp_list(state).await
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc;

static TELEGRAM_QUERY_ID: AtomicU64 = AtomicU64::new(1);

// Store pending query channels
lazy_static::lazy_static! {
    static ref TELEGRAM_QUERY_CHANNELS: Mutex<HashMap<u64, mpsc::Sender<serde_json::Value>>> = Mutex::new(HashMap::new());
}

/// Query Telegram data from frontend via event bridge
/// This is called by MCP Bridge Server to get Telegram data
#[tauri::command]
pub async fn telegram_query(
    app: AppHandle,
    query_type: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let query_id = TELEGRAM_QUERY_ID.fetch_add(1, Ordering::SeqCst);

    // Create a channel for the response
    let (tx, rx) = mpsc::channel();

    // Store the sender
    {
        let mut channels = TELEGRAM_QUERY_CHANNELS.lock().map_err(|e| e.to_string())?;
        channels.insert(query_id, tx);
    }

    // Emit query event to frontend
    app.emit("telegram-query", json!({
        "queryId": query_id,
        "queryType": query_type,
        "params": params
    })).map_err(|e| e.to_string())?;

    // Wait for response with timeout (10 seconds)
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(result) => Ok(result),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Clean up on timeout
            let mut channels = TELEGRAM_QUERY_CHANNELS.lock().map_err(|e| e.to_string())?;
            channels.remove(&query_id);
            Err("Query timeout".to_string())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Query channel closed".to_string())
        }
    }
}

/// Called by frontend to respond to a telegram query
#[tauri::command]
pub async fn telegram_query_response(
    query_id: u64,
    result: serde_json::Value,
) -> Result<(), String> {
    let tx = {
        let mut channels = TELEGRAM_QUERY_CHANNELS.lock().map_err(|e| e.to_string())?;
        channels.remove(&query_id)
    };

    if let Some(tx) = tx {
        tx.send(result).map_err(|_| "Failed to send response".to_string())?;
    }

    Ok(())
}
