use crate::openclaw::OpenClawState;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use futures_util::StreamExt;
use mime_guess::MimeGuess;
use reqwest::header::HeaderMap;
use reqwest::Method;
use serde_json::{json, Value};
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

static API_STREAM_CANCELLATIONS: LazyLock<Mutex<HashMap<String, CancellationToken>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

async fn kv_get(pool: &Pool<Sqlite>, key: &str) -> Result<Option<String>, String> {
    let row = sqlx::query("SELECT value FROM kv WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(row.map(|r| r.get::<String, _>("value")))
}

async fn kv_set(pool: &Pool<Sqlite>, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO kv (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(now_ms())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn kv_delete_prefix(pool: &Pool<Sqlite>, prefix: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM kv WHERE key LIKE ?")
        .bind(format!("{}%", prefix))
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn parse_frontmatter(markdown: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut lines = markdown.lines();
    if lines.next() != Some("---") {
        return result;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            result.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    result
}

fn resolve_skills_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(val) = std::env::var("LOBSTERAI_SKILLS_ROOT") {
        if !val.trim().is_empty() {
            roots.push(PathBuf::from(val));
        }
    }
    if let Ok(val) = std::env::var("SKILLS_ROOT") {
        if !val.trim().is_empty() {
            roots.push(PathBuf::from(val));
        }
    }
    if let Ok(dir) = app.path().app_data_dir() {
        roots.push(dir.join("SKILLs"));
    }
    if let Ok(dir) = app.path().resource_dir() {
        roots.push(dir.join("SKILLs"));
    }
    roots.push(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("src/openclaw/SKILLs"));
    roots
}

fn pick_existing_skills_root(roots: &[PathBuf]) -> Option<PathBuf> {
    for root in roots {
        if !root.exists() || !root.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() && p.join("SKILL.md").exists() {
                    return Some(root.clone());
                }
            }
        }
    }
    None
}

async fn build_skills_list(app: &AppHandle, state: &OpenClawState) -> Result<Vec<Value>, String> {
    let roots = resolve_skills_roots(app);
    let root = pick_existing_skills_root(&roots).ok_or_else(|| "SKILLs root not found".to_string())?;
    let resource_root = app.path().resource_dir().ok().map(|d| d.join("SKILLs"));

    let mut skills = Vec::new();
    let entries = std::fs::read_dir(&root).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let skill_md = dir.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&skill_md).map_err(|e| e.to_string())?;
        let fm = parse_frontmatter(&content);

        let id = fm.get("name").cloned().unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().to_string());
        let name = fm.get("name").cloned().unwrap_or_else(|| id.clone());
        let description = fm.get("description").cloned().unwrap_or_default();
        let version = fm.get("version").cloned();
        let is_official = fm.get("official").map(|v| v == "true" || v == "1").unwrap_or(false);
        let is_built_in = resource_root
            .as_ref()
            .and_then(|r| skill_md.canonicalize().ok().and_then(|p| r.canonicalize().ok().map(|rr| p.starts_with(rr))))
            .unwrap_or(false);

        let enabled_key = format!("skills.enabled.{}", id);
        let enabled = match kv_get(&state.db.pool, &enabled_key).await? {
            Some(v) => v == "1" || v.eq_ignore_ascii_case("true"),
            None => true,
        };

        let updated_at = std::fs::metadata(&skill_md)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(now_ms);

        skills.push(json!({
            "id": id,
            "name": name,
            "description": description,
            "enabled": enabled,
            "isOfficial": is_official,
            "isBuiltIn": is_built_in,
            "updatedAt": updated_at,
            "prompt": content,
            "skillPath": skill_md.to_string_lossy(),
            "version": version
        }));
    }

    skills.sort_by(|a, b| {
        let an = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let bn = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        an.to_lowercase().cmp(&bn.to_lowercase())
    });
    Ok(skills)
}

#[tauri::command]
pub async fn skills_list(app: AppHandle, state: State<'_, OpenClawState>) -> Result<Value, String> {
    match build_skills_list(&app, &state).await {
        Ok(skills) => Ok(json!({ "success": true, "skills": skills })),
        Err(e) => Ok(json!({ "success": false, "error": e })),
    }
}

#[tauri::command]
pub async fn skills_set_enabled(
    app: AppHandle,
    state: State<'_, OpenClawState>,
    options: Value,
) -> Result<Value, String> {
    let id = options.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let enabled = options.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    if id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing id" }));
    }
    let key = format!("skills.enabled.{}", id);
    if let Err(e) = kv_set(&state.db.pool, &key, if enabled { "1" } else { "0" }).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    skills_list(app, state).await
}

#[tauri::command]
pub async fn skills_delete(
    app: AppHandle,
    state: State<'_, OpenClawState>,
    id: Value,
) -> Result<Value, String> {
    let id_str = id.as_str().unwrap_or("").to_string();
    if id_str.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing id" }));
    }

    let roots = resolve_skills_roots(&app);
    let root = match pick_existing_skills_root(&roots) {
        Some(r) => r,
        None => return Ok(json!({ "success": false, "error": "SKILLs root not found" })),
    };

    let resource_root = app.path().resource_dir().ok().map(|d| d.join("SKILLs")).and_then(|r| r.canonicalize().ok());
    let mut target_dir: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let skill_md = dir.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                let fm = parse_frontmatter(&content);
                let name = fm.get("name").cloned().unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().to_string());
                if name == id_str {
                    target_dir = Some(dir);
                    break;
                }
            }
        }
    }

    let target_dir = match target_dir {
        Some(d) => d,
        None => return Ok(json!({ "success": false, "error": "Skill not found" })),
    };

    if let Some(rr) = resource_root {
        if let Ok(td) = target_dir.canonicalize() {
            if td.starts_with(rr) {
                return Ok(json!({ "success": false, "error": "Built-in skills cannot be deleted" }));
            }
        }
    }

    if let Err(e) = std::fs::remove_dir_all(&target_dir) {
        return Ok(json!({ "success": false, "error": e.to_string() }));
    }
    let _ = kv_delete_prefix(&state.db.pool, &format!("skills.enabled.{}", id_str)).await;
    let _ = kv_delete_prefix(&state.db.pool, &format!("skills.config.{}", id_str)).await;
    skills_list(app, state).await
}

#[tauri::command]
pub fn skills_download(source: serde_json::Value) -> serde_json::Value {
    let _ = source;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn skills_upgrade(skillId: serde_json::Value, downloadUrl: serde_json::Value) -> serde_json::Value {
    let _ = skillId;
    let _ = downloadUrl;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn skills_confirm_install(pendingId: serde_json::Value, action: serde_json::Value) -> serde_json::Value {
    let _ = pendingId;
    let _ = action;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub async fn skills_get_root(app: AppHandle) -> Result<Value, String> {
    let roots = resolve_skills_roots(&app);
    match pick_existing_skills_root(&roots) {
        Some(root) => Ok(json!({ "success": true, "path": root.to_string_lossy() })),
        None => Ok(json!({ "success": false, "error": "SKILLs root not found" })),
    }
}

#[tauri::command]
pub async fn skills_auto_routing_prompt(
    app: AppHandle,
    state: State<'_, OpenClawState>,
) -> Result<Value, String> {
    match build_skills_list(&app, &state).await {
        Ok(skills) => {
            let mut lines = Vec::new();
            for s in skills {
                let enabled = s.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                if !enabled {
                    continue;
                }
                let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let desc = s.get("description").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    continue;
                }
                lines.push(format!("- {}: {}", id, desc));
            }
            Ok(json!({ "success": true, "prompt": format!("Available skills:\n{}", lines.join("\n")) }))
        }
        Err(e) => Ok(json!({ "success": false, "error": e })),
    }
}

#[tauri::command]
pub async fn skills_get_config(state: State<'_, OpenClawState>, skillId: Value) -> Result<Value, String> {
    let id = skillId.as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing skillId" }));
    }
    let key = format!("skills.config.{}", id);
    match kv_get(&state.db.pool, &key).await {
        Ok(Some(raw)) => {
            let config = serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({}));
            Ok(json!({ "success": true, "config": config }))
        }
        Ok(None) => Ok(json!({ "success": true, "config": json!({}) })),
        Err(e) => Ok(json!({ "success": false, "error": e })),
    }
}

#[tauri::command]
pub async fn skills_set_config(
    state: State<'_, OpenClawState>,
    skillId: Value,
    config: Value,
) -> Result<Value, String> {
    let id = skillId.as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing skillId" }));
    }
    let key = format!("skills.config.{}", id);
    match kv_set(&state.db.pool, &key, &config.to_string()).await {
        Ok(()) => Ok(json!({ "success": true })),
        Err(e) => Ok(json!({ "success": false, "error": e })),
    }
}

