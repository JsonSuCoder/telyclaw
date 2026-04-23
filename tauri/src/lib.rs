use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::fs;

use serde_json::json;
use tauri::{Emitter, LogicalPosition, Manager, webview::DownloadEvent};
use sqlx::Row;
use url::Url;
use uuid::Uuid;

use tauri_plugin_autostart::MacosLauncher;

mod deeplink;
use deeplink::Deeplink;

mod tray;
mod window;
use crate::window::{WINDOW_STATES, WindowState};

#[cfg(target_os = "macos")]
mod mac;

#[derive(Debug)]
pub struct AppStateStruct {
  pub notification_count: i32,
  pub is_muted: bool,
}

impl Default for AppStateStruct {
  fn default() -> Self {
    Self {
      notification_count: 0,
      is_muted: false,
    }
  }
}

pub type AppState = Mutex<AppStateStruct>;

pub const TRAFFIC_LIGHT_POSITION_OVERLAY_LEGACY: LogicalPosition<f64> = LogicalPosition::new(12.0, 26.0);
pub const TRAFFIC_LIGHT_POSITION_OVERLAY_26: LogicalPosition<f64> = LogicalPosition::new(12.0, 30.0);
pub const TRAFFIC_LIGHT_POSITION_DEFAULT: LogicalPosition<f64> = LogicalPosition::new(12.0, 12.0);

pub static TRAFFIC_LIGHT_POSITION_OVERLAY: LazyLock<LogicalPosition<f64>> = LazyLock::new(|| {
  if let tauri_plugin_os::Version::Semantic(major, _, _) = tauri_plugin_os::version() {
      if major >= 26 {
          return TRAFFIC_LIGHT_POSITION_OVERLAY_26;
      }
  }
  TRAFFIC_LIGHT_POSITION_OVERLAY_LEGACY
});

pub const WINDOW_WIDTH: f64 = 1088.0;
pub const WINDOW_HEIGHT: f64 = 700.0;
pub const WINDOW_MIN_WIDTH: f64 = 360.0;
pub const WINDOW_MIN_HEIGHT: f64 = 200.0;

pub static LAST_URL: LazyLock<std::sync::Mutex<String>> =
  LazyLock::new(|| std::sync::Mutex::new(BASE_URL.to_string()));

pub const DEFAULT_WINDOW_TITLE: &str = match std::option_env!("APP_TITLE") {
  Some(title) => title,
  None => "Telegram Air",
};

pub const BASE_URL: &str = match std::option_env!("BASE_URL") {
  Some(url) => url,
  None => "tauri://localhost",
};

pub const WITH_UPDATER: &str = match std::option_env!("WITH_UPDATER") {
  Some(str) => str,
  None => "false",
};

pub(crate) fn strip_hash_from_url(url: &str) -> String {
  if let Ok(mut parsed_url) = Url::parse(url) {
    parsed_url.set_fragment(None);
    parsed_url.to_string()
  } else {
    url.to_string()
  }
}

pub(crate) fn save_window_url(app: &tauri::AppHandle, window_label: &str) {
  if let Some(webview_window) = app.get_webview_window(window_label) {
    if let Ok(current_url) = webview_window.url() {
      let url_without_hash = strip_hash_from_url(current_url.as_str());
      if let Ok(mut last_url) = LAST_URL.lock() {
        *last_url = url_without_hash;
      }
    }
  }
}

