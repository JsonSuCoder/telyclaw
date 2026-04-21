use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::db::OpenClawDb;

// ============================================================================
// Data Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub schedule: TaskSchedule,
    pub session_target: String,
    pub wake_mode: String,
    pub payload: TaskPayload,
    pub delivery: TaskDelivery,
    pub agent_id: Option<String>,
    pub session_key: Option<String>,
    pub state: TaskState,
    pub created_at: String,  // ISO 8601 string
    pub updated_at: String,  // ISO 8601 string
}

// Schedule types matching frontend: { kind: 'at' | 'every' | 'cron', ... }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskSchedule {
    #[serde(rename = "at")]
    At { at: String },
    #[serde(rename = "every")]
    Every {
        #[serde(rename = "everyMs")]
        every_ms: u64,
        #[serde(rename = "anchorMs", default)]
        anchor_ms: Option<u64>,
    },
    #[serde(rename = "cron")]
    Cron {
        expr: String,
        #[serde(default)]
        tz: Option<String>,
        #[serde(rename = "staggerMs", default)]
        stagger_ms: Option<u64>,
    },
}

// Payload types matching frontend: { kind: 'agentTurn' | 'systemEvent', ... }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskPayload {
    #[serde(rename = "agentTurn")]
    AgentTurn {
        message: String,
        #[serde(rename = "timeoutSeconds", default)]
        timeout_seconds: Option<u32>,
        #[serde(default)]
        model: Option<String>,
    },
    #[serde(rename = "systemEvent")]
    SystemEvent {
        text: String,
    },
}

// Delivery types matching frontend: { mode: 'none' | 'announce' | 'webhook', ... }
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum TaskDelivery {
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "announce")]
    Announce {
        #[serde(default)]
        channel: Option<String>,
        #[serde(default)]
        to: Option<String>,
        #[serde(rename = "accountId", default)]
        account_id: Option<String>,
        #[serde(rename = "bestEffort", default)]
        best_effort: Option<bool>,
    },
    #[serde(rename = "webhook")]
    Webhook {
        #[serde(default)]
        to: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskState {
    #[serde(default)]
    pub next_run_at_ms: Option<i64>,
    #[serde(default)]
    pub last_run_at_ms: Option<i64>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_duration_ms: Option<i64>,
    #[serde(default)]
    pub running_at_ms: Option<i64>,
    #[serde(default)]
    pub consecutive_errors: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub id: String,
    pub task_id: String,
    pub task_name: String,
    pub session_id: Option<String>,
    pub session_key: Option<String>,
    pub status: String,
    pub started_at: String,      // ISO 8601 string
    pub finished_at: Option<String>,  // ISO 8601 string
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    pub schedule: TaskSchedule,
    #[serde(default)]
    pub session_target: Option<String>,
    #[serde(default)]
    pub wake_mode: Option<String>,
    pub payload: TaskPayload,
    #[serde(default)]
    pub delivery: Option<TaskDelivery>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub session_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub schedule: Option<TaskSchedule>,
    #[serde(default)]
    pub session_target: Option<String>,
    #[serde(default)]
    pub wake_mode: Option<String>,
    #[serde(default)]
    pub payload: Option<TaskPayload>,
    #[serde(default)]
    pub delivery: Option<TaskDelivery>,
    #[serde(default)]
    pub agent_id: Option<String>,
}

// ============================================================================
// Scheduler State
// ============================================================================

pub struct SchedulerState {
    pub running_tasks: HashMap<String, tokio::task::JoinHandle<()>>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            running_tasks: HashMap::new(),
        }
    }
}

pub type SharedSchedulerState = Arc<RwLock<SchedulerState>>;

pub fn create_scheduler_state() -> SharedSchedulerState {
    Arc::new(RwLock::new(SchedulerState::default()))
}

// ============================================================================
// Database Operations
// ============================================================================

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn millis_to_iso8601(millis: i64) -> String {
    DateTime::from_timestamp_millis(millis)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| Utc::now().to_rfc3339())
}