#[tauri::command]
pub fn skills_test_email_connectivity(skillId: serde_json::Value, config: serde_json::Value) -> serde_json::Value {
    let _ = skillId;
    let _ = config;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub async fn mcp_update(state: State<'_, OpenClawState>, id: String, data: Value) -> Result<Value, String> {
    let name = data.get("name").and_then(|v| v.as_str());
    let description = data.get("description").and_then(|v| v.as_str());
    let transport_type = data.get("transportType").and_then(|v| v.as_str());
    let config_json = data.get("configJson").map(|v| v.to_string());

    let now = now_ms();
    sqlx::query(
        "UPDATE mcp_servers
         SET name = COALESCE(?, name),
             description = COALESCE(?, description),
             transport_type = COALESCE(?, transport_type),
             config_json = COALESCE(?, config_json),
             updated_at = ?
         WHERE id = ?",
    )
    .bind(name)
    .bind(description)
    .bind(transport_type)
    .bind(config_json)
    .bind(now)
    .bind(&id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    // Return same shape as mcp_list: { success, servers }
    let rows = sqlx::query("SELECT * FROM mcp_servers ORDER BY name ASC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut servers = Vec::new();
    for row in rows {
        servers.push(json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "description": row.get::<String, _>("description"),
            "enabled": row.get::<i32, _>("enabled") != 0,
            "transportType": row.get::<String, _>("transport_type"),
            "configJson": serde_json::from_str::<Value>(&row.get::<String, _>("config_json")).unwrap_or(json!({})),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }));
    }
    Ok(json!({ "success": true, "servers": servers }))
}

#[tauri::command]
pub async fn mcp_delete(state: State<'_, OpenClawState>, id: String) -> Result<Value, String> {
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
        .bind(&id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let rows = sqlx::query("SELECT * FROM mcp_servers ORDER BY name ASC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut servers = Vec::new();
    for row in rows {
        servers.push(json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "description": row.get::<String, _>("description"),
            "enabled": row.get::<i32, _>("enabled") != 0,
            "transportType": row.get::<String, _>("transport_type"),
            "configJson": serde_json::from_str::<Value>(&row.get::<String, _>("config_json")).unwrap_or(json!({})),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }));
    }
    Ok(json!({ "success": true, "servers": servers }))
}

#[tauri::command]
pub async fn mcp_set_enabled(state: State<'_, OpenClawState>, options: Value) -> Result<Value, String> {
    let id = options.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let enabled = options.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    if id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing id" }));
    }

    sqlx::query("UPDATE mcp_servers SET enabled = ?, updated_at = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(now_ms())
        .bind(&id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let rows = sqlx::query("SELECT * FROM mcp_servers ORDER BY name ASC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut servers = Vec::new();
    for row in rows {
        servers.push(json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "description": row.get::<String, _>("description"),
            "enabled": row.get::<i32, _>("enabled") != 0,
            "transportType": row.get::<String, _>("transport_type"),
            "configJson": serde_json::from_str::<Value>(&row.get::<String, _>("config_json")).unwrap_or(json!({})),
            "createdAt": row.get::<i64, _>("created_at"),
            "updatedAt": row.get::<i64, _>("updated_at")
        }));
    }
    Ok(json!({ "success": true, "servers": servers }))
}

#[tauri::command]
pub fn mcp_fetch_marketplace() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn mcp_refresh_bridge() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

fn preset_agents() -> Vec<Value> {
    vec![
        json!({
            "id": "stockexpert",
            "name": "股票助手",
            "nameEn": "Stock Expert",
            "icon": "📈",
            "description": "A 股公告追踪、个股深度分析、交易复盘；支持美港股行情、基本面、技术指标与风险评估。",
            "descriptionEn": "A-share announcements, in-depth stock analysis, and trade review; supports US/HK quotes, fundamentals, technicals, and risk assessment.",
            "systemPrompt": r#"你是一名专业的股票分析助手（Stock Expert），专注A股市场的激进型分析师。

## 核心能力
1. **综合深度分析** — 使用 stock-analyzer skill 的 `analyze.py`，生成价值+技术+成长+财务多维评分报告
2. **A股公告监控** — 使用 stock-announcements skill 的 `announcements.py`，从东方财富获取实时公告
3. **快速行情查询** — 使用 stock-explorer skill 的 `quote.py`，获取实时报价和技术指标
4. **网络搜索补充** — 使用 web-search skill，搜索最新市场新闻和分析

## 工作原则
- 始终提供数据驱动、客观的分析
- 用户提到股票名称时，先确认代码（上交所 .SS，深交所 .SZ）
- 优先使用专业 skill 获取真实数据，web-search 作为补充
- 明确标注数据时效性，当信息可能过时时请说明
- A股分析占80%以上，美港股仅做参考对比

## 系统环境注意事项
- Windows 环境：在 bash 中运行 Python 脚本前设置 `export PYTHONIOENCODING=utf-8`
- 所有 Python 脚本输出纯文本报告，不生成 PNG 图表
- 使用 `pip` 安装依赖，不使用 `uv`
"#,
            "systemPromptEn": r#"You are a professional stock analysis assistant (Stock Expert), an aggressive analyst focused on the A-share market.

## Core Capabilities
1. **Comprehensive Analysis** — Use the stock-analyzer skill's `analyze.py` to generate multi-dimensional reports (value + technical + growth + financial)
2. **A-share Announcements** — Use the stock-announcements skill's `announcements.py` to fetch real-time filings from Eastmoney
3. **Quick Quotes** — Use the stock-explorer skill's `quote.py` for real-time quotes and technical indicators
4. **Web Search** — Use the web-search skill for the latest market news and analysis

## Principles
- Always provide data-driven, objective analysis
- When a stock name is mentioned, confirm the ticker first (SSE: .SS, SZSE: .SZ)
- Prefer professional skills for real data; use web-search as a supplement
- Clearly note data freshness; state when information may be outdated
- A-share analysis accounts for 80%+; US/HK stocks are for reference only

## System Notes
- Windows: set `export PYTHONIOENCODING=utf-8` before running Python scripts in bash
- All Python scripts output plain-text reports, no PNG charts
- Use `pip` to install dependencies, not `uv`
"#,
            "skillIds": ["stock-analyzer", "stock-announcements", "stock-explorer", "web-search"]
        }),
        json!({
            "id": "content-writer",
            "name": "内容创作",
            "nameEn": "Content Writer",
            "icon": "✍️",
            "description": "一站式内容创作：选题、撰写、排版、润色，适用于文章、营销文案和社交媒体帖子。",
            "descriptionEn": "All-in-one content creation: topic planning, writing, formatting, and polishing for articles, marketing copy, and social media posts.",
            "systemPrompt": r#"你是一名专业的内容创作助手，擅长微信公众号和自媒体内容。

## 核心能力
1. **选题规划** — 使用 content-planner skill 搜索微信热文，分析竞品，生成内容日历
2. **文章撰写** — 使用 article-writer skill 的5种风格和11步工作流
3. **热搜追踪** — 使用 daily-trending skill 聚合多平台热搜
4. **网络调研** — 使用 web-search skill 搜索素材和验证事实

## 5种写作风格
- **deep-analysis**: 严谨结构、数据支撑 (2000-4000字)
- **practical-guide**: 步骤清晰、可操作 (1500-3000字)
- **story-driven**: 对话式、情感共鸣 (1500-2500字)
- **opinion**: 观点鲜明、正反论证 (1000-2000字)
- **news-brief**: 倒金字塔、事实导向 (500-1000字)

## 工作原则
- 写作前先确认选题和风格
- 大纲需经用户确认后再展开撰写
- 用故事代替说教，用数据支撑观点
- 段落不超过4行（手机屏幕可视范围）
- 前3行必须有吸引力钩子
"#,
            "systemPromptEn": r#"You are a professional content creation assistant skilled in social media and blog writing.

## Core Capabilities
1. **Topic Planning** — Use the content-planner skill to research trending articles, analyze competitors, and generate a content calendar
2. **Article Writing** — Use the article-writer skill with 5 styles and an 11-step workflow
3. **Trending Topics** — Use the daily-trending skill to aggregate trending searches across platforms
4. **Web Research** — Use the web-search skill to find material and verify facts

## 5 Writing Styles
- **deep-analysis**: rigorous structure, data-backed (2000–4000 words)
- **practical-guide**: clear steps, actionable (1500–3000 words)
- **story-driven**: conversational, emotionally engaging (1500–2500 words)
- **opinion**: strong viewpoint, balanced arguments (1000–2000 words)
- **news-brief**: inverted pyramid, fact-oriented (500–1000 words)

## Principles
- Confirm the topic and style before writing
- Get user approval on the outline before drafting
- Show, don't tell; support opinions with data
- Keep paragraphs under 4 lines (mobile-friendly)
- The first 3 lines must contain an attention-grabbing hook
"#,
            "skillIds": ["content-planner", "article-writer", "daily-trending", "web-search"]
        }),
        json!({
            "id": "lesson-planner",
            "name": "备课出卷专家",
            "nameEn": "Lesson Planner",
            "icon": "📚",
            "description": "阅读教材和教学参考资料，生成教案、试卷、答案解析或英语听力原文。",
            "descriptionEn": "Read textbooks and teaching references to generate lesson plans, exams, answer keys, or English listening scripts.",
            "systemPrompt": r#"你是一名资深教育专家助手，专精K12教学内容设计。

## 核心能力
1. **教案生成** — 根据教材内容和课标要求，生成结构化教案
2. **试卷设计** — 使用 docx skill 生成难度均衡的试卷 (Word格式)
3. **答案解析** — 创建包含详细解题过程的答案
4. **数据统计** — 使用 xlsx skill 生成成绩分析表 (Excel格式)
5. **英语听力** — 编写英语听力理解原文

## 工作原则
- 遵循国家课程标准，确保内容适龄
- 试卷难度分布: 基础60% + 中等25% + 拔高15%
- 教案包含: 教学目标、重难点、教学过程、板书设计、课后反思
- 试卷包含: 题目编号、分值、参考答案、评分标准
- 输出文件统一使用 docx 格式（试卷）或 xlsx 格式（数据）
"#,
            "systemPromptEn": r#"You are a senior education expert assistant specializing in K-12 instructional content design.

## Core Capabilities
1. **Lesson Plan Generation** — Create structured lesson plans based on textbook content and curriculum standards
2. **Exam Design** — Use the docx skill to generate balanced-difficulty exams (Word format)
3. **Answer Keys** — Create answers with detailed solution steps
4. **Data Analysis** — Use the xlsx skill to generate grade analysis sheets (Excel format)
5. **English Listening** — Write English listening comprehension scripts

## Principles
- Follow national curriculum standards; ensure age-appropriate content
- Exam difficulty distribution: basic 60% + intermediate 25% + advanced 15%
- Lesson plans include: objectives, key/difficult points, teaching process, board design, post-class reflection
- Exams include: question numbers, scores, reference answers, grading criteria
- Output files in docx (exams) or xlsx (data) format
"#,
            "skillIds": ["docx", "xlsx", "web-search"]
        }),
        json!({
            "id": "content-summarizer",
            "name": "内容总结助手",
            "nameEn": "Content Summarizer",
            "icon": "📋",
            "description": "支持音视频、链接、文档摘要。自动识别会议、讲座、访谈等内容类型。",
            "descriptionEn": "Summarize audio, video, links, and documents. Automatically detects content types like meetings, lectures, and interviews.",
            "systemPrompt": r#"你是一名专业的内容摘要助手，擅长信息提炼和结构化整理。

## 核心能力
1. **网页总结** — 使用 web-search skill 搜索 + 抓取网页内容后提炼要点
2. **文档摘要** — 总结用户上传的文档、文章
3. **会议纪要** — 从文字记录中提取决策、行动项
4. **多源聚合** — 综合多个来源生成统一摘要

## 输出格式
- **一句话摘要**: 核心结论
- **关键要点**: 3-5 条bullet points
- **详细摘要**: 按原文结构分段总结
- **行动项** (如适用): TODO 列表

## 工作原则
- 保留关键细节，消除冗余
- 区分事实与观点
- 自动识别内容类型（会议/讲座/访谈/文章）并调整摘要风格
- 给出链接时先搜索获取内容，再总结
"#,
            "systemPromptEn": r#"You are a professional content summarization assistant skilled in information extraction and structured organization.

## Core Capabilities
1. **Web Summarization** — Use the web-search skill to search and fetch web content, then extract key points
2. **Document Summarization** — Summarize user-uploaded documents and articles
3. **Meeting Minutes** — Extract decisions and action items from transcripts
4. **Multi-source Aggregation** — Combine multiple sources into a unified summary

## Output Format
- **One-line Summary**: core conclusion
- **Key Points**: 3–5 bullet points
- **Detailed Summary**: section-by-section following the original structure
- **Action Items** (if applicable): TODO list

## Principles
- Retain key details, eliminate redundancy
- Distinguish facts from opinions
- Automatically detect content type (meeting/lecture/interview/article) and adjust summary style
- When given a link, fetch the content first, then summarize
"#,
            "skillIds": ["web-search"]
        }),
        json!({
            "id": "health-interpreter",
            "name": "医疗健康解读",
            "nameEn": "Health Interpreter",
            "icon": "🏥",
            "description": "体检报告、化验单、医学指标的通俗解读，帮你看懂每一项数值的含义和注意事项。",
            "descriptionEn": "Plain-language interpretation of medical reports, lab results, and health indicators — understand every value and what to watch for.",
            "systemPrompt": r#"你是一名耐心专业的全科医生助手，擅长将复杂的医学报告翻译成通俗易懂的语言。

## 核心能力
1. **体检报告解读** — 逐项解释指标含义、正常范围、偏高/偏低的可能原因
2. **化验单翻译** — 血常规、肝功能、肾功能、血脂、血糖等常见检验项目
3. **健康建议** — 根据异常指标给出饮食、运动、作息方面的调理建议
4. **医学科普** — 用大白话解释专业术语和疾病知识
5. **网络查询** — 使用 web-search 查询最新医学指南和健康资讯

## 工作流程
1. 用户发送体检报告文字或图片 → 识别所有指标项
2. 按系统分类（血液、肝功、肾功、血脂等）逐项解读
3. 对异常指标（↑↓）重点标注，解释可能原因
4. 给出综合健康评价和生活建议

## 输出格式
- 每个指标：指标名 → 你的数值 → 参考范围 → 通俗解读
- 异常项用 ⚠️ 标注，严重异常用 🔴 标注
- 最后给出「综合建议」和「建议复查项目」

## 工作原则
- 语言通俗，避免堆砌专业术语，必要时用比喻帮助理解
- 区分「需要关注」和「无需担心」的指标，不制造焦虑
- 遇到严重异常值时，明确建议尽快就医
- 不做具体疾病确诊，不推荐具体药物

## ⚠️ 免责声明（每次回答必须附带）
每次回答末尾必须附上以下声明：
> 📋 以上解读仅供健康参考，不构成医疗诊断或治疗建议。如有异常指标，请及时咨询专业医生。

## 图片支持说明
- 如果当前模型支持图片输入，可以直接分析用户上传的体检报告图片
- 如果不支持图片，请引导用户将报告中的数值以文字形式发送
"#,
            "systemPromptEn": r#"You are a patient and professional general practitioner assistant skilled at translating complex medical reports into plain language.

## Core Capabilities
1. **Medical Report Interpretation** — Explain each indicator's meaning, normal range, and possible causes of abnormalities
2. **Lab Result Translation** — Complete blood count, liver function, kidney function, lipids, blood sugar, etc.
3. **Health Advice** — Provide diet, exercise, and lifestyle suggestions based on abnormal indicators
4. **Medical Education** — Explain medical terminology and conditions in everyday language
5. **Web Search** — Use web-search to look up the latest medical guidelines and health information

## Workflow
1. User sends medical report text or image → identify all indicator items
2. Interpret item by item, grouped by system (blood, liver, kidney, lipids, etc.)
3. Highlight abnormal indicators (↑↓) and explain possible causes
4. Provide overall health assessment and lifestyle recommendations

## Output Format
- Each indicator: name → your value → reference range → plain-language explanation
- Flag abnormal items with ⚠️, serious abnormalities with 🔴
- End with "Overall Recommendations" and "Suggested Follow-up Tests"

## Principles
- Use plain language; avoid jargon overload; use analogies when helpful
- Distinguish "needs attention" from "no concern" — don't cause unnecessary anxiety
- For seriously abnormal values, clearly advise seeking medical attention promptly
- Do not diagnose specific diseases or recommend specific medications

## ⚠️ Disclaimer (must include in every response)
Append the following at the end of every response:
> 📋 The above interpretation is for health reference only and does not constitute medical diagnosis or treatment advice. Please consult a professional doctor for any abnormal indicators.

## Image Support
- If the current model supports image input, you can directly analyze uploaded medical report images
- If not, guide the user to send the values as text
"#,
            "skillIds": ["web-search"]
        }),
        json!({
            "id": "pet-care",
            "name": "萌宠管家",
            "nameEn": "Pet Care",
            "icon": "🐾",
            "description": "猫狗日常饲养、异常行为分析、食品配料解读，做你身边有温度的宠物百科。",
            "descriptionEn": "Daily cat & dog care, behavior analysis, and food ingredient guides — your warm and knowledgeable pet encyclopedia.",
            "systemPrompt": r#"你是一名温暖专业的宠物饲养顾问，熟悉猫狗的健康护理、行为心理和营养学知识。

## 核心能力
1. **行为分析** — 解读宠物异常行为的原因和应对方法（乱叫、乱尿、食欲变化等）
2. **健康咨询** — 常见疾病症状识别、就医时机判断、术后护理指导
3. **营养指导** — 猫粮狗粮配料表解读、自制鲜食建议、营养补充方案
4. **日常护理** — 疫苗驱虫时间表、洗护美容、季节护理要点
5. **网络搜索** — 使用 web-search 查询最新宠物医学资讯和产品评测

## 工作流程
1. 先了解宠物基本信息（品种、年龄、体重、是否绝育）
2. 详细了解问题表现（持续多久、频率、伴随症状）
3. 分析可能原因（按可能性从高到低排列）
4. 给出具体可操作的建议

## 沟通风格
- 语气温暖亲切，理解宠物主人的焦虑心情
- 称呼宠物为「毛孩子」「小家伙」等亲切用语
- 先安抚情绪，再给专业分析
- 建议要具体可操作，不说空话

## 工作原则
- 遇到疑似严重疾病症状（持续呕吐、血便、呼吸困难等），立即建议就医，不耽误
- 食物推荐以安全为第一原则，明确标注禁忌食物（如猫不能吃洋葱、狗不能吃巧克力）
- 不推荐具体商业品牌，只分析配料表成分
- 区分猫和狗的差异，不混淆护理方案

## ⚠️ 免责声明（涉及疾病时附带）
当涉及疾病判断时，回答末尾附上：
> 🐾 以上分析仅供参考，宠物健康问题请以宠物医院专业诊断为准。如症状持续或加重，请尽快带毛孩子就医。
"#,
            "systemPromptEn": r#"You are a warm and knowledgeable pet care consultant, well-versed in cat and dog health, behavior psychology, and nutrition.

## Core Capabilities
1. **Behavior Analysis** — Interpret abnormal pet behaviors and coping strategies (excessive barking, inappropriate elimination, appetite changes, etc.)
2. **Health Consultation** — Common symptom identification, when to see a vet, post-surgery care guidance
3. **Nutrition Guidance** — Pet food ingredient analysis, homemade meal suggestions, supplement plans
4. **Daily Care** — Vaccination and deworming schedules, grooming, seasonal care tips
5. **Web Search** — Use web-search for the latest pet medical information and product reviews

## Workflow
1. First, learn the pet's basic info (breed, age, weight, spayed/neutered)
2. Understand the problem in detail (duration, frequency, accompanying symptoms)
3. Analyze possible causes (ranked from most to least likely)
4. Provide specific, actionable recommendations

## Communication Style
- Warm and empathetic tone; understand pet owners' anxiety
- Use friendly terms like "your furry friend" or "your little buddy"
- First reassure emotions, then provide professional analysis
- Recommendations should be specific and actionable

## Principles
- For suspected serious symptoms (persistent vomiting, bloody stool, breathing difficulty), immediately advise seeing a vet
- Food recommendations prioritize safety; clearly list forbidden foods (e.g., cats can't eat onions, dogs can't eat chocolate)
- Do not recommend specific commercial brands; only analyze ingredient lists
- Differentiate between cat and dog care; never mix up care plans

## ⚠️ Disclaimer (include when discussing health issues)
When health issues are involved, append:
> 🐾 The above analysis is for reference only. For pet health issues, please consult a professional veterinarian. If symptoms persist or worsen, please take your furry friend to the vet promptly.
"#,
            "skillIds": ["web-search"]
        }),
    ]
}

#[tauri::command]
pub async fn agents_update(state: State<'_, OpenClawState>, id: String, updates: Value) -> Result<Value, String> {
    let row = sqlx::query("SELECT * FROM agents WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let row = match row {
        Some(r) => r,
        None => return Ok(Value::Null),
    };

    let existing_skill_ids = row.get::<String, _>("skill_ids");
    let updated_name = updates.get("name").and_then(|v| v.as_str()).unwrap_or(row.get::<String, _>("name").as_str()).to_string();
    let updated_description = updates.get("description").and_then(|v| v.as_str()).unwrap_or(row.get::<String, _>("description").as_str()).to_string();
    let updated_system_prompt = updates.get("systemPrompt").and_then(|v| v.as_str()).unwrap_or(row.get::<String, _>("system_prompt").as_str()).to_string();
    let updated_identity = updates.get("identity").and_then(|v| v.as_str()).unwrap_or(row.get::<String, _>("identity").as_str()).to_string();
    let updated_model = updates.get("model").and_then(|v| v.as_str()).unwrap_or(row.get::<String, _>("model").as_str()).to_string();
    let updated_icon = updates.get("icon").and_then(|v| v.as_str()).unwrap_or(row.get::<String, _>("icon").as_str()).to_string();
    let updated_skill_ids = updates.get("skillIds").map(|v| v.to_string()).unwrap_or(existing_skill_ids);
    let updated_enabled = updates
        .get("enabled")
        .and_then(|v| v.as_bool())
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(row.get::<i32, _>("enabled"));

    let now = now_ms();
    sqlx::query(
        "UPDATE agents
         SET name = ?, description = ?, system_prompt = ?, identity = ?, model = ?, icon = ?, skill_ids = ?, enabled = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(updated_name)
    .bind(updated_description)
    .bind(updated_system_prompt)
    .bind(updated_identity)
    .bind(updated_model)
    .bind(updated_icon)
    .bind(updated_skill_ids)
    .bind(updated_enabled)
    .bind(now)
    .bind(&id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    let updated = sqlx::query("SELECT * FROM agents WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let updated = match updated {
        Some(r) => r,
        None => return Ok(Value::Null),
    };

    Ok(json!({
        "id": updated.get::<String, _>("id"),
        "name": updated.get::<String, _>("name"),
        "description": updated.get::<String, _>("description"),
        "systemPrompt": updated.get::<String, _>("system_prompt"),
        "identity": updated.get::<String, _>("identity"),
        "model": updated.get::<String, _>("model"),
        "icon": updated.get::<String, _>("icon"),
        "skillIds": serde_json::from_str::<Vec<String>>(&updated.get::<String, _>("skill_ids")).unwrap_or_default(),
        "enabled": updated.get::<i32, _>("enabled") != 0,
        "isDefault": updated.get::<i32, _>("is_default") != 0,
        "source": updated.get::<String, _>("source"),
        "presetId": updated.get::<String, _>("preset_id"),
        "createdAt": updated.get::<i64, _>("created_at"),
        "updatedAt": updated.get::<i64, _>("updated_at")
    }))
}

#[tauri::command]
pub async fn agents_delete(state: State<'_, OpenClawState>, id: String) -> Result<Value, String> {
    sqlx::query("DELETE FROM agents WHERE id = ?")
        .bind(&id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn agents_presets(state: State<'_, OpenClawState>) -> Result<Vec<Value>, String> {
    let rows = sqlx::query("SELECT preset_id FROM agents WHERE source = 'preset'")
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut installed = std::collections::HashSet::<String>::new();
    for row in rows {
        let preset_id = row.get::<String, _>("preset_id");
        if !preset_id.trim().is_empty() {
            installed.insert(preset_id);
        }
    }

    let mut result = Vec::new();
    for mut p in preset_agents() {
        let pid = p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let is_installed = installed.contains(&pid);
        if let Value::Object(map) = &mut p {
            map.insert("installed".to_string(), Value::Bool(is_installed));
        }
        result.push(p);
    }
    Ok(result)
}

#[tauri::command]
pub async fn agents_add_preset(state: State<'_, OpenClawState>, presetId: String) -> Result<Value, String> {
    let preset = preset_agents()
        .into_iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(presetId.as_str()));

    let preset = match preset {
        Some(p) => p,
        None => return Ok(Value::Null),
    };

    let preset_id = preset.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let icon = preset.get("icon").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let description = preset.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let system_prompt = preset.get("systemPrompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let skill_ids = preset.get("skillIds").cloned().unwrap_or_else(|| json!([])).to_string();
    let name = preset.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let existing = sqlx::query("SELECT * FROM agents WHERE id = ?")
        .bind(&preset_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    if existing.is_some() {
        let row = existing.unwrap();
        return Ok(json!({
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

    let now = now_ms();
    sqlx::query(
        "INSERT INTO agents (id, name, description, system_prompt, identity, model, icon, skill_ids, enabled, is_default, source, preset_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, 0, 'preset', ?, ?, ?)",
    )
    .bind(&preset_id)
    .bind(&name)
    .bind(&description)
    .bind(&system_prompt)
    .bind("")
    .bind("")
    .bind(&icon)
    .bind(skill_ids)
    .bind(&preset_id)
    .bind(now)
    .bind(now)
    .execute(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;

    let row = sqlx::query("SELECT * FROM agents WHERE id = ?")
        .bind(&preset_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
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
    }))
}

#[tauri::command]
pub async fn api_fetch(
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<Value>,
) -> Value {
    if url.trim().is_empty() {
        return json!({ "ok": false, "status": 0, "error": "Missing url" });
    }
    let method = method.unwrap_or_else(|| "GET".to_string());
    let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET);

    let mut header_map = HeaderMap::new();
    if let Some(map) = headers {
        for (k, v) in map {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(&v),
            ) {
                header_map.insert(name, value);
            }
        }
    }

    let client = reqwest::Client::new();
    let mut req = client.request(method, url).headers(header_map);
    if let Some(body) = body {
        if body.is_string() {
            req = req.body(body.as_str().unwrap_or("").to_string());
        } else {
            req = req.body(body.to_string());
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return json!({ "ok": false, "status": 0, "error": e.to_string() }),
    };

    let status = resp.status().as_u16();
    let ok = resp.status().is_success();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return json!({ "ok": false, "status": status, "error": e.to_string() }),
    };
    let text = String::from_utf8_lossy(&bytes).to_string();

    let data = if content_type.to_lowercase().contains("json") {
        serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))
    } else {
        Value::String(text)
    };

    json!({ "ok": ok, "status": status, "data": data })
}

#[tauri::command]
pub async fn api_stream(
    app: AppHandle,
    requestId: String,
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<Value>,
) -> Value {
    if requestId.trim().is_empty() {
        return json!({ "ok": false, "status": 0, "error": "Missing requestId" });
    }
    if url.trim().is_empty() {
        return json!({ "ok": false, "status": 0, "error": "Missing url" });
    }

    let method = method.unwrap_or_else(|| "POST".to_string());
    let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::POST);

    let mut header_map = HeaderMap::new();
    if let Some(map) = headers {
        for (k, v) in map {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(&v),
            ) {
                header_map.insert(name, value);
            }
        }
    }

    let client = reqwest::Client::new();
    let mut req = client.request(method, url).headers(header_map);
    if let Some(body) = body {
        if body.is_string() {
            req = req.body(body.as_str().unwrap_or("").to_string());
        } else {
            req = req.body(body.to_string());
        }
    }

    let cancel_token = CancellationToken::new();
    {
        let mut map = API_STREAM_CANCELLATIONS.lock().unwrap();
        map.insert(requestId.clone(), cancel_token.clone());
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let mut map = API_STREAM_CANCELLATIONS.lock().unwrap();
            map.remove(&requestId);
            return json!({ "ok": false, "status": 0, "error": e.to_string() });
        }
    };

    let status = resp.status().as_u16();
    let ok = resp.status().is_success();

    if !ok {
        let text = resp.text().await.unwrap_or_else(|_| "".to_string());
        let mut map = API_STREAM_CANCELLATIONS.lock().unwrap();
        map.remove(&requestId);
        return json!({ "ok": false, "status": status, "error": text });
    }

    let app_clone = app.clone();
    let request_id_clone = requestId.clone();
    tauri::async_runtime::spawn(async move {
        let data_event = format!("api_stream_data_{}", request_id_clone);
        let done_event = format!("api_stream_done_{}", request_id_clone);
        let error_event = format!("api_stream_error_{}", request_id_clone);
        let abort_event = format!("api_stream_abort_{}", request_id_clone);

        let mut stream = resp.bytes_stream();
        while let Some(item) = stream.next().await {
            if cancel_token.is_cancelled() {
                let _ = app_clone.emit(&abort_event, json!({ "requestId": request_id_clone }));
                let mut map = API_STREAM_CANCELLATIONS.lock().unwrap();
                map.remove(&request_id_clone);
                return;
            }
            match item {
                Ok(chunk) => {
                    let text = String::from_utf8_lossy(&chunk).to_string();
                    if !text.is_empty() {
                        let _ = app_clone.emit(&data_event, text);
                    }
                }
                Err(e) => {
                    let _ = app_clone.emit(&error_event, e.to_string());
                    let mut map = API_STREAM_CANCELLATIONS.lock().unwrap();
                    map.remove(&request_id_clone);
                    return;
                }
            }
        }

        let _ = app_clone.emit(&done_event, json!({ "requestId": request_id_clone }));
        let mut map = API_STREAM_CANCELLATIONS.lock().unwrap();
        map.remove(&request_id_clone);
    });

    json!({ "ok": true, "status": status })
}

#[tauri::command]
pub fn api_cancel_stream(app: AppHandle, requestId: String) -> Value {
    if requestId.is_empty() {
        return json!({ "success": false, "error": "Missing requestId" });
    }
    let token = {
        let map = API_STREAM_CANCELLATIONS.lock().unwrap();
        map.get(&requestId).cloned()
    };
    if let Some(t) = token {
        t.cancel();
    }
    let abort_event = format!("api_stream_abort_{}", requestId);
    let _ = app.emit(&abort_event, json!({ "requestId": requestId }));
    json!({ "success": true })
}

#[tauri::command]
pub async fn get_api_config(app: AppHandle) -> Result<Value, String> {
    let path = app.path().app_data_dir().map_err(|e| e.to_string())?.join("store.json");
    if !path.exists() {
        return Ok(json!({ "apiKey": "", "baseUrl": "", "model": "" }));
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let store: HashMap<String, Value> = serde_json::from_str(&content).unwrap_or_default();
    let app_config = store.get("app_config").cloned().unwrap_or_else(|| json!({}));
    let api_key = app_config.pointer("/api/key").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let base_url = app_config.pointer("/api/baseUrl").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let model = app_config.pointer("/model/defaultModel").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(json!({ "apiKey": api_key, "baseUrl": base_url, "model": model }))
}

#[tauri::command]
pub async fn check_api_config(app: AppHandle, options: Value) -> Result<Value, String> {
    let _ = options;
    let cfg = get_api_config(app).await?;
    let api_key = cfg.get("apiKey").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let base_url = cfg.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let model = cfg.get("model").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if api_key.is_empty() || base_url.is_empty() || model.is_empty() {
        return Ok(json!({ "hasConfig": false, "config": cfg, "error": "Missing API key/baseUrl/model" }));
    }
    Ok(json!({ "hasConfig": true, "config": cfg }))
}

#[tauri::command]
pub async fn save_api_config(app: AppHandle, config: Value) -> Result<Value, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    }
    let path = data_dir.join("store.json");
    let mut store: HashMap<String, Value> = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let mut app_config = store.get("app_config").cloned().unwrap_or_else(|| json!({}));
    if !app_config.is_object() {
        app_config = json!({});
    }
    let api_key = config.get("apiKey").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let base_url = config.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let model = config.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if let Value::Object(map) = &mut app_config {
        let api = map.entry("api".to_string()).or_insert_with(|| json!({}));
        if let Value::Object(api_map) = api {
            if !api_key.trim().is_empty() {
                api_map.insert("key".to_string(), json!(api_key));
            }
            if !base_url.trim().is_empty() {
                api_map.insert("baseUrl".to_string(), json!(base_url));
            }
        }
        let model_obj = map.entry("model".to_string()).or_insert_with(|| json!({}));
        if let Value::Object(model_map) = model_obj {
            if !model.trim().is_empty() {
                model_map.insert("defaultModel".to_string(), json!(model));
            }
        }
    }

    store.insert("app_config".to_string(), app_config);
    let content = serde_json::to_string(&store).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub fn generate_session_title(userInput: serde_json::Value) -> serde_json::Value {
    let _ = userInput;
    json!("New Session")
}

#[tauri::command]
pub fn get_recent_cwds(limit: serde_json::Value) -> serde_json::Value {
    let _ = limit;
    json!([])
}

#[tauri::command]
pub fn window_minimize(app: AppHandle) -> serde_json::Value {
    let window = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().values().next().cloned());
    if let Some(w) = window {
        match w.minimize() {
            Ok(_) => json!({"success": true}),
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    } else {
        json!({"success": false, "error": "No window available"})
    }
}

#[tauri::command]
pub fn window_toggle_maximize(app: AppHandle) -> serde_json::Value {
    let window = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().values().next().cloned());
    if let Some(w) = window {
        match w.is_maximized() {
            Ok(true) => match w.unmaximize() {
                Ok(_) => json!({"success": true}),
                Err(e) => json!({"success": false, "error": e.to_string()}),
            },
            Ok(false) => match w.maximize() {
                Ok(_) => json!({"success": true}),
                Err(e) => json!({"success": false, "error": e.to_string()}),
            },
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    } else {
        json!({"success": false, "error": "No window available"})
    }
}

#[tauri::command]
pub fn window_close(app: AppHandle) -> serde_json::Value {
    let window = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().values().next().cloned());
    if let Some(w) = window {
        match w.close() {
            Ok(_) => json!({"success": true}),
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    } else {
        json!({"success": false, "error": "No window available"})
    }
}

#[tauri::command]
pub fn window_is_maximized(app: AppHandle) -> bool {
    let window = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().values().next().cloned());
    window
        .and_then(|w| w.is_maximized().ok())
        .unwrap_or(false)
}

#[tauri::command]
pub fn window_show_system_menu(position: serde_json::Value) -> serde_json::Value {
    let _ = position;
    json!({"success": true})
}

fn open_path_with_system(path: &std::path::Path, reveal_in_folder: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        if reveal_in_folder {
            cmd.arg("-R");
        }
        let status = cmd.arg(path).status().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("open failed with status {}", status))
        }
    }
    #[cfg(target_os = "windows")]
    {
        let status = if reveal_in_folder {
            Command::new("explorer")
                .arg(format!("/select,{}", path.to_string_lossy()))
                .status()
                .map_err(|e| e.to_string())?
        } else {
            Command::new("explorer")
                .arg(path)
                .status()
                .map_err(|e| e.to_string())?
        };
        if status.success() {
            Ok(())
        } else {
            Err(format!("explorer failed with status {}", status))
        }
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let target = if reveal_in_folder {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let status = Command::new("xdg-open")
            .arg(target)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("xdg-open failed with status {}", status))
        }
    }
}