pub fn run() {
  let app = tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      let active_windows = app.windows();
      if active_windows.len() >= 1 {
        let window = active_windows.values().next().unwrap();
        window.set_focus().unwrap_or_default();
      } else {
        open_new_window(app.clone(), BASE_URL.to_string()).unwrap();
      }
    }))
    .plugin(tauri_plugin_os::init())
    .plugin(tauri_plugin_fs::init())
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_log::Builder::default().build())
    .plugin(tauri_plugin_window_state::Builder::default().build())
    .plugin(tauri_plugin_deep_link::init())
    .plugin(tauri_plugin_process::init())
    .plugin(tauri_plugin_sql::Builder::default().build())
    .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--auto-launched"])))
    .on_window_event(|window, event| match event {
    tauri::WindowEvent::CloseRequested { api, .. } => {
      let active_windows = window.app_handle().windows();

      if active_windows.len() == 1 {
        // Save current URL before hiding the last window
        save_window_url(&window.app_handle(), window.label());

        #[cfg(target_os = "macos")]
        window.app_handle().hide().unwrap_or_default();
        #[cfg(not(target_os = "macos"))]
        window.hide().unwrap_or_default();
        api.prevent_close();
      }
    }
    tauri::WindowEvent::ThemeChanged(_) => {
      #[cfg(target_os = "macos")]
      if let Some(base_window) = window.app_handle().get_window(window.label()) {
        if let Ok(mut states) = WINDOW_STATES.lock() {
          if let Some(state) = states.get_mut(window.label()) {
            let title = if state.is_overlay {
              "".to_string()
            } else {
              state.title.clone()
            };
            let traffic_position = if state.is_overlay {
              *TRAFFIC_LIGHT_POSITION_OVERLAY
            } else {
              TRAFFIC_LIGHT_POSITION_DEFAULT
            };
            mac::update_window_title(base_window.clone(), title, traffic_position);
          }
        }
      }
    }
    tauri::WindowEvent::Destroyed => {
      if let Ok(mut states) = WINDOW_STATES.lock() {
        states.remove(window.label());
      }
    }
    _ => {}
  })
  .setup(|app| {
    // Initialize OpenClaw DB
    let app_handle = app.handle().clone();
    let db = tauri::async_runtime::block_on(OpenClawDb::new(&app_handle))
      .expect("Failed to initialize OpenClaw DB");

    // Start scheduled tasks background scheduler
    let scheduler_db = db.clone();
    let scheduler_app = app.handle().clone();
    crate::openclaw::scheduled_tasks::start_scheduler(scheduler_app, scheduler_db);

    app_handle.manage(OpenClawState { db });

    // Start MCP Bridge Server
    let secret = uuid::Uuid::new_v4().to_string();
    let app_handle_for_mcp = app.handle().clone();
    let port = tauri::async_runtime::block_on(crate::openclaw::mcp_bridge::start_mcp_bridge_server(app_handle_for_mcp, secret))
      .expect("Failed to start MCP Bridge Server");
    log::info!("MCP Bridge Server started on port {}", port);

    // Manage app state
    app.manage(AppState::new(AppStateStruct::default()));

    let _main_window = open_new_window(app.handle().clone(), BASE_URL.to_string())
      .expect("Failed to open main window");

    let deeplink = Deeplink::init();
    if let Err(err) = deeplink.setup(app.handle()) {
      log::error!("Failed to setup deeplink: {:?}", err);
    }

    if WITH_UPDATER == "true" {
      app
        .handle()
        .plugin(tauri_plugin_updater::Builder::new().build())?;
    }

    crate::tray::TrayManager::init(app.handle().clone())?;

    Ok(())
  });

  let app = app.invoke_handler(tauri::generate_handler![
    mark_title_bar_overlay,
    set_notifications_count,
    set_window_title,
    open_new_window_cmd,
    save_current_url,
    set_menu_translations,
    auto_launch_get,
    auto_launch_set,
    prevent_sleep_get,
    prevent_sleep_set,
    store_get,
    store_set,
    store_remove,
    openclaw_engine_get_status,
    openclaw_engine_install,
    openclaw_engine_retry_install,
    openclaw_engine_restart_gateway,
    openclaw_session_policy_get,
    openclaw_session_policy_set,
    cowork_get_config,
    cowork_set_config,
    cowork_list_sessions,
    cowork_get_session,
    cowork_start_session,
    cowork_continue_session,
    cowork_stop_session,
    cowork_delete_session,
    cowork_delete_sessions,
    cowork_list_messages,
    cowork_add_message,
    cowork_update_message,
    agents_list,
    agents_get,
    agents_create,
    mcp_list,
    mcp_create,
    app_get_version,
    app_get_system_locale,
    skills_list,
    skills_set_enabled,
    skills_delete,
    skills_download,
    skills_upgrade,
    skills_confirm_install,
    skills_get_root,
    skills_auto_routing_prompt,
    skills_get_config,
    skills_set_config,
    skills_test_email_connectivity,
    mcp_update,
    mcp_delete,
    mcp_set_enabled,
    mcp_fetch_marketplace,
    mcp_refresh_bridge,
    crate::openclaw::mcp_bridge::resolve_ask_user,
    crate::openclaw::mcp_bridge::resolve_telegram_query,
    agents_update,
    agents_delete,
    agents_presets,
    agents_add_preset,
    api_fetch,
    api_stream,
    api_cancel_stream,
    get_api_config,
    check_api_config,
    save_api_config,
    generate_session_title,
    get_recent_cwds,
    window_minimize,
    window_toggle_maximize,
    window_close,
    window_is_maximized,
    window_show_system_menu,
    shell_open_path,
    shell_show_item_in_folder,
    cowork_set_session_pinned,
    cowork_rename_session,
    cowork_remote_managed,
    cowork_export_result_image,
    cowork_capture_image_chunk,
    cowork_save_result_image,
    cowork_export_session_text,
    cowork_respond_to_permission,
    cowork_list_memory_entries,
    cowork_create_memory_entry,
    cowork_update_memory_entry,
    cowork_delete_memory_entry,
    cowork_get_memory_stats,
    cowork_read_bootstrap_file,
    cowork_write_bootstrap_file,
    dialog_select_directory,
    dialog_select_file,
    dialog_select_files,
    dialog_save_inline_file,
    dialog_read_file_as_data_url,
    app_update_download,
    app_update_cancel_download,
    app_update_install,
    log_get_path,
    log_open_folder,
    log_export_zip,
    im_get_config,
    im_set_config,
    im_sync_config,
    im_start_gateway,
    im_stop_gateway,
    im_test_gateway,
    im_get_status,
    im_get_local_ip,
    im_get_openclaw_config_schema,
    im_weixin_qr_login_start,
    im_weixin_qr_login_wait,
    im_list_pairing_requests,
    im_approve_pairing_code,
    im_reject_pairing_request,
    im_add_qq_instance,
    im_delete_qq_instance,
    im_set_qq_instance_config,
    im_add_feishu_instance,
    im_delete_feishu_instance,
    im_set_feishu_instance_config,
    im_add_dingtalk_instance,
    im_delete_dingtalk_instance,
    im_set_dingtalk_instance_config,
    scheduled_tasks_list,
    scheduled_tasks_get,
    scheduled_tasks_create,
    scheduled_tasks_update,
    scheduled_tasks_delete,
    scheduled_tasks_toggle,
    scheduled_tasks_run_manually,
    scheduled_tasks_stop,
    scheduled_tasks_list_runs,
    scheduled_tasks_count_runs,
    scheduled_tasks_list_all_runs,
    scheduled_tasks_resolve_session,
    scheduled_tasks_list_channels,
    scheduled_tasks_list_channel_conversations,
    permissions_check_calendar,
    permissions_request_calendar,
    auth_login,
    auth_exchange,
    auth_get_user,
    auth_get_quota,
    auth_logout,
    auth_refresh_token,
    auth_get_access_token,
    auth_get_models,
    auth_get_profile_summary,
    enterprise_get_config,
    feishu_install_qrcode,
    feishu_install_poll,
    feishu_install_verify,
    github_copilot_request_device_code,
    github_copilot_poll_for_token,
    github_copilot_cancel_polling,
    github_copilot_sign_out,
    github_copilot_refresh_token,
    telegram_query,
    telegram_query_response
  ]);

  app
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[tauri::command]
#[cfg(target_os = "macos")]
fn mark_title_bar_overlay(window: tauri::WebviewWindow, is_overlay: bool) {
  use crate::mac;

  if let Ok(mut states) = WINDOW_STATES.lock() {
    if let Some(state) = states.get_mut(window.label()) {
      state.is_overlay = is_overlay;
    }
  }

  if is_overlay {
    window
      .set_title_bar_style(tauri::utils::TitleBarStyle::Overlay)
      .unwrap_or_default();

    if let Some(base_window) = window.app_handle().get_window(window.label()) {
      // Empty title keeps original behaviour but triggers the reposition.
      mac::update_window_title(
        base_window.clone(),
        "".to_string(),
        *TRAFFIC_LIGHT_POSITION_OVERLAY,
      );
    }
  } else {
    window
      .set_title_bar_style(tauri::utils::TitleBarStyle::Visible)
      .unwrap_or_default();

    // Determine the title we should restore.
    let mut title_to_set = DEFAULT_WINDOW_TITLE.to_string();
    if let Ok(states) = WINDOW_STATES.lock() {
      if let Some(state) = states.get(window.label()) {
        title_to_set = state.title.clone();
      }
    }

    if let Some(base_window) = window.app_handle().get_window(window.label()) {
      mac::update_window_title(
        base_window.clone(),
        title_to_set,
        TRAFFIC_LIGHT_POSITION_DEFAULT,
      );
    }
  }
}

