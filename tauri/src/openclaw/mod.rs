pub mod db;
pub mod default_system_prompt;
pub mod mcp_bridge;
pub mod mcp_manager;
pub mod scheduled_tasks;

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

/// Call model with tool support - handles the full tool use loop
/// Returns (final_text, total_tool_seq_offset)
pub async fn call_model_with_tools(
    app: AppHandle,
    pool: sqlx::Pool<sqlx::Sqlite>,
    session_id: String,
    cfg: &SimpleApiConfig,
    system_prompt: String,
    initial_prompt: String,
    base_sequence: i64,
) -> (String, i64) {
    // Get tool definitions from frontend
    let tools: Vec<serde_json::Value> = {
        let result = telegram_query(
            app.clone(),
            "getToolDefinitions".to_string(),
            json!({}),
        ).await;
        match result {
            Ok(v) => {
                // Frontend returns { success: true, data: [...] }
                let arr = v.get("data").and_then(|d| d.as_array())
                    .or_else(|| v.as_array())
                    .or_else(|| v.get("tools").and_then(|t| t.as_array()));
                if let Some(arr) = arr {
                    arr.iter().filter_map(|t| {
                        let name = t.get("name")?.as_str()?;
                        let desc = t.get("description").and_then(|d| d.as_str()).unwrap_or("");
                        // Support both "input_schema" and "inputSchema"
                        let schema = t.get("input_schema")
                            .or_else(|| t.get("inputSchema"))
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object"}));
                        Some(json!({
                            "name": name,
                            "description": desc,
                            "input_schema": schema
                        }))
                    }).collect()
                } else {
                    vec![]
                }
            }
            Err(_) => vec![]
        }
    };

    let base_url = cfg.base_url.trim_end_matches('/');
    let url = format!("{}/v1/messages", base_url);
    let api_key = &cfg.api_key;
    let model = &cfg.model;
    let client = reqwest::Client::new();

    let system_for_request = if system_prompt.trim().is_empty() {
        default_system_prompt::DEFAULT_SYSTEM_PROMPT.to_string()
    } else {
        system_prompt
    };

    let mut messages: Vec<serde_json::Value> = vec![json!({ "role": "user", "content": initial_prompt })];
    let mut seq = base_sequence;
    let mut total_tool_seq_offset: i64 = 0;

    let mut steps = 0;
    let mut final_text = String::new();

    loop {
        steps += 1;
        if steps > 6 {
            break;
        }

        let resp = client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-api-key", api_key.clone())
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "max_tokens": 1024,
                "system": system_for_request,
                "messages": messages,
                "tools": tools,
            }))
            .send()
            .await;

        let raw_text = match resp {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(e) => {
                final_text = format!("Error: {}", e);
                break;
            }
        };

        let v = match serde_json::from_str::<serde_json::Value>(&raw_text) {
            Ok(v) => v,
            Err(_) => {
                final_text = raw_text;
                break;
            }
        };

        let content = match v.get("content").and_then(|c| c.as_array()) {
            Some(arr) => arr.clone(),
            None => {
                final_text = raw_text;
                break;
            }
        };

        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut text_out = String::new();
        for item in &content {
            match item.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        text_out.push_str(t);
                    }
                }
                Some("tool_use") => {
                    let id = item.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let input = item.get("input").cloned().unwrap_or_else(|| json!({}));
                    if !id.is_empty() && !name.is_empty() {
                        tool_uses.push((id, name, input));
                    }
                }
                _ => {}
            }
        }

        messages.push(json!({ "role": "assistant", "content": content }));

        if tool_uses.is_empty() {
            final_text = if !text_out.trim().is_empty() { text_out } else { raw_text };
            break;
        }

        let mut tool_seq_offset: i64 = 0;
        for (tool_use_id, tool_name, tool_input) in tool_uses {
            let tool_msg_id = uuid::Uuid::new_v4().to_string();
            let tool_ts = chrono::Utc::now().timestamp_millis();

            let _ = sqlx::query(
                "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&tool_msg_id)
            .bind(&session_id)
            .bind("tool_use")
            .bind("")
            .bind(Some(json!({
                "toolName": &tool_name,
                "toolInput": &tool_input,
                "toolUseId": &tool_use_id,
            }).to_string()))
            .bind(tool_ts)
            .bind(seq + 10 + tool_seq_offset)
            .execute(&pool)
            .await;
            tool_seq_offset += 1;

            let _ = app.emit("cowork_stream_message", json!({
                "sessionId": session_id,
                "message": {
                    "id": tool_msg_id,
                    "type": "tool_use",
                    "content": "",
                    "timestamp": tool_ts,
                    "sequence": seq + 10 + tool_seq_offset - 1,
                    "metadata": {
                        "toolName": &tool_name,
                        "toolInput": &tool_input,
                        "toolUseId": &tool_use_id,
                    }
                }
            }));

            // Execute tool via frontend's unified executeTool
            let (tool_ok, tool_result_value) = {
                let result = telegram_query(
                    app.clone(),
                    "executeTool".to_string(),
                    json!({ "toolName": tool_name, "toolInput": tool_input }),
                ).await;
                match result {
                    Ok(v) => {
                        let success = v.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
                        (success, v)
                    }
                    Err(e) => (false, json!({ "success": false, "error": e })),
                }
            };

            let tool_result_text = serde_json::to_string(&tool_result_value).unwrap_or_else(|_| "{\"success\":false}".to_string());

            let tool_result_msg_id = uuid::Uuid::new_v4().to_string();
            let tool_result_ts = chrono::Utc::now().timestamp_millis();

            let _ = sqlx::query(
                "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&tool_result_msg_id)
            .bind(&session_id)
            .bind("tool_result")
            .bind(&tool_result_text)
            .bind(Some(json!({
                "toolUseId": &tool_use_id,
                "toolName": &tool_name,
                "toolResult": &tool_result_text,
                "isError": !tool_ok
            }).to_string()))
            .bind(tool_result_ts)
            .bind(seq + 10 + tool_seq_offset)
            .execute(&pool)
            .await;
            tool_seq_offset += 1;

            let _ = app.emit("cowork_stream_message", json!({
                "sessionId": session_id,
                "message": {
                    "id": tool_result_msg_id,
                    "type": "tool_result",
                    "content": tool_result_text.clone(),
                    "timestamp": tool_result_ts,
                    "sequence": seq + 10 + tool_seq_offset - 1,
                    "metadata": {
                        "toolUseId": &tool_use_id,
                        "toolName": &tool_name,
                        "isError": !tool_ok
                    }
                }
            }));

            messages.push(json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": tool_result_text,
                    "is_error": !tool_ok
                }]
            }));
        }
        seq += tool_seq_offset;
        total_tool_seq_offset += tool_seq_offset;
    }

    let result_text = if final_text.trim().is_empty() {
        "Empty response".to_string()
    } else {
        final_text
    };
    (result_text, total_tool_seq_offset)
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
            "sequence": 1,
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
                "sequence": 2,
                "metadata": { "isStreaming": true }
            }
        }));

        let cfg = extract_simple_api_config(&app_clone);
        let (reply, total_tool_seq_offset) = if let Some(cfg) = cfg {
            call_model_with_tools(
                app_clone.clone(),
                pool.clone(),
                session_id.clone(),
                &cfg,
                system_prompt.clone(),
                prompt.clone(),
                2, // base_sequence after user message (1) and assistant placeholder (2)
            ).await
        } else {
            ("API config missing. Please open Settings -> Model and set your API key.".to_string(), 0)
        };

        // If tools were used, create a new assistant message with correct sequence
        // Otherwise, update the original assistant message
        if total_tool_seq_offset > 0 {
            // Delete the placeholder assistant message
            let _ = sqlx::query("DELETE FROM cowork_messages WHERE id = ?")
                .bind(&assistant_message_id)
                .execute(&pool)
                .await;

            let final_msg_id = uuid::Uuid::new_v4().to_string();
            let final_ts = now_ms();
            let final_seq = 2 + 10 + total_tool_seq_offset;

            let _ = sqlx::query(
                "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&final_msg_id)
            .bind(&session_id)
            .bind("assistant")
            .bind(&reply)
            .bind(None::<String>)
            .bind(final_ts)
            .bind(final_seq)
            .execute(&pool)
            .await;

            let _ = app_clone.emit("cowork_stream_message", json!({
                "sessionId": session_id,
                "message": {
                    "id": final_msg_id,
                    "type": "assistant",
                    "content": reply,
                    "timestamp": final_ts,
                    "sequence": final_seq
                }
            }));
        } else {
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
        }

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

// ============================================================================
// Scheduled Tasks Commands
// ============================================================================

#[tauri::command]
pub async fn scheduled_tasks_list(
    state: State<'_, OpenClawState>,
) -> Result<serde_json::Value, String> {
    log::info!("[scheduled_tasks_list] Listing all tasks");
    match scheduled_tasks::db_list_tasks(&state.db).await {
        Ok(tasks) => {
            log::info!("[scheduled_tasks_list] Found {} tasks", tasks.len());
            Ok(json!({ "success": true, "tasks": tasks }))
        }
        Err(e) => {
            log::error!("[scheduled_tasks_list] Failed to list tasks: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn scheduled_tasks_get(
    state: State<'_, OpenClawState>,
    taskId: String,
) -> Result<serde_json::Value, String> {
    match scheduled_tasks::db_get_task(&state.db, &taskId).await? {
        Some(task) => Ok(json!({ "success": true, "task": task })),
        None => Ok(json!({ "success": false, "error": "Task not found" })),
    }
}

#[tauri::command]
pub async fn scheduled_tasks_create(
    state: State<'_, OpenClawState>,
    input: scheduled_tasks::CreateTaskInput,
) -> Result<serde_json::Value, String> {
    log::info!("[scheduled_tasks_create] Received input: {:?}", input);
    match scheduled_tasks::db_create_task(&state.db, input).await {
        Ok(task) => {
            log::info!("[scheduled_tasks_create] Task created successfully: {:?}", task.id);
            Ok(json!({ "success": true, "task": task }))
        }
        Err(e) => {
            log::error!("[scheduled_tasks_create] Failed to create task: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn scheduled_tasks_update(
    state: State<'_, OpenClawState>,
    taskId: String,
    input: scheduled_tasks::UpdateTaskInput,
) -> Result<serde_json::Value, String> {
    let task = scheduled_tasks::db_update_task(&state.db, &taskId, input).await?;
    Ok(json!({ "success": true, "task": task }))
}

#[tauri::command]
pub async fn scheduled_tasks_delete(
    state: State<'_, OpenClawState>,
    taskId: String,
) -> Result<serde_json::Value, String> {
    scheduled_tasks::db_delete_task(&state.db, &taskId).await?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn scheduled_tasks_toggle(
    state: State<'_, OpenClawState>,
    taskId: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    let task = scheduled_tasks::db_toggle_task(&state.db, &taskId, enabled).await?;
    Ok(json!({ "success": true, "task": task }))
}

#[tauri::command]
pub async fn scheduled_tasks_get_runs(
    state: State<'_, OpenClawState>,
    taskId: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);
    let runs = scheduled_tasks::db_list_runs(&state.db, &taskId, limit, offset).await?;
    let total = scheduled_tasks::db_count_runs(&state.db, &taskId).await?;
    Ok(json!({ "success": true, "runs": runs, "total": total }))
}

#[tauri::command]
pub async fn scheduled_tasks_list_all_runs(
    state: State<'_, OpenClawState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    let runs = scheduled_tasks::db_list_all_runs(&state.db, limit, offset).await?;
    Ok(json!({ "success": true, "runs": runs }))
}

#[tauri::command]
pub async fn scheduled_tasks_run_manually(
    app: AppHandle,
    state: State<'_, OpenClawState>,
    taskId: String,
) -> Result<serde_json::Value, String> {
    // Get the task
    let task = scheduled_tasks::db_get_task(&state.db, &taskId).await?
        .ok_or_else(|| "Task not found".to_string())?;

    // Create a run record
    let run = scheduled_tasks::db_create_run(&state.db, &task).await?;

    // Mark task as running
    scheduled_tasks::db_mark_task_running(&state.db, &taskId).await?;

    // Emit run started event
    let _ = app.emit("scheduled_tasks_run_update", json!({
        "type": "started",
        "run": &run
    }));

    // Spawn async task to execute
    let app_clone = app.clone();
    let pool = state.db.pool.clone();
    let run_id = run.id.clone();
    let task_id = taskId.clone();

    tauri::async_runtime::spawn(async move {
        let start_time = now_ms();
        let db = db::OpenClawDb { pool: pool.clone() };

        // Get the message from payload
        let message = scheduled_tasks::get_payload_message(&task.payload);

        // Create an isolated cowork session for this task run
        let session_id = uuid::Uuid::new_v4().to_string();
        let session_key = format!("cron:{}", task.id);
        let now = now_ms();

        // Create session in database
        let session_result = sqlx::query(
            "INSERT INTO cowork_sessions (id, title, status, cwd, system_prompt, execution_mode, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&session_id)
        .bind(format!("Scheduled: {}", task.name))
        .bind("running")
        .bind("")
        .bind("")
        .bind("local")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await;

        if let Err(e) = session_result {
            log::error!("[scheduled_task_run] Failed to create session: {}", e);
            let _ = scheduled_tasks::db_complete_run(&db, &run_id, None, Some(&e.to_string())).await;
            let duration_ms = now_ms() - start_time;
            let _ = scheduled_tasks::db_mark_task_completed(&db, &task_id, false, Some(&e.to_string()), duration_ms).await;
            return;
        }

        // Insert user message
        let user_msg_id = uuid::Uuid::new_v4().to_string();
        let _ = sqlx::query(
            "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&user_msg_id)
        .bind(&session_id)
        .bind("user")
        .bind(&message)
        .bind("{}")
        .bind(now)
        .bind(1)
        .execute(&pool)
        .await;

        // Get API config and call model
        let cfg = extract_simple_api_config(&app_clone);
        let (reply, _) = if let Some(cfg) = cfg {
            call_model_with_tools(
                app_clone.clone(),
                pool.clone(),
                session_id.clone(),
                &cfg,
                String::new(), // Use default system prompt
                message.clone(),
                2,
            ).await
        } else {
            ("API config missing. Please configure your API key in Settings.".to_string(), 0)
        };

        // Insert assistant response
        let assistant_msg_id = uuid::Uuid::new_v4().to_string();
        let response_time = now_ms();
        let _ = sqlx::query(
            "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&assistant_msg_id)
        .bind(&session_id)
        .bind("assistant")
        .bind(&reply)
        .bind("{}")
        .bind(response_time)
        .bind(100)
        .execute(&pool)
        .await;

        // Update session status
        let _ = sqlx::query("UPDATE cowork_sessions SET status = ?, updated_at = ? WHERE id = ?")
            .bind("completed")
            .bind(response_time)
            .bind(&session_id)
            .execute(&pool)
            .await;

        // Handle delivery if configured
        if let scheduled_tasks::TaskDelivery::Announce { channel, to, .. } = &task.delivery {
            if let (Some(channel), Some(to)) = (channel, to) {
                if channel == "telegram" {
                    // Send result to Telegram chat
                    let delivery_result = telegram_query(
                        app_clone.clone(),
                        "sendScheduledTaskResult".to_string(),
                        json!({
                            "chatId": to,
                            "taskName": task.name,
                            "result": reply
                        }),
                    ).await;

                    if let Err(e) = delivery_result {
                        log::warn!("[scheduled_task_run] Failed to deliver to Telegram: {}", e);
                    }
                }
            }
        }

        // Complete the run
        let duration_ms = now_ms() - start_time;
        let completed_run = scheduled_tasks::db_complete_run(&db, &run_id, Some(&session_id), None).await;
        let _ = scheduled_tasks::db_mark_task_completed(&db, &task_id, true, None, duration_ms).await;

        // Emit completion events
        if let Ok(run) = completed_run {
            let _ = app_clone.emit("scheduled_tasks_run_update", json!({
                "type": "completed",
                "run": run
            }));
        }

        // Emit status update
        if let Ok(Some(updated_task)) = scheduled_tasks::db_get_task(&db, &task_id).await {
            let _ = app_clone.emit("scheduled_tasks_status_update", json!({
                "taskId": task_id,
                "state": updated_task.state
            }));
        }
    });

    Ok(json!({ "success": true, "run": run }))
}

#[tauri::command]
pub async fn scheduled_tasks_stop(
    state: State<'_, OpenClawState>,
    taskId: String,
) -> Result<serde_json::Value, String> {
    // Update task state to not running (clear running_at_ms)
    let idle_state = scheduled_tasks::TaskState {
        running_at_ms: None,
        ..Default::default()
    };
    scheduled_tasks::db_update_task_state(&state.db, &taskId, &idle_state).await?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn scheduled_tasks_list_runs(
    state: State<'_, OpenClawState>,
    taskId: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);
    let runs = scheduled_tasks::db_list_runs(&state.db, &taskId, limit, offset).await?;
    Ok(json!({ "success": true, "runs": runs }))
}

#[tauri::command]
pub async fn scheduled_tasks_count_runs(
    state: State<'_, OpenClawState>,
    taskId: String,
) -> Result<serde_json::Value, String> {
    let total = scheduled_tasks::db_count_runs(&state.db, &taskId).await?;
    Ok(json!({ "success": true, "total": total }))
}

#[tauri::command]
pub async fn scheduled_tasks_resolve_session(
    state: State<'_, OpenClawState>,
    sessionKey: String,
) -> Result<serde_json::Value, String> {
    match scheduled_tasks::db_get_task_by_session_key(&state.db, &sessionKey).await? {
        Some(task) => Ok(json!({ "success": true, "task": task })),
        None => Ok(json!({ "success": false, "error": "No task found for session key" })),
    }
}

#[tauri::command]
pub async fn scheduled_tasks_list_channels(
    _state: State<'_, OpenClawState>,
) -> Result<serde_json::Value, String> {
    // This would list available IM channels (Telegram, WeChat, etc.)
    // For now, return empty list - will be implemented when IM gateway is ready
    Ok(json!({ "success": true, "channels": [] }))
}

#[tauri::command]
pub async fn scheduled_tasks_list_channel_conversations(
    _state: State<'_, OpenClawState>,
    _channelId: String,
    _accountId: Option<String>,
) -> Result<serde_json::Value, String> {
    // This would list conversations in a specific channel
    // For now, return empty list - will be implemented when IM gateway is ready
    Ok(json!({ "success": true, "conversations": [] }))
}