fn row_to_task(row: &sqlx::sqlite::SqliteRow) -> Result<ScheduledTask, String> {
    let schedule_json: String = row.try_get("schedule_json").map_err(|e| e.to_string())?;
    let payload_json: String = row.try_get("payload_json").map_err(|e| e.to_string())?;
    let delivery_json: String = row.try_get("delivery_json").map_err(|e| e.to_string())?;
    let state_json: String = row.try_get("state_json").map_err(|e| e.to_string())?;

    let created_at_ms: i64 = row.try_get("created_at").map_err(|e| e.to_string())?;
    let updated_at_ms: i64 = row.try_get("updated_at").map_err(|e| e.to_string())?;

    Ok(ScheduledTask {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        name: row.try_get("name").map_err(|e| e.to_string())?,
        description: row.try_get("description").map_err(|e| e.to_string())?,
        enabled: row.try_get::<i32, _>("enabled").map_err(|e| e.to_string())? != 0,
        schedule: serde_json::from_str(&schedule_json).map_err(|e| e.to_string())?,
        session_target: row.try_get("session_target").map_err(|e| e.to_string())?,
        wake_mode: row.try_get("wake_mode").map_err(|e| e.to_string())?,
        payload: serde_json::from_str(&payload_json).map_err(|e| e.to_string())?,
        delivery: serde_json::from_str(&delivery_json).unwrap_or_default(),
        agent_id: row.try_get("agent_id").ok(),
        session_key: row.try_get("session_key").ok(),
        state: serde_json::from_str(&state_json).unwrap_or_default(),
        created_at: millis_to_iso8601(created_at_ms),
        updated_at: millis_to_iso8601(updated_at_ms),
    })
}

fn row_to_run(row: &sqlx::sqlite::SqliteRow) -> Result<TaskRun, String> {
    let started_at_ms: i64 = row.try_get("started_at").map_err(|e| e.to_string())?;
    let finished_at_ms: Option<i64> = row.try_get("finished_at").ok();

    Ok(TaskRun {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        task_id: row.try_get("task_id").map_err(|e| e.to_string())?,
        task_name: row.try_get("task_name").map_err(|e| e.to_string())?,
        session_id: row.try_get("session_id").ok(),
        session_key: row.try_get("session_key").ok(),
        status: row.try_get("status").map_err(|e| e.to_string())?,
        started_at: millis_to_iso8601(started_at_ms),
        finished_at: finished_at_ms.map(millis_to_iso8601),
        duration_ms: row.try_get("duration_ms").ok(),
        error: row.try_get("error").ok(),
    })
}

pub async fn db_list_tasks(db: &OpenClawDb) -> Result<Vec<ScheduledTask>, String> {
    let rows = sqlx::query("SELECT * FROM scheduled_tasks ORDER BY created_at DESC")
        .fetch_all(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_task).collect()
}