#[tauri::command]
#[cfg(not(target_os = "macos"))]
#[allow(unused_variables)]
fn mark_title_bar_overlay(window: tauri::WebviewWindow, is_overlay: bool) {
  // noop
}

#[tauri::command]
fn set_notifications_count(
  window: tauri::WebviewWindow,
  amount: i32,
  is_muted: bool,
  state: tauri::State<'_, AppState>,
) {
  // Update app state
  if let Ok(mut app_state) = state.lock() {
    app_state.notification_count = amount;
    app_state.is_muted = is_muted;
  }

  crate::tray::set_notifications_count(&window, amount, is_muted);
}

#[tauri::command]
fn set_menu_translations(translations: HashMap<String, String>) {
  crate::tray::set_menu_translations(translations);
}

#[tauri::command]
fn set_window_title(window: tauri::WebviewWindow, title: String) {
  if let Ok(mut states) = WINDOW_STATES.lock() {
    if let Some(state) = states.get_mut(window.label()) {
      state.title = title.clone();
      if !state.is_overlay {
        window.set_title(&title).unwrap_or_default();
      }
    }
  }
}

#[tauri::command]
async fn open_new_window_cmd(app: tauri::AppHandle, url: String) -> bool {
  open_new_window(app, url).is_ok()
}

#[tauri::command]
fn save_current_url(window: tauri::WebviewWindow) {
  if let Ok(current_url) = window.url() {
    let url_without_hash = strip_hash_from_url(current_url.as_str());
    if let Ok(mut last_url) = LAST_URL.lock() {
      *last_url = url_without_hash;
    }
  }
}