#[tauri::command]
pub fn shell_open_path(filePath: serde_json::Value) -> serde_json::Value {
    let path = filePath.as_str().unwrap_or("").trim().to_string();
    if path.is_empty() {
        return json!({"success": false, "error": "Missing filePath"});
    }
    match open_path_with_system(std::path::Path::new(&path), false) {
        Ok(_) => json!({"success": true}),
        Err(e) => json!({"success": false, "error": e}),
    }
}

#[tauri::command]
pub fn shell_show_item_in_folder(filePath: serde_json::Value) -> serde_json::Value {
    let path = filePath.as_str().unwrap_or("").trim().to_string();
    if path.is_empty() {
        return json!({"success": false, "error": "Missing filePath"});
    }
    match open_path_with_system(std::path::Path::new(&path), true) {
        Ok(_) => json!({"success": true}),
        Err(e) => json!({"success": false, "error": e}),
    }
}

#[tauri::command]
pub async fn cowork_set_session_pinned(state: State<'_, OpenClawState>, sessionId: String, pinned: bool) -> Result<Value, String> {
    if sessionId.trim().is_empty() {
        return Ok(json!({ "success": false, "error": "Missing sessionId" }));
    }
    sqlx::query("UPDATE cowork_sessions SET pinned = ?, updated_at = ? WHERE id = ?")
        .bind(if pinned { 1 } else { 0 })
        .bind(now_ms())
        .bind(&sessionId)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn cowork_rename_session(state: State<'_, OpenClawState>, sessionId: String, title: String) -> Result<Value, String> {
    let title = title.trim().to_string();
    if sessionId.trim().is_empty() {
        return Ok(json!({ "success": false, "error": "Missing sessionId" }));
    }
    if title.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing title" }));
    }
    sqlx::query("UPDATE cowork_sessions SET title = ?, updated_at = ? WHERE id = ?")
        .bind(&title)
        .bind(now_ms())
        .bind(&sessionId)
        .execute(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub fn cowork_remote_managed(sessionId: Value) -> Value {
    let _ = sessionId;
    json!({ "success": true, "remoteManaged": false })
}

#[tauri::command]
pub fn cowork_export_result_image(options: serde_json::Value) -> serde_json::Value {
    let _ = options;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn cowork_capture_image_chunk(options: serde_json::Value) -> serde_json::Value {
    let _ = options;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn cowork_save_result_image(options: serde_json::Value) -> serde_json::Value {
    let _ = options;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn cowork_export_session_text(options: serde_json::Value) -> serde_json::Value {
    let _ = options;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn cowork_respond_to_permission(options: Value) -> Value {
    let _ = options;
    json!({ "success": true })
}

#[tauri::command]
pub async fn cowork_list_memory_entries(
    state: State<'_, OpenClawState>,
    query: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Value, String> {
    let query = query.unwrap_or_default().trim().to_string();
    let limit = limit.unwrap_or(200).clamp(1, 2000);
    let offset = offset.unwrap_or(0).max(0);

    let sql = if query.is_empty() {
        "SELECT id, text, kind, created_at, updated_at
         FROM cowork_memory_entries
         ORDER BY updated_at DESC
         LIMIT ? OFFSET ?"
    } else {
        "SELECT id, text, kind, created_at, updated_at
         FROM cowork_memory_entries
         WHERE text LIKE ?
         ORDER BY updated_at DESC
         LIMIT ? OFFSET ?"
    };

    let rows = if query.is_empty() {
        sqlx::query(sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.db.pool)
            .await
    } else {
        sqlx::query(sql)
            .bind(format!("%{}%", query))
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.db.pool)
            .await
    };

    let result = match rows {
        Ok(rows) => {
            let mut entries = Vec::new();
            for row in rows {
                entries.push(json!({
                    "id": row.get::<String, _>("id"),
                    "text": row.get::<String, _>("text"),
                    "kind": row.get::<String, _>("kind"),
                    "createdAt": row.get::<i64, _>("created_at"),
                    "updatedAt": row.get::<i64, _>("updated_at")
                }));
            }
            json!({ "success": true, "entries": entries })
        }
        Err(e) => json!({ "success": false, "error": e.to_string() }),
    };
    Ok(result)
}

#[tauri::command]
pub async fn cowork_create_memory_entry(state: State<'_, OpenClawState>, text: String) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Ok(json!({ "success": false, "error": "Missing text" }));
    }
    let kind = "explicit".to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();

    let res = sqlx::query(
        "INSERT INTO cowork_memory_entries (id, text, kind, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&text)
    .bind(&kind)
    .bind(now)
    .bind(now)
    .execute(&state.db.pool)
    .await;

    let result = match res {
        Ok(_) => json!({ "success": true, "entry": { "id": id, "text": text, "kind": kind, "createdAt": now, "updatedAt": now } }),
        Err(e) => json!({ "success": false, "error": e.to_string() }),
    };
    Ok(result)
}

#[tauri::command]
pub async fn cowork_update_memory_entry(state: State<'_, OpenClawState>, id: String, text: String) -> Result<Value, String> {
    if id.trim().is_empty() {
        return Ok(json!({ "success": false, "error": "Missing id" }));
    }
    if text.trim().is_empty() {
        return Ok(json!({ "success": false, "error": "Missing text" }));
    }
    let now = now_ms();

    let res = sqlx::query(
        "UPDATE cowork_memory_entries SET text = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&text)
    .bind(now)
    .bind(&id)
    .execute(&state.db.pool)
    .await;

    let result = match res {
        Ok(r) if r.rows_affected() > 0 => json!({ "success": true, "entry": { "id": id, "text": text, "updatedAt": now } }),
        Ok(_) => json!({ "success": false, "error": "Entry not found" }),
        Err(e) => json!({ "success": false, "error": e.to_string() }),
    };
    Ok(result)
}

#[tauri::command]
pub async fn cowork_delete_memory_entry(state: State<'_, OpenClawState>, id: String) -> Result<Value, String> {
    if id.trim().is_empty() {
        return Ok(json!({ "success": false, "error": "Missing id" }));
    }
    let result = match sqlx::query("DELETE FROM cowork_memory_entries WHERE id = ?")
        .bind(&id)
        .execute(&state.db.pool)
        .await
    {
        Ok(_) => json!({ "success": true }),
        Err(e) => json!({ "success": false, "error": e.to_string() }),
    };
    Ok(result)
}

#[tauri::command]
pub async fn cowork_get_memory_stats(state: State<'_, OpenClawState>) -> Result<Value, String> {
    let total = sqlx::query("SELECT COUNT(1) AS c FROM cowork_memory_entries")
        .fetch_one(&state.db.pool)
        .await
        .ok()
        .and_then(|r| r.try_get::<i64, _>("c").ok())
        .unwrap_or(0);

    let explicit = sqlx::query("SELECT COUNT(1) AS c FROM cowork_memory_entries WHERE kind = 'explicit'")
        .fetch_one(&state.db.pool)
        .await
        .ok()
        .and_then(|r| r.try_get::<i64, _>("c").ok())
        .unwrap_or(0);

    let implicit = total.saturating_sub(explicit);
    Ok(json!({
        "success": true,
        "stats": {
            "total": total,
            "explicit": explicit,
            "implicit": implicit,
            "created": total,
            "stale": 0,
            "updated": 0,
            "deleted": 0
        }
    }))
}

fn sanitize_bootstrap_filename(filename: &str) -> Option<String> {
    let name = filename.trim();
    if name.is_empty() {
        return None;
    }
    if name.contains('/') || name.contains('\\') {
        return None;
    }
    let allowed = [
        "USER.md",
        "IDENTITY.md",
        "SOUL.md",
        "SYSTEM.md",
        "USER.txt",
        "IDENTITY.txt",
        "SOUL.txt",
        "SYSTEM.txt",
    ];
    if allowed.contains(&name) {
        Some(name.to_string())
    } else {
        None
    }
}

async fn cowork_working_dir(app: &AppHandle, state: &OpenClawState) -> Result<PathBuf, String> {
    let stored = kv_get(&state.db.pool, "cowork.config")
        .await?
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());

    let from_config = stored
        .as_ref()
        .and_then(|v| v.get("workingDirectory"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);

    let fallback = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("cowork-workspace");

    let dir = from_config.unwrap_or(fallback);
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

#[tauri::command]
pub async fn cowork_read_bootstrap_file(app: AppHandle, state: State<'_, OpenClawState>, filename: String) -> Result<Value, String> {
    let filename = match sanitize_bootstrap_filename(&filename) {
        Some(v) => v,
        None => return Ok(json!({ "success": false, "error": "Invalid filename" })),
    };
    let dir = match cowork_working_dir(&app, &state).await {
        Ok(d) => d,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    let path = dir.join(filename);
    let content = std::fs::read_to_string(&path).unwrap_or_else(|_| "".to_string());
    Ok(json!({ "success": true, "content": content }))
}

#[tauri::command]
pub async fn cowork_write_bootstrap_file(app: AppHandle, state: State<'_, OpenClawState>, filename: String, content: String) -> Result<Value, String> {
    let filename = match sanitize_bootstrap_filename(&filename) {
        Some(v) => v,
        None => return Ok(json!({ "success": false, "error": "Invalid filename" })),
    };
    let dir = match cowork_working_dir(&app, &state).await {
        Ok(d) => d,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    let path = dir.join(filename);
    if let Err(e) = std::fs::write(&path, content) {
        return Ok(json!({ "success": false, "error": e.to_string() }));
    }
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub fn dialog_select_directory() -> Value {
    let folder = rfd::FileDialog::new().pick_folder();
    match folder {
        Some(path) => json!({ "success": true, "canceled": false, "path": path.to_string_lossy() }),
        None => json!({ "success": true, "canceled": true }),
    }
}

#[tauri::command]
pub fn dialog_select_file(title: Option<String>, defaultPath: Option<String>) -> Value {
    let mut dialog = rfd::FileDialog::new();
    if let Some(title) = title.as_deref() {
        dialog = dialog.set_title(title);
    }
    if let Some(default_path) = defaultPath.as_deref() {
        dialog = dialog.set_directory(default_path);
    }
    let file = dialog.pick_file();
    match file {
        Some(path) => json!({ "success": true, "canceled": false, "path": path.to_string_lossy() }),
        None => json!({ "success": true, "canceled": true }),
    }
}

#[tauri::command]
pub fn dialog_select_files(title: Option<String>, defaultPath: Option<String>) -> Value {
    let mut dialog = rfd::FileDialog::new();
    if let Some(title) = title.as_deref() {
        dialog = dialog.set_title(title);
    }
    if let Some(default_path) = defaultPath.as_deref() {
        dialog = dialog.set_directory(default_path);
    }
    let files = dialog.pick_files();
    match files {
        Some(paths) => json!({
            "success": true,
            "canceled": false,
            "paths": paths.into_iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>()
        }),
        None => json!({ "success": true, "canceled": true, "paths": [] }),
    }
}

#[tauri::command]
pub fn dialog_save_inline_file(
    app: AppHandle,
    filename: Option<String>,
    contentBase64: String,
    cwd: Option<String>,
) -> Value {
    let filename = filename
        .unwrap_or_else(|| "attachment.bin".to_string())
        .trim()
        .to_string();
    let filename = if filename.is_empty() { "attachment.bin".to_string() } else { filename };
    let content_base64 = contentBase64;
    if content_base64.is_empty() {
        return json!({ "success": false, "error": "Missing contentBase64" });
    }

    let bytes = match BASE64_STANDARD.decode(content_base64.as_bytes()) {
        Ok(b) => b,
        Err(e) => return json!({ "success": false, "error": e.to_string() }),
    };

    let base_dir = cwd
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| app.path().app_data_dir().ok())
        .unwrap_or_else(|| std::env::temp_dir());

    let dir = base_dir.join(".lobsterai-inline");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return json!({ "success": false, "error": e.to_string() });
    }
    let path = dir.join(filename);
    if let Err(e) = std::fs::write(&path, bytes) {
        return json!({ "success": false, "error": e.to_string() });
    }
    json!({ "success": true, "canceled": false, "path": path.to_string_lossy() })
}

#[tauri::command]
pub fn dialog_read_file_as_data_url(filePath: String) -> Value {
    let path = PathBuf::from(filePath);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return json!({ "success": false, "error": e.to_string() }),
    };
    let mime = MimeGuess::from_path(&path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    let encoded = BASE64_STANDARD.encode(&bytes);
    let data_url = format!("data:{};base64,{}", mime, encoded);
    json!({ "success": true, "dataUrl": data_url })
}

#[tauri::command]
pub fn app_update_download(url: serde_json::Value) -> serde_json::Value {
    let _ = url;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn app_update_cancel_download() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn app_update_install(filePath: serde_json::Value) -> serde_json::Value {
    let _ = filePath;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn log_get_path(app: AppHandle) -> serde_json::Value {
    match app.path().app_log_dir() {
        Ok(path) => json!({"success": true, "path": path.to_string_lossy()}),
        Err(e) => json!({"success": false, "error": e.to_string()}),
    }
}

#[tauri::command]
pub fn log_open_folder(app: AppHandle) -> serde_json::Value {
    let log_dir = match app.path().app_log_dir() {
        Ok(path) => path,
        Err(e) => return json!({"success": false, "error": e.to_string()}),
    };
    match open_path_with_system(&log_dir, false) {
        Ok(_) => json!({"success": true}),
        Err(e) => json!({"success": false, "error": e}),
    }
}

#[tauri::command]
pub fn log_export_zip(app: AppHandle) -> serde_json::Value {
    let log_dir = match app.path().app_log_dir() {
        Ok(path) => path,
        Err(e) => return json!({"success": false, "error": e.to_string()}),
    };
    if !log_dir.exists() {
        return json!({"success": false, "error": "Log directory does not exist"});
    }

    let export_name = format!("lobsterai-logs-{}.zip", now_ms());
    let export_path = std::env::temp_dir().join(export_name);

    #[cfg(target_os = "windows")]
    let status_result = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(format!(
            "Compress-Archive -Path '{}' -DestinationPath '{}' -Force",
            log_dir.join("*").to_string_lossy(),
            export_path.to_string_lossy()
        ))
        .status();

    #[cfg(not(target_os = "windows"))]
    let status_result = Command::new("zip")
        .arg("-r")
        .arg(&export_path)
        .arg(".")
        .current_dir(&log_dir)
        .status();

    match status_result {
        Ok(status) if status.success() => json!({
            "success": true,
            "canceled": false,
            "path": export_path.to_string_lossy(),
            "missingEntries": []
        }),
        Ok(status) => json!({
            "success": false,
            "error": format!("zip command failed with status {}", status)
        }),
        Err(e) => json!({
            "success": false,
            "error": e.to_string()
        }),
    }
}

fn default_im_config() -> Value {
    json!({
        "dingtalk": { "instances": [] },
        "feishu": { "instances": [] },
        "telegram": {
            "enabled": false,
            "botToken": "",
            "dmPolicy": "open",
            "allowFrom": [],
            "groupPolicy": "allowlist",
            "groupAllowFrom": [],
            "groups": { "*": { "requireMention": true } },
            "historyLimit": 50,
            "replyToMode": "off",
            "linkPreview": true,
            "streaming": "partial",
            "mediaMaxMb": 100,
            "proxy": "",
            "webhookUrl": "",
            "webhookSecret": "",
            "debug": false
        },
        "qq": { "instances": [] },
        "discord": {
            "enabled": false,
            "botToken": "",
            "dmPolicy": "open",
            "allowFrom": [],
            "groupPolicy": "allowlist",
            "groupAllowFrom": [],
            "guilds": { "*": { "requireMention": true } },
            "historyLimit": 50,
            "streaming": "off",
            "mediaMaxMb": 25,
            "proxy": "",
            "debug": false
        },
        "nim": {
            "enabled": false,
            "appKey": "",
            "account": "",
            "token": ""
        },
        "netease-bee": {
            "enabled": false,
            "clientId": "",
            "secret": "",
            "debug": true
        },
        "wecom": {
            "enabled": false,
            "botId": "",
            "secret": "",
            "dmPolicy": "open",
            "allowFrom": [],
            "groupPolicy": "open",
            "groupAllowFrom": [],
            "sendThinkingMessage": true,
            "debug": true
        },
        "popo": {
            "enabled": false,
            "connectionMode": "websocket",
            "appKey": "",
            "appSecret": "",
            "token": "",
            "aesKey": "",
            "webhookBaseUrl": "",
            "webhookPath": "/popo/callback",
            "webhookPort": 3100,
            "dmPolicy": "open",
            "allowFrom": [],
            "groupPolicy": "open",
            "groupAllowFrom": [],
            "textChunkLimit": 3000,
            "richTextChunkLimit": 5000,
            "debug": true
        },
        "weixin": {
            "enabled": false,
            "accountId": "",
            "dmPolicy": "open",
            "allowFrom": [],
            "groupPolicy": "open",
            "groupAllowFrom": [],
            "debug": true
        },
        "settings": {
            "systemPrompt": "",
            "skillsEnabled": true
        }
    })
}

fn default_im_status() -> Value {
    json!({
        "dingtalk": { "instances": [] },
        "feishu": { "instances": [] },
        "telegram": { "connected": false, "startedAt": null, "lastError": null, "botUsername": null, "lastInboundAt": null, "lastOutboundAt": null },
        "discord": { "connected": false, "starting": false, "startedAt": null, "lastError": null, "botUsername": null, "lastInboundAt": null, "lastOutboundAt": null },
        "nim": { "connected": false, "startedAt": null, "lastError": null, "botAccount": null, "lastInboundAt": null, "lastOutboundAt": null },
        "netease-bee": { "connected": false, "startedAt": null, "lastError": null, "botAccount": null, "lastInboundAt": null, "lastOutboundAt": null },
        "qq": { "instances": [] },
        "wecom": { "connected": false, "startedAt": null, "lastError": null, "botId": null, "lastInboundAt": null, "lastOutboundAt": null },
        "popo": { "connected": false, "startedAt": null, "lastError": null, "lastInboundAt": null, "lastOutboundAt": null },
        "weixin": { "connected": false, "startedAt": null, "lastError": null, "lastInboundAt": null, "lastOutboundAt": null }
    })
}

fn deep_merge(into: &mut Value, patch: &Value) {
    match (into, patch) {
        (Value::Object(into_map), Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                match into_map.get_mut(k) {
                    Some(existing) => deep_merge(existing, v),
                    None => {
                        into_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (into_slot, patch_val) => {
            *into_slot = patch_val.clone();
        }
    }
}

async fn get_im_config_from_db(state: &OpenClawState) -> Result<Value, String> {
    let mut base = default_im_config();
    if let Some(raw) = kv_get(&state.db.pool, "im.config").await? {
        if let Ok(stored) = serde_json::from_str::<Value>(&raw) {
            deep_merge(&mut base, &stored);
        }
    }
    Ok(base)
}

async fn set_im_config_to_db(state: &OpenClawState, config: &Value) -> Result<(), String> {
    kv_set(&state.db.pool, "im.config", &config.to_string()).await
}

#[tauri::command]
pub async fn im_get_config(state: State<'_, OpenClawState>) -> Result<Value, String> {
    match get_im_config_from_db(&state).await {
        Ok(config) => Ok(json!({ "success": true, "config": config })),
        Err(e) => Ok(json!({ "success": false, "error": e })),
    }
}

#[tauri::command]
pub async fn im_set_config(
    state: State<'_, OpenClawState>,
    config: Value,
    options: Value,
) -> Result<Value, String> {
    let _ = options;
    let mut existing = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    deep_merge(&mut existing, &config);
    if let Err(e) = set_im_config_to_db(&state, &existing).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": existing }))
}

#[tauri::command]
pub async fn im_sync_config(state: State<'_, OpenClawState>) -> Result<Value, String> {
    let _ = state;
    Ok(json!({ "success": true }))
}

fn set_platform_enabled(config: &mut Value, platform: &str, enabled: bool) {
    if let Value::Object(map) = config {
        let entry = map.entry(platform.to_string()).or_insert_with(|| json!({}));
        if let Value::Object(pmap) = entry {
            pmap.insert("enabled".to_string(), Value::Bool(enabled));
        }
    }
}

#[tauri::command]
pub async fn im_start_gateway(state: State<'_, OpenClawState>, platform: Value) -> Result<Value, String> {
    let platform = platform.as_str().unwrap_or("").to_string();
    if platform.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing platform" }));
    }
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    set_platform_enabled(&mut config, &platform, true);
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn im_stop_gateway(state: State<'_, OpenClawState>, platform: Value) -> Result<Value, String> {
    let platform = platform.as_str().unwrap_or("").to_string();
    if platform.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing platform" }));
    }
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    set_platform_enabled(&mut config, &platform, false);
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn im_test_gateway(
    state: State<'_, OpenClawState>,
    platform: Value,
    configOverride: Value,
) -> Result<Value, String> {
    let _ = state;
    let _ = configOverride;
    let platform = platform.as_str().unwrap_or("unknown");
    Ok(json!({
        "success": true,
        "result": {
            "platform": platform,
            "testedAt": now_ms(),
            "verdict": "fail",
            "checks": [
                { "code": "openclaw_gateway_not_running", "level": "warn", "message": "IM gateway is not implemented in Tauri backend yet." }
            ]
        }
    }))
}

#[tauri::command]
pub async fn im_get_status(state: State<'_, OpenClawState>) -> Result<Value, String> {
    let _ = state;
    Ok(json!({ "success": true, "status": default_im_status() }))
}

#[tauri::command]
pub fn im_get_local_ip() -> Value {
    let ip = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|sock| {
            let _ = sock.connect("8.8.8.8:80");
            sock.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    json!({ "success": true, "ip": ip })
}

#[tauri::command]
pub fn im_get_openclaw_config_schema() -> Value {
    json!({ "success": true, "schema": {} })
}

#[tauri::command]
pub fn im_weixin_qr_login_start() -> Value {
    json!({ "success": false, "error": "not_implemented" })
}

#[tauri::command]
pub fn im_weixin_qr_login_wait(accountId: Value) -> Value {
    let _ = accountId;
    json!({ "success": false, "error": "not_implemented" })
}

#[tauri::command]
pub fn im_list_pairing_requests(platform: Value) -> Value {
    let _ = platform;
    json!({ "success": true, "requests": [], "allowFrom": [] })
}

#[tauri::command]
pub fn im_approve_pairing_code(platform: Value, code: Value) -> Value {
    let _ = platform;
    let _ = code;
    json!({ "success": true })
}

#[tauri::command]
pub fn im_reject_pairing_request(platform: Value, code: Value) -> Value {
    let _ = platform;
    let _ = code;
    json!({ "success": true })
}

fn default_qq_instance_config(instance_id: &str, instance_name: &str) -> Value {
    json!({
        "instanceId": instance_id,
        "instanceName": instance_name,
        "enabled": false,
        "appId": "",
        "appSecret": "",
        "dmPolicy": "open",
        "allowFrom": [],
        "groupPolicy": "open",
        "groupAllowFrom": [],
        "historyLimit": 50,
        "markdownSupport": true,
        "imageServerBaseUrl": "",
        "debug": false
    })
}

fn default_feishu_instance_config(instance_id: &str, instance_name: &str) -> Value {
    json!({
        "instanceId": instance_id,
        "instanceName": instance_name,
        "enabled": false,
        "appId": "",
        "appSecret": "",
        "domain": "feishu",
        "dmPolicy": "open",
        "allowFrom": [],
        "groupPolicy": "open",
        "groupAllowFrom": [],
        "groups": { "*": { "requireMention": true } },
        "historyLimit": 50,
        "streaming": true,
        "replyMode": "auto",
        "blockStreaming": false,
        "footer": { "status": true, "elapsed": true },
        "mediaMaxMb": 30,
        "debug": false
    })
}

fn default_dingtalk_instance_config(instance_id: &str, instance_name: &str) -> Value {
    json!({
        "instanceId": instance_id,
        "instanceName": instance_name,
        "enabled": false,
        "clientId": "",
        "clientSecret": "",
        "dmPolicy": "open",
        "allowFrom": [],
        "groupPolicy": "open",
        "sessionTimeout": 1800000,
        "separateSessionByConversation": true,
        "groupSessionScope": "group",
        "sharedMemoryAcrossConversations": false,
        "gatewayBaseUrl": "",
        "debug": false
    })
}

fn ensure_instances_array(config: &mut Value, key: &str) {
    if let Value::Object(map) = config {
        let entry = map.entry(key.to_string()).or_insert_with(|| json!({ "instances": [] }));
        if let Value::Object(inner) = entry {
            if !inner.get("instances").map(|v| v.is_array()).unwrap_or(false) {
                inner.insert("instances".to_string(), json!([]));
            }
        }
    }
}

#[tauri::command]
pub async fn im_add_qq_instance(state: State<'_, OpenClawState>, name: Value) -> Result<Value, String> {
    let instance_name = name.as_str().unwrap_or("QQ").to_string();
    let instance_id = uuid::Uuid::new_v4().to_string();
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    ensure_instances_array(&mut config, "qq");
    if let Some(instances) = config.pointer_mut("/qq/instances").and_then(|v| v.as_array_mut()) {
        instances.push(default_qq_instance_config(&instance_id, &instance_name));
    }
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": config, "instanceId": instance_id }))
}

#[tauri::command]
pub async fn im_delete_qq_instance(state: State<'_, OpenClawState>, instanceId: Value) -> Result<Value, String> {
    let instance_id = instanceId.as_str().unwrap_or("").to_string();
    if instance_id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing instanceId" }));
    }
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    if let Some(instances) = config.pointer_mut("/qq/instances").and_then(|v| v.as_array_mut()) {
        instances.retain(|v| v.get("instanceId").and_then(|x| x.as_str()) != Some(&instance_id));
    }
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": config }))
}

#[tauri::command]
pub async fn im_set_qq_instance_config(
    state: State<'_, OpenClawState>,
    instanceId: Value,
    config: Value,
    options: Value,
) -> Result<Value, String> {
    let _ = options;
    let instance_id = instanceId.as_str().unwrap_or("").to_string();
    if instance_id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing instanceId" }));
    }
    let mut current = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    if let Some(instances) = current.pointer_mut("/qq/instances").and_then(|v| v.as_array_mut()) {
        for inst in instances.iter_mut() {
            if inst.get("instanceId").and_then(|v| v.as_str()) == Some(&instance_id) {
                deep_merge(inst, &config);
            }
        }
    }
    if let Err(e) = set_im_config_to_db(&state, &current).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": current }))
}

#[tauri::command]
pub async fn im_add_feishu_instance(state: State<'_, OpenClawState>, name: Value) -> Result<Value, String> {
    let instance_name = name.as_str().unwrap_or("Feishu").to_string();
    let instance_id = uuid::Uuid::new_v4().to_string();
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    ensure_instances_array(&mut config, "feishu");
    if let Some(instances) = config.pointer_mut("/feishu/instances").and_then(|v| v.as_array_mut()) {
        instances.push(default_feishu_instance_config(&instance_id, &instance_name));
    }
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": config, "instanceId": instance_id }))
}

#[tauri::command]
pub async fn im_delete_feishu_instance(state: State<'_, OpenClawState>, instanceId: Value) -> Result<Value, String> {
    let instance_id = instanceId.as_str().unwrap_or("").to_string();
    if instance_id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing instanceId" }));
    }
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    if let Some(instances) = config.pointer_mut("/feishu/instances").and_then(|v| v.as_array_mut()) {
        instances.retain(|v| v.get("instanceId").and_then(|x| x.as_str()) != Some(&instance_id));
    }
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": config }))
}

#[tauri::command]
pub async fn im_set_feishu_instance_config(
    state: State<'_, OpenClawState>,
    instanceId: Value,
    config: Value,
    options: Value,
) -> Result<Value, String> {
    let _ = options;
    let instance_id = instanceId.as_str().unwrap_or("").to_string();
    if instance_id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing instanceId" }));
    }
    let mut current = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    if let Some(instances) = current.pointer_mut("/feishu/instances").and_then(|v| v.as_array_mut()) {
        for inst in instances.iter_mut() {
            if inst.get("instanceId").and_then(|v| v.as_str()) == Some(&instance_id) {
                deep_merge(inst, &config);
            }
        }
    }
    if let Err(e) = set_im_config_to_db(&state, &current).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": current }))
}

#[tauri::command]
pub async fn im_add_dingtalk_instance(state: State<'_, OpenClawState>, name: Value) -> Result<Value, String> {
    let instance_name = name.as_str().unwrap_or("DingTalk").to_string();
    let instance_id = uuid::Uuid::new_v4().to_string();
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    ensure_instances_array(&mut config, "dingtalk");
    if let Some(instances) = config.pointer_mut("/dingtalk/instances").and_then(|v| v.as_array_mut()) {
        instances.push(default_dingtalk_instance_config(&instance_id, &instance_name));
    }
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": config, "instanceId": instance_id }))
}

#[tauri::command]
pub async fn im_delete_dingtalk_instance(state: State<'_, OpenClawState>, instanceId: Value) -> Result<Value, String> {
    let instance_id = instanceId.as_str().unwrap_or("").to_string();
    if instance_id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing instanceId" }));
    }
    let mut config = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    if let Some(instances) = config.pointer_mut("/dingtalk/instances").and_then(|v| v.as_array_mut()) {
        instances.retain(|v| v.get("instanceId").and_then(|x| x.as_str()) != Some(&instance_id));
    }
    if let Err(e) = set_im_config_to_db(&state, &config).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": config }))
}

#[tauri::command]
pub async fn im_set_dingtalk_instance_config(
    state: State<'_, OpenClawState>,
    instanceId: Value,
    config: Value,
    options: Value,
) -> Result<Value, String> {
    let _ = options;
    let instance_id = instanceId.as_str().unwrap_or("").to_string();
    if instance_id.is_empty() {
        return Ok(json!({ "success": false, "error": "Missing instanceId" }));
    }
    let mut current = match get_im_config_from_db(&state).await {
        Ok(v) => v,
        Err(e) => return Ok(json!({ "success": false, "error": e })),
    };
    if let Some(instances) = current.pointer_mut("/dingtalk/instances").and_then(|v| v.as_array_mut()) {
        for inst in instances.iter_mut() {
            if inst.get("instanceId").and_then(|v| v.as_str()) == Some(&instance_id) {
                deep_merge(inst, &config);
            }
        }
    }
    if let Err(e) = set_im_config_to_db(&state, &current).await {
        return Ok(json!({ "success": false, "error": e }));
    }
    Ok(json!({ "success": true, "config": current }))
}

#[tauri::command]
pub fn scheduled_tasks_list() -> serde_json::Value {
    json!([])
}

#[tauri::command]
pub fn scheduled_tasks_get(id: serde_json::Value) -> serde_json::Value {
    let _ = id;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_create(input: serde_json::Value) -> serde_json::Value {
    let _ = input;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_update(id: serde_json::Value, input: serde_json::Value) -> serde_json::Value {
    let _ = id;
    let _ = input;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_delete(id: serde_json::Value) -> serde_json::Value {
    let _ = id;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_toggle(id: serde_json::Value, enabled: serde_json::Value) -> serde_json::Value {
    let _ = id;
    let _ = enabled;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_run_manually(id: serde_json::Value) -> serde_json::Value {
    let _ = id;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_stop(id: serde_json::Value) -> serde_json::Value {
    let _ = id;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_list_runs(taskId: serde_json::Value, limit: serde_json::Value, offset: serde_json::Value) -> serde_json::Value {
    let _ = taskId;
    let _ = limit;
    let _ = offset;
    json!([])
}

#[tauri::command]
pub fn scheduled_tasks_count_runs(taskId: serde_json::Value) -> serde_json::Value {
    let _ = taskId;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_list_all_runs(limit: serde_json::Value, offset: serde_json::Value) -> serde_json::Value {
    let _ = limit;
    let _ = offset;
    json!([])
}

#[tauri::command]
pub fn scheduled_tasks_resolve_session(sessionKey: serde_json::Value) -> serde_json::Value {
    let _ = sessionKey;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn scheduled_tasks_list_channels() -> serde_json::Value {
    json!([])
}

#[tauri::command]
pub fn scheduled_tasks_list_channel_conversations(channel: serde_json::Value, accountId: serde_json::Value) -> serde_json::Value {
    let _ = channel;
    let _ = accountId;
    json!([])
}

#[tauri::command]
pub fn permissions_check_calendar() -> serde_json::Value {
    json!({"granted": false})
}

#[tauri::command]
pub fn permissions_request_calendar() -> serde_json::Value {
    json!({"granted": false})
}

#[tauri::command]
pub fn auth_login(loginUrl: serde_json::Value) -> serde_json::Value {
    let _ = loginUrl;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn auth_exchange(code: serde_json::Value) -> serde_json::Value {
    let _ = code;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn auth_get_user() -> serde_json::Value {
    serde_json::Value::Null
}

#[tauri::command]
pub fn auth_get_quota() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn auth_logout() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn auth_refresh_token() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn auth_get_access_token() -> serde_json::Value {
    json!("")
}

#[tauri::command]
pub fn auth_get_models() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn auth_get_profile_summary() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn enterprise_get_config() -> serde_json::Value {
    json!({})
}

#[tauri::command]
pub fn feishu_install_qrcode(isLark: serde_json::Value) -> serde_json::Value {
    let _ = isLark;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn feishu_install_poll(deviceCode: serde_json::Value) -> serde_json::Value {
    let _ = deviceCode;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn feishu_install_verify(appId: serde_json::Value, appSecret: serde_json::Value) -> serde_json::Value {
    let _ = appId;
    let _ = appSecret;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn github_copilot_request_device_code() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn github_copilot_poll_for_token(deviceCode: serde_json::Value, interval: serde_json::Value, expiresIn: serde_json::Value) -> serde_json::Value {
    let _ = deviceCode;
    let _ = interval;
    let _ = expiresIn;
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn github_copilot_cancel_polling() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn github_copilot_sign_out() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}

#[tauri::command]
pub fn github_copilot_refresh_token() -> serde_json::Value {
    json!({"success": false, "error": "not_implemented"})
}
