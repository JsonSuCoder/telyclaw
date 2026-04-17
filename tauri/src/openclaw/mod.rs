pub mod db;

use db::OpenClawDb;
use serde_json::json;
use tauri::State;
use sqlx::Row;
use chrono;

pub struct OpenClawState {
    pub db: OpenClawDb,
}

#[tauri::command]
pub async fn openclaw_engine_get_status() -> serde_json::Value {
    json!({
        "phase": "not_installed",
        "version": null,
        "message": "OpenClaw engine is not fully implemented in Tauri yet.",
        "canRetry": true
    })
}

#[tauri::command]
pub async fn cowork_list_sessions(state: State<'_, OpenClawState>) -> Result<Vec<serde_json::Value>, String> {
    let rows = sqlx::query("SELECT * FROM cowork_sessions ORDER BY updated_at DESC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(json!({
            "id": row.get::<String, _>("id"),
            "title": row.get::<String, _>("title"),
            "status": row.get::<String, _>("status"),
            "pinned": row.get::<i32, _>("pinned") != 0,
            "cwd": row.get::<String, _>("cwd"),
            "systemPrompt": row.get::<String, _>("system_prompt"),
            "executionMode": row.get::<Option<String>, _>("execution_mode"),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }));
    }
    Ok(result)
}

#[tauri::command]
pub async fn cowork_get_session(state: State<'_, OpenClawState>, session_id: String) -> Result<Option<serde_json::Value>, String> {
    let row = sqlx::query("SELECT * FROM cowork_sessions WHERE id = ?")
        .bind(&session_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    match row {
        Some(row) => Ok(Some(json!({
            "id": row.get::<String, _>("id"),
            "title": row.get::<String, _>("title"),
            "status": row.get::<String, _>("status"),
            "pinned": row.get::<i32, _>("pinned") != 0,
            "cwd": row.get::<String, _>("cwd"),
            "systemPrompt": row.get::<String, _>("system_prompt"),
            "executionMode": row.get::<Option<String>, _>("execution_mode"),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }))),
        None => Ok(None)
    }
}

#[tauri::command]
pub async fn cowork_start_session(state: State<'_, OpenClawState>, options: serde_json::Value) -> Result<String, String> {
    let id = options.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let title = options.get("title").and_then(|v| v.as_str()).unwrap_or("New Session");
    let cwd = options.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
    let system_prompt = options.get("systemPrompt").and_then(|v| v.as_str()).unwrap_or("");
    let execution_mode = options.get("executionMode").and_then(|v| v.as_str());
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        "INSERT INTO cowork_sessions (id, title, status, cwd, system_prompt, execution_mode, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(title)
    .bind("idle")
    .bind(cwd)
    .bind(system_prompt)
    .bind(execution_mode)
    .bind(now)
    .bind(now)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(id)
}

#[tauri::command]
pub async fn cowork_delete_session(state: State<'_, OpenClawState>, session_id: String) -> Result<(), String> {
    sqlx::query("DELETE FROM cowork_sessions WHERE id = ?")
        .bind(session_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
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