#[derive(serde::Serialize)]
struct AutoLaunchStatus {
  enabled: bool,
}

#[tauri::command]
fn auto_launch_get() -> AutoLaunchStatus {
  AutoLaunchStatus { enabled: false }
}

#[tauri::command]
fn auto_launch_set(enabled: bool) -> Result<(), String> {
  println!("auto_launch_set: {}", enabled);
  Ok(())
}

#[tauri::command]
fn prevent_sleep_get() -> AutoLaunchStatus {
  // TODO: implement real prevent sleep status
  AutoLaunchStatus { enabled: false }
}

#[tauri::command]
fn prevent_sleep_set(enabled: bool) -> Result<(), String> {
  // TODO: implement real prevent sleep
  println!("prevent_sleep_set: {}", enabled);
  Ok(())
}

#[tauri::command]
fn store_get(app: tauri::AppHandle, key: String) -> Result<serde_json::Value, String> {
  let path = app.path().app_data_dir().map_err(|e| e.to_string())?.join("store.json");
  if !path.exists() {
    return Ok(serde_json::Value::Null);
  }
  let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
  let store: HashMap<String, serde_json::Value> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
  Ok(store.get(&key).cloned().unwrap_or(serde_json::Value::Null))
}

#[tauri::command]
fn store_set(app: tauri::AppHandle, key: String, value: serde_json::Value) -> Result<(), String> {
  let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
  if !data_dir.exists() {
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
  }
  let path = data_dir.join("store.json");
  let mut store: HashMap<String, serde_json::Value> = if path.exists() {
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).unwrap_or_default()
  } else {
    HashMap::new()
  };
  store.insert(key, value);
  let content = serde_json::to_string(&store).map_err(|e| e.to_string())?;
  fs::write(path, content).map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