pub async fn db_get_task(db: &OpenClawDb, id: &str) -> Result<Option<ScheduledTask>, String> {
    let row = sqlx::query("SELECT * FROM scheduled_tasks WHERE id = ?")
        .bind(id)
        .fetch_optional(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    match row {
        Some(r) => Ok(Some(row_to_task(&r)?)),
        None => Ok(None),
    }
}

pub async fn db_create_task(db: &OpenClawDb, input: CreateTaskInput) -> Result<ScheduledTask, String> {
    log::info!("[db_create_task] Starting task creation with name: {}", input.name);

    let id = Uuid::new_v4().to_string();
    let now = now_millis();

    log::info!("[db_create_task] Serializing schedule...");
    let schedule_json = serde_json::to_string(&input.schedule).map_err(|e| {
        log::error!("[db_create_task] Failed to serialize schedule: {}", e);
        e.to_string()
    })?;

    log::info!("[db_create_task] Serializing payload...");
    let payload_json = serde_json::to_string(&input.payload).map_err(|e| {
        log::error!("[db_create_task] Failed to serialize payload: {}", e);
        e.to_string()
    })?;

    log::info!("[db_create_task] Serializing delivery...");
    let delivery_json = match &input.delivery {
        Some(d) => serde_json::to_string(d).map_err(|e| {
            log::error!("[db_create_task] Failed to serialize delivery: {}", e);
            e.to_string()
        })?,
        None => "null".to_string(),
    };

    // Calculate initial next_run_at_ms based on schedule
    log::info!("[db_create_task] Calculating next run time...");
    let next_run_at_ms = calculate_next_run(&input.schedule);
    let state = TaskState {
        next_run_at_ms,
        ..Default::default()
    };
    let state_json = serde_json::to_string(&state).map_err(|e| {
        log::error!("[db_create_task] Failed to serialize state: {}", e);
        e.to_string()
    })?;
    let enabled = input.enabled.unwrap_or(true);

    log::info!("[db_create_task] Inserting into database with id: {}", id);
    sqlx::query(
        "INSERT INTO scheduled_tasks (id, name, description, enabled, schedule_json, session_target, wake_mode, payload_json, delivery_json, agent_id, session_key, state_json, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&input.name)
    .bind(input.description.as_deref().unwrap_or(""))
    .bind(if enabled { 1 } else { 0 })
    .bind(&schedule_json)
    .bind(input.session_target.as_deref().unwrap_or("main"))
    .bind(input.wake_mode.as_deref().unwrap_or("now"))
    .bind(&payload_json)
    .bind(&delivery_json)
    .bind(&input.agent_id)
    .bind(&input.session_key)
    .bind(&state_json)
    .bind(now)
    .bind(now)
    .execute(&db.pool)
    .await
    .map_err(|e| {
        log::error!("[db_create_task] Database insert failed: {}", e);
        e.to_string()
    })?;

    log::info!("[db_create_task] Task inserted, retrieving...");
    db_get_task(db, &id).await?.ok_or_else(|| "Failed to retrieve created task".to_string())
}

pub async fn db_update_task(db: &OpenClawDb, id: &str, input: UpdateTaskInput) -> Result<ScheduledTask, String> {
    let existing = db_get_task(db, id).await?.ok_or_else(|| "Task not found".to_string())?;
    let now = now_millis();

    let name = input.name.unwrap_or(existing.name);
    let description = input.description.unwrap_or(existing.description);
    let enabled = input.enabled.unwrap_or(existing.enabled);
    let schedule = input.schedule.unwrap_or(existing.schedule);
    let session_target = input.session_target.unwrap_or(existing.session_target);
    let wake_mode = input.wake_mode.unwrap_or(existing.wake_mode);
    let payload = input.payload.unwrap_or(existing.payload);
    let delivery = input.delivery.unwrap_or(existing.delivery);
    let agent_id = input.agent_id.or(existing.agent_id);

    let schedule_json = serde_json::to_string(&schedule).map_err(|e| e.to_string())?;
    let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let delivery_json = serde_json::to_string(&delivery).map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE scheduled_tasks SET name = ?, description = ?, enabled = ?, schedule_json = ?, session_target = ?, wake_mode = ?, payload_json = ?, delivery_json = ?, agent_id = ?, updated_at = ? WHERE id = ?"
    )
    .bind(&name)
    .bind(&description)
    .bind(if enabled { 1 } else { 0 })
    .bind(&schedule_json)
    .bind(&session_target)
    .bind(&wake_mode)
    .bind(&payload_json)
    .bind(&delivery_json)
    .bind(&agent_id)
    .bind(now)
    .bind(id)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;

    db_get_task(db, id).await?.ok_or_else(|| "Failed to retrieve updated task".to_string())
}

pub async fn db_delete_task(db: &OpenClawDb, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM scheduled_tasks WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn db_toggle_task(db: &OpenClawDb, id: &str, enabled: bool) -> Result<ScheduledTask, String> {
    let now = now_millis();
    sqlx::query("UPDATE scheduled_tasks SET enabled = ?, updated_at = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(now)
        .bind(id)
        .execute(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    db_get_task(db, id).await?.ok_or_else(|| "Task not found".to_string())
}

pub async fn db_update_task_state(db: &OpenClawDb, id: &str, state: &TaskState) -> Result<(), String> {
    let now = now_millis();
    let state_json = serde_json::to_string(state).map_err(|e| e.to_string())?;

    sqlx::query("UPDATE scheduled_tasks SET state_json = ?, updated_at = ? WHERE id = ?")
        .bind(&state_json)
        .bind(now)
        .bind(id)
        .execute(&db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn db_set_task_session_key(db: &OpenClawDb, id: &str, session_key: &str) -> Result<(), String> {
    let now = now_millis();
    sqlx::query("UPDATE scheduled_tasks SET session_key = ?, updated_at = ? WHERE id = ?")
        .bind(session_key)
        .bind(now)
        .bind(id)
        .execute(&db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Task Runs

pub async fn db_create_run(db: &OpenClawDb, task: &ScheduledTask) -> Result<TaskRun, String> {
    let id = Uuid::new_v4().to_string();
    let now = now_millis();

    sqlx::query(
        "INSERT INTO scheduled_task_runs (id, task_id, task_name, session_id, session_key, status, started_at)
         VALUES (?, ?, ?, NULL, ?, 'running', ?)"
    )
    .bind(&id)
    .bind(&task.id)
    .bind(&task.name)
    .bind(&task.session_key)
    .bind(now)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;

    let row = sqlx::query("SELECT * FROM scheduled_task_runs WHERE id = ?")
        .bind(&id)
        .fetch_one(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    row_to_run(&row)
}

pub async fn db_complete_run(db: &OpenClawDb, run_id: &str, session_id: Option<&str>, error: Option<&str>) -> Result<TaskRun, String> {
    let now = now_millis();
    let status = if error.is_some() { "failed" } else { "completed" };

    // Get started_at to calculate duration
    let row = sqlx::query("SELECT started_at FROM scheduled_task_runs WHERE id = ?")
        .bind(run_id)
        .fetch_one(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let started_at: i64 = row.try_get("started_at").map_err(|e| e.to_string())?;
    let duration_ms = now - started_at;

    sqlx::query(
        "UPDATE scheduled_task_runs SET status = ?, session_id = ?, finished_at = ?, duration_ms = ?, error = ? WHERE id = ?"
    )
    .bind(status)
    .bind(session_id)
    .bind(now)
    .bind(duration_ms)
    .bind(error)
    .bind(run_id)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;

    // Fetch and return the updated run
    let row = sqlx::query("SELECT * FROM scheduled_task_runs WHERE id = ?")
        .bind(run_id)
        .fetch_one(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    row_to_run(&row)
}

pub async fn db_list_runs(db: &OpenClawDb, task_id: &str, limit: i64, offset: i64) -> Result<Vec<TaskRun>, String> {
    let rows = sqlx::query(
        "SELECT * FROM scheduled_task_runs WHERE task_id = ? ORDER BY started_at DESC LIMIT ? OFFSET ?"
    )
    .bind(task_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_run).collect()
}

pub async fn db_count_runs(db: &OpenClawDb, task_id: &str) -> Result<i64, String> {
    let row = sqlx::query("SELECT COUNT(*) as count FROM scheduled_task_runs WHERE task_id = ?")
        .bind(task_id)
        .fetch_one(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    row.try_get("count").map_err(|e| e.to_string())
}

pub async fn db_list_all_runs(db: &OpenClawDb, limit: i64, offset: i64) -> Result<Vec<TaskRun>, String> {
    let rows = sqlx::query(
        "SELECT * FROM scheduled_task_runs ORDER BY started_at DESC LIMIT ? OFFSET ?"
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_run).collect()
}

pub async fn db_get_task_by_session_key(db: &OpenClawDb, session_key: &str) -> Result<Option<ScheduledTask>, String> {
    let row = sqlx::query("SELECT * FROM scheduled_tasks WHERE session_key = ?")
        .bind(session_key)
        .fetch_optional(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    match row {
        Some(r) => Ok(Some(row_to_task(&r)?)),
        None => Ok(None),
    }
}

// ============================================================================
// Scheduler Logic
// ============================================================================

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use cron::Schedule as CronSchedule;
use std::str::FromStr;

/// Calculate the next run time for a task based on its schedule
pub fn calculate_next_run(schedule: &TaskSchedule) -> Option<i64> {
    let now = Utc::now();

    match schedule {
        TaskSchedule::At { at } => {
            // Parse ISO 8601 datetime string
            if let Ok(dt) = DateTime::parse_from_rfc3339(at) {
                let ts = dt.timestamp_millis();
                if ts > now.timestamp_millis() {
                    return Some(ts);
                }
            }
            // Try parsing without timezone
            if let Ok(naive) = NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%S") {
                let local = Local.from_local_datetime(&naive).single();
                if let Some(dt) = local {
                    let ts = dt.timestamp_millis();
                    if ts > now.timestamp_millis() {
                        return Some(ts);
                    }
                }
            }
            None
        }
        TaskSchedule::Every { every_ms, .. } => {
            Some(now.timestamp_millis() + *every_ms as i64)
        }
        TaskSchedule::Cron { expr, .. } => {
            if let Ok(cron) = CronSchedule::from_str(expr) {
                if let Some(next) = cron.upcoming(Utc).next() {
                    return Some(next.timestamp_millis());
                }
            }
            None
        }
    }
}

/// Calculate delay in milliseconds until next run
pub fn calculate_delay_ms(schedule: &TaskSchedule, last_run_at: Option<i64>) -> Option<u64> {
    let now = now_millis();

    match schedule {
        TaskSchedule::At { at } => {
            // One-time schedule - if already run, don't run again
            if last_run_at.is_some() {
                return None;
            }
            if let Some(next) = calculate_next_run(schedule) {
                if next > now {
                    return Some((next - now) as u64);
                }
            }
            None
        }
        TaskSchedule::Every { every_ms, anchor_ms } => {
            let interval_ms = *every_ms;

            if let Some(last) = last_run_at {
                let next = last + interval_ms as i64;
                if next > now {
                    return Some((next - now) as u64);
                }
                // If we missed the window, run immediately
                return Some(0);
            }
            // First run - use anchor if provided, otherwise run immediately
            if let Some(anchor) = anchor_ms {
                let anchor_i64 = *anchor as i64;
                if anchor_i64 > now {
                    return Some((anchor_i64 - now) as u64);
                }
            }
            Some(0)
        }
        TaskSchedule::Cron { expr, .. } => {
            if let Ok(cron) = CronSchedule::from_str(expr) {
                if let Some(next) = cron.upcoming(Utc).next() {
                    let next_ms = next.timestamp_millis();
                    if next_ms > now {
                        return Some((next_ms - now) as u64);
                    }
                }
            }
            None
        }
    }
}

/// Check if a one-time task has expired (already run or past due)
pub fn is_task_expired(task: &ScheduledTask) -> bool {
    match &task.schedule {
        TaskSchedule::At { .. } => {
            // One-time task is expired if it has run or if the time has passed
            if task.state.last_run_at_ms.is_some() {
                return true;
            }
            calculate_next_run(&task.schedule).is_none()
        }
        _ => false, // Recurring tasks don't expire
    }
}

/// Get the message to send to the AI from the task payload
pub fn get_payload_message(payload: &TaskPayload) -> String {
    match payload {
        TaskPayload::AgentTurn { message, .. } => message.clone(),
        TaskPayload::SystemEvent { text } => text.clone(),
    }
}

/// Get enabled tasks that are due to run
pub async fn db_get_due_tasks(db: &OpenClawDb) -> Result<Vec<ScheduledTask>, String> {
    let now = now_millis();
    let rows = sqlx::query(
        "SELECT * FROM scheduled_tasks WHERE enabled = 1 ORDER BY created_at ASC"
    )
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut due_tasks = Vec::new();
    for row in rows {
        if let Ok(task) = row_to_task(&row) {
            // Check if task is due
            if let Some(next_run) = task.state.next_run_at_ms {
                if next_run <= now && task.state.running_at_ms.is_none() {
                    due_tasks.push(task);
                }
            }
        }
    }
    Ok(due_tasks)
}

/// Mark task as running
pub async fn db_mark_task_running(db: &OpenClawDb, id: &str) -> Result<(), String> {
    let now = now_millis();
    let row = sqlx::query("SELECT state_json FROM scheduled_tasks WHERE id = ?")
        .bind(id)
        .fetch_one(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let state_json: String = row.try_get("state_json").map_err(|e| e.to_string())?;
    let mut state: TaskState = serde_json::from_str(&state_json).unwrap_or_default();
    state.running_at_ms = Some(now);

    let new_state_json = serde_json::to_string(&state).map_err(|e| e.to_string())?;
    sqlx::query("UPDATE scheduled_tasks SET state_json = ?, updated_at = ? WHERE id = ?")
        .bind(&new_state_json)
        .bind(now)
        .bind(id)
        .execute(&db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Mark task run completed and update state
pub async fn db_mark_task_completed(
    db: &OpenClawDb,
    id: &str,
    success: bool,
    error: Option<&str>,
    duration_ms: i64,
) -> Result<ScheduledTask, String> {
    let now = now_millis();

    // Get current task to calculate next run
    let task = db_get_task(db, id).await?.ok_or("Task not found")?;
    let next_run = calculate_next_run(&task.schedule);

    let mut state = task.state.clone();
    state.running_at_ms = None;
    state.last_run_at_ms = Some(now);
    state.last_status = Some(if success { "success".to_string() } else { "error".to_string() });
    state.last_error = error.map(|e| e.to_string());
    state.last_duration_ms = Some(duration_ms);
    state.next_run_at_ms = next_run;

    if success {
        state.consecutive_errors = 0;
    } else {
        state.consecutive_errors += 1;
    }

    let state_json = serde_json::to_string(&state).map_err(|e| e.to_string())?;
    sqlx::query("UPDATE scheduled_tasks SET state_json = ?, updated_at = ? WHERE id = ?")
        .bind(&state_json)
        .bind(now)
        .bind(id)
        .execute(&db.pool)
        .await
        .map_err(|e| e.to_string())?;

    // For one-time tasks that have completed, disable them
    if matches!(task.schedule, TaskSchedule::At { .. }) && success {
        sqlx::query("UPDATE scheduled_tasks SET enabled = 0, updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    }

    db_get_task(db, id).await?.ok_or("Task not found after update".to_string())
}

// ============================================================================
// Background Scheduler
// ============================================================================

use tauri::{AppHandle, Emitter};

/// Start the background scheduler that checks for due tasks
pub fn start_scheduler(app: AppHandle, db: OpenClawDb) {
    log::info!("[scheduler] Starting background task scheduler");

    tauri::async_runtime::spawn(async move {
        let check_interval = std::time::Duration::from_secs(30); // Check every 30 seconds

        loop {
            tokio::time::sleep(check_interval).await;

            // Get due tasks
            match db_get_due_tasks(&db).await {
                Ok(tasks) => {
                    for task in tasks {
                        log::info!("[scheduler] Task '{}' is due, triggering execution", task.name);

                        // Emit event to trigger task execution
                        // The frontend or a command handler will pick this up
                        let _ = app.emit("scheduled_task_due", serde_json::json!({
                            "taskId": task.id,
                            "taskName": task.name
                        }));
                    }
                }
                Err(e) => {
                    log::error!("[scheduler] Failed to check due tasks: {}", e);
                }
            }
        }
    });
}