fn store_remove(app: tauri::AppHandle, key: String) -> Result<(), String> {
  let path = app.path().app_data_dir().map_err(|e| e.to_string())?;
  let path = path.join("store.json");
  if !path.exists() {
    return Ok(());
  }
  let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
  let mut store: HashMap<String, serde_json::Value> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
  store.remove(&key);
  let content = serde_json::to_string(&store).map_err(|e| e.to_string())?;
  fs::write(path, content).map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
fn openclaw_engine_install() -> Result<(), String> {
  Ok(())
}

#[tauri::command]
fn openclaw_engine_retry_install() -> Result<(), String> {
  Ok(())
}

#[tauri::command]
fn openclaw_engine_restart_gateway() -> Result<(), String> {
  Ok(())
}

#[tauri::command]
fn openclaw_session_policy_get() -> serde_json::Value {
  json!({ "keepAlive": "30d" })
}

#[tauri::command]
fn openclaw_session_policy_set(config: serde_json::Value) -> Result<(), String> {
  println!("openclaw_session_policy_set: {:?}", config);
  Ok(())
}

#[tauri::command]
async fn cowork_get_config(app: tauri::AppHandle, state: tauri::State<'_, OpenClawState>) -> Result<serde_json::Value, String> {
  let row = sqlx::query("SELECT value FROM kv WHERE key = ?")
    .bind("cowork.config")
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

  let mut config = row
    .and_then(|r| r.try_get::<String, _>("value").ok())
    .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    .unwrap_or_else(|| json!({}));

  let default_workdir = app
    .path()
    .app_data_dir()
    .map_err(|e| e.to_string())?
    .join("cowork-workspace");
  if !default_workdir.exists() {
    std::fs::create_dir_all(&default_workdir).map_err(|e| e.to_string())?;
  }

  if !config.get("workingDirectory").and_then(|v| v.as_str()).map(|v| !v.trim().is_empty()).unwrap_or(false) {
    if let serde_json::Value::Object(map) = &mut config {
      map.insert("workingDirectory".to_string(), json!(default_workdir.to_string_lossy().to_string()));
    }
  }

  if let serde_json::Value::Object(map) = &mut config {
    map.entry("systemPrompt".to_string()).or_insert_with(|| json!(""));
    map.entry("executionMode".to_string()).or_insert_with(|| json!("local"));
    map.entry("agentEngine".to_string()).or_insert_with(|| json!("openclaw"));
    map.entry("memoryEnabled".to_string()).or_insert_with(|| json!(true));
    map.entry("memoryImplicitUpdateEnabled".to_string()).or_insert_with(|| json!(true));
    map.entry("memoryLlmJudgeEnabled".to_string()).or_insert_with(|| json!(false));
    map.entry("memoryGuardLevel".to_string()).or_insert_with(|| json!("standard"));
    map.entry("memoryUserMemoriesMaxItems".to_string()).or_insert_with(|| json!(200));
    map.entry("skipMissedJobs".to_string()).or_insert_with(|| json!(true));
    map.entry("openClawSessionPolicy".to_string()).or_insert_with(|| json!({ "keepAlive": "30d" }));
  }

  Ok(json!({ "success": true, "config": config }))
}

#[tauri::command]
async fn cowork_set_config(state: tauri::State<'_, OpenClawState>, config: serde_json::Value) -> Result<serde_json::Value, String> {
  let now = chrono::Utc::now().timestamp_millis();
  sqlx::query(
    "INSERT INTO kv (key, value, updated_at) VALUES (?, ?, ?)
     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
  )
  .bind("cowork.config")
  .bind(config.to_string())
  .bind(now)
  .execute(&state.db.pool)
  .await
  .map_err(|e| e.to_string())?;

  Ok(json!({ "success": true }))
}

#[tauri::command]
async fn cowork_continue_session(
  app: tauri::AppHandle,
  state: tauri::State<'_, OpenClawState>,
  sessionId: String,
  prompt: String,
  systemPrompt: Option<String>,
  activeSkillIds: Option<Vec<String>>,
  imageAttachments: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
  if sessionId.trim().is_empty() {
    return Ok(json!({ "success": false, "error": "Missing sessionId" }));
  }
  if prompt.trim().is_empty() {
    return Ok(json!({ "success": false, "error": "Missing prompt" }));
  }

  let now = chrono::Utc::now().timestamp_millis();

  sqlx::query("UPDATE cowork_sessions SET status = ?, updated_at = ? WHERE id = ?")
    .bind("running")
    .bind(now)
    .bind(&sessionId)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

  let next_seq = sqlx::query("SELECT COALESCE(MAX(sequence), 0) AS max_seq FROM cowork_messages WHERE session_id = ?")
    .bind(&sessionId)
    .fetch_one(&state.db.pool)
    .await
    .ok()
    .and_then(|r| r.try_get::<i64, _>("max_seq").ok())
    .unwrap_or(0) + 1;

  let user_message_id = uuid::Uuid::new_v4().to_string();
  let mut meta = serde_json::Map::new();
  let active_skill_ids_val = json!(activeSkillIds.clone().unwrap_or_default());
  meta.insert("skillIds".to_string(), active_skill_ids_val.clone());
  if let Some(attachments) = imageAttachments.clone() {
    meta.insert("imageAttachments".to_string(), attachments);
  }
  let meta_str = serde_json::Value::Object(meta).to_string();

  sqlx::query(
    "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
     VALUES (?, ?, ?, ?, ?, ?, ?)",
  )
  .bind(&user_message_id)
  .bind(&sessionId)
  .bind("user")
  .bind(&prompt)
  .bind(meta_str)
  .bind(now)
  .bind(next_seq)
  .execute(&state.db.pool)
  .await
  .map_err(|e| e.to_string())?;

  let _ = app.emit("cowork_stream_message", json!({
    "sessionId": sessionId,
    "message": {
      "id": user_message_id,
      "type": "user",
      "content": prompt,
      "timestamp": now,
      "sequence": next_seq,
      "metadata": {
        "skillIds": active_skill_ids_val,
        "imageAttachments": imageAttachments
      }
    }
  }));

  let cfg = {
    let path = app.path().app_data_dir().map_err(|e| e.to_string())?.join("store.json");
    let content = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".to_string());
    let store: HashMap<String, serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    let app_config = store.get("app_config").cloned().unwrap_or_else(|| json!({}));
    let api_key = app_config.pointer("/api/key").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let base_url = app_config.pointer("/api/baseUrl").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let model = app_config.pointer("/model/defaultModel").and_then(|v| v.as_str()).unwrap_or("").to_string();
    (api_key, base_url, model)
  };

  let (api_key, base_url, model) = cfg;
  let pool = state.db.pool.clone();
  let app_clone = app.clone();
  let session_id_clone = sessionId.clone();
  let system_prompt = systemPrompt.unwrap_or_default();

  tauri::async_runtime::spawn(async move {
    let assistant_message_id = uuid::Uuid::new_v4().to_string();
    let ts = chrono::Utc::now().timestamp_millis();
    let seq = next_seq + 1;

    let _ = sqlx::query(
      "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
       VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&assistant_message_id)
    .bind(&session_id_clone)
    .bind("assistant")
    .bind("")
    .bind(Option::<String>::None)
    .bind(ts)
    .bind(seq)
    .execute(&pool)
    .await;

    let _ = app_clone.emit("cowork_stream_message", json!({
      "sessionId": session_id_clone,
      "message": { "id": assistant_message_id, "type": "assistant", "content": "", "timestamp": ts, "sequence": seq, "metadata": { "isStreaming": true } }
    }));

    let (reply, total_tool_seq_offset) = if api_key.trim().is_empty() || base_url.trim().is_empty() || model.trim().is_empty() {
      ("API config missing. Please open Settings -> Model and set your API key.".to_string(), 0i64)
    } else {
      let client = reqwest::Client::new();
      let base = base_url.trim_end_matches('/').to_string();
      let url = format!("{}/v1/messages", base);
      // Dynamically fetch tool definitions from frontend
      let tool_defs_result = crate::openclaw::telegram_query(
        app_clone.clone(),
        "getToolDefinitions".to_string(),
        json!({}),
      ).await;

      let tools = match tool_defs_result {
        Ok(v) => {
          println!("[DEBUG] tool_defs_result: {:?}", v);
          // Extract the data array from { success: true, data: [...] }
          if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
            // Convert to Claude API format (remove handler field, keep name/description/input_schema)
            let claude_tools: Vec<serde_json::Value> = data.iter().map(|t| {
              json!({
                "name": t.get("name"),
                "description": t.get("description"),
                "input_schema": t.get("input_schema")
              })
            }).collect();
            println!("[DEBUG] claude_tools count: {}", claude_tools.len());
            json!(claude_tools)
          } else {
            println!("[DEBUG] No data array found in tool_defs_result");
            json!([])
          }
        }
        Err(e) => {
          println!("[DEBUG] tool_defs_result error: {}", e);
          json!([])
        }
      };
      println!("[DEBUG] Final tools: {:?}", tools);

      let system_for_request = if system_prompt.trim().is_empty() {
        openclaw::default_system_prompt::DEFAULT_SYSTEM_PROMPT.to_string()
      } else {
        system_prompt.clone()
      };

      let mut messages: Vec<serde_json::Value> = vec![json!({ "role": "user", "content": prompt })];

      let mut steps = 0;
      let mut final_text = String::new();
      let mut total_tool_seq_offset: i64 = 0;
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
          Err(e) => format!("Error: {}", e),
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

        for (tool_use_id, tool_name, tool_input) in tool_uses {
          let tool_msg_id = uuid::Uuid::new_v4().to_string();
          let tool_ts = chrono::Utc::now().timestamp_millis();
          let tool_name_meta = tool_name.clone();
          let tool_input_meta = tool_input.clone();

          let _ = sqlx::query(
            "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
          )
          .bind(&tool_msg_id)
          .bind(&session_id_clone)
          .bind("tool_use")
          .bind("")
          .bind(Some(json!({
            "toolName": tool_name_meta,
            "toolInput": tool_input_meta,
            "toolUseId": tool_use_id,
          }).to_string()))
          .bind(tool_ts)
          .bind(seq + 10 + total_tool_seq_offset)
          .execute(&pool)
          .await;
          let tool_use_seq = seq + 10 + total_tool_seq_offset;
          total_tool_seq_offset += 1;
          let _ = app_clone.emit("cowork_stream_message", json!({
            "sessionId": session_id_clone,
            "message": {
              "id": tool_msg_id,
              "type": "tool_use",
              "content": "",
              "timestamp": tool_ts,
              "sequence": tool_use_seq,
              "metadata": {
                "toolName": &tool_name,
                "toolInput": &tool_input,
                "toolUseId": tool_use_id,
              }
            }
          }));

          // Execute tool via frontend's unified executeTool
          let (tool_ok, tool_result_value) = {
            let result = crate::openclaw::telegram_query(
              app_clone.clone(),
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
          .bind(&session_id_clone)
          .bind("tool_result")
          .bind(tool_result_text.clone())
          .bind(Some(json!({
            "toolUseId": tool_use_id,
            "toolName": &tool_name,
            "toolResult": tool_result_text,
            "isError": !tool_ok
          }).to_string()))
          .bind(tool_result_ts)
          .bind(seq + 10 + total_tool_seq_offset)
          .execute(&pool)
          .await;
          let tool_result_seq = seq + 10 + total_tool_seq_offset;
          total_tool_seq_offset += 1;

          let _ = app_clone.emit("cowork_stream_message", json!({
            "sessionId": session_id_clone,
            "message": {
              "id": tool_result_msg_id,
              "type": "tool_result",
              "content": tool_result_text,
              "timestamp": tool_result_ts,
              "sequence": tool_result_seq,
              "metadata": {
                "toolUseId": tool_use_id,
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
        // Continue the loop to send tool results back to Claude
        continue;
      }

      if final_text.trim().is_empty() {
        ("Empty response".to_string(), total_tool_seq_offset)
      } else {
        (final_text, total_tool_seq_offset)
      }
    };

    // If tools were used, create a new assistant message with correct sequence
    // Otherwise, update the original assistant message
    if total_tool_seq_offset > 0 {
      let final_msg_id = uuid::Uuid::new_v4().to_string();
      let final_ts = chrono::Utc::now().timestamp_millis();
      let final_seq = seq + 10 + total_tool_seq_offset;

      let _ = sqlx::query(
        "INSERT INTO cowork_messages (id, session_id, type, content, metadata, created_at, sequence)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
      )
      .bind(&final_msg_id)
      .bind(&session_id_clone)
      .bind("assistant")
      .bind(&reply)
      .bind(None::<String>)
      .bind(final_ts)
      .bind(final_seq)
      .execute(&pool)
      .await;

      let _ = app_clone.emit("cowork_stream_message", json!({
        "sessionId": session_id_clone,
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
        "sessionId": session_id_clone,
        "messageId": assistant_message_id,
        "content": reply
      }));
    }

    let _ = sqlx::query("UPDATE cowork_sessions SET status = ?, updated_at = ? WHERE id = ?")
      .bind("completed")
      .bind(chrono::Utc::now().timestamp_millis())
      .bind(&session_id_clone)
      .execute(&pool)
      .await;

    let _ = app_clone.emit("cowork_stream_complete", json!({ "sessionId": session_id_clone }));
    let _ = app_clone.emit("cowork_sessions_changed", json!({}));
  });

  Ok(json!({ "success": true }))
}

#[tauri::command]
async fn cowork_stop_session(
  state: tauri::State<'_, OpenClawState>,
  sessionId: String,
) -> Result<serde_json::Value, String> {
  if sessionId.trim().is_empty() {
    return Ok(json!({ "success": false, "error": "Missing sessionId" }));
  }
  sqlx::query("UPDATE cowork_sessions SET status = ?, updated_at = ? WHERE id = ?")
    .bind("idle")
    .bind(chrono::Utc::now().timestamp_millis())
    .bind(&sessionId)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;
  Ok(json!({ "success": true }))
}

#[tauri::command]
async fn cowork_delete_sessions(
  state: tauri::State<'_, OpenClawState>,
  sessionIds: Vec<String>,
) -> Result<serde_json::Value, String> {
  if sessionIds.is_empty() {
    return Ok(json!({ "success": true }));
  }
  for id in &sessionIds {
    let _ = sqlx::query("DELETE FROM cowork_messages WHERE session_id = ?")
      .bind(id)
      .execute(&state.db.pool)
      .await;
    let _ = sqlx::query("DELETE FROM cowork_sessions WHERE id = ?")
      .bind(id)
      .execute(&state.db.pool)
      .await;
  }
  Ok(json!({ "success": true }))
}

#[tauri::command]
fn app_get_version(app: tauri::AppHandle) -> String {
  app.package_info().version.to_string()
}

#[tauri::command]
fn app_get_system_locale() -> String {
  "zh-CN".to_string()
}

mod openclaw;
use openclaw::{
    db::OpenClawDb, OpenClawState,
    openclaw_engine_get_status,
    cowork_list_sessions,
    cowork_get_session,
    cowork_start_session,
    cowork_delete_session,
    cowork_list_messages,
    cowork_add_message,
    cowork_update_message,
    agents_list,
    agents_get,
    agents_create,
    mcp_list,
    mcp_create,
    telegram_query,
    telegram_query_response,
    // scheduled_tasks
    scheduled_tasks_list,
    scheduled_tasks_get,
    scheduled_tasks_create,
    scheduled_tasks_update,
    scheduled_tasks_delete,
    scheduled_tasks_toggle,
    scheduled_tasks_run_manually,
    scheduled_tasks_stop,
    scheduled_tasks_list_runs,
    scheduled_tasks_count_runs,
    scheduled_tasks_list_all_runs,
    scheduled_tasks_resolve_session,
    scheduled_tasks_list_channels,
    scheduled_tasks_list_channel_conversations,
};

pub(crate) fn open_new_window(
  app: tauri::AppHandle,
  url: String,
) -> Result<tauri::WebviewWindow, tauri::Error> {
  let window_label = Uuid::new_v4().to_string();
  let new_window_builder = tauri::WebviewWindowBuilder::new(
    &app,
    window_label.clone(),
    tauri::WebviewUrl::App(url.into()),
  )
  .additional_browser_args("--autoplay-policy=no-user-gesture-required")
  .fullscreen(false)
  .resizable(true)
  .title(DEFAULT_WINDOW_TITLE)
  .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
  .min_inner_size(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)
  .disable_drag_drop_handler() // Required for Drag & Drop on Windows
  .initialization_script(&format!(
    "window.tauri = {{ version: '{}' }};",
    env!("CARGO_PKG_VERSION")
  ))
  .on_download(|window, event| {
    match event {
      #[allow(unused_variables)]
      DownloadEvent::Requested { destination, .. } => {
        // On macOS, Webview does not provide basic download logic
        #[cfg(target_os = "macos")]
        if let Some(filename) = destination.file_name() {
          if let Ok(downloads_dir) = window.app_handle().path().download_dir() {
            let new_destination = downloads_dir.join(filename);
            *destination = new_destination;
          }
        }
      }
      DownloadEvent::Finished { url, success, .. } => {
        window
          .emit_to(
            window.label(),
            "download-finished",
            json!({
              "url": url.to_string(),
              "success": success
            }),
          )
          .unwrap_or_default();
      }
      _ => {}
    };
    true
  });

  if let Ok(mut states) = WINDOW_STATES.lock() {
    let new_state = WindowState {
      title: DEFAULT_WINDOW_TITLE.to_string(),
      is_overlay: cfg!(target_os = "macos"),
    };
    states.insert(window_label.to_string(), new_state);
  }

  #[cfg(target_os = "macos")]
  let new_window_builder = new_window_builder.title_bar_style(tauri::TitleBarStyle::Overlay);
  #[cfg(target_os = "macos")]
  let new_window_builder = new_window_builder.title("");

  let window = new_window_builder.build()?;

  #[cfg(target_os = "macos")]
  if let Some(base_window) = app.get_window(&window_label) {
    mac::setup_traffic_light_positioner(&base_window, *TRAFFIC_LIGHT_POSITION_OVERLAY);
  }

  // Apply stored notification count to the new window
  if let Some(state) = app.try_state::<AppState>() {
    if let Ok(app_state) = state.lock() {
      crate::tray::set_notifications_count(
        &window,
        app_state.notification_count,
        app_state.is_muted,
      );
    }
  }

  Ok(window)
}

mod tauri_compat_stubs;
use tauri_compat_stubs::*;
