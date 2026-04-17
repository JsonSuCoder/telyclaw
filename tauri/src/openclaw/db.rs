use sqlx::{sqlite::SqlitePool, Pool, Sqlite};
use tauri::AppHandle;
use tauri::Manager;

pub struct OpenClawDb {
    pub pool: Pool<Sqlite>,
}

impl OpenClawDb {
    pub async fn new(app: &AppHandle) -> Result<Self, String> {
        let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        if !app_dir.exists() {
            std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
        }
        let db_path = app_dir.join("lobsterai.sqlite");
        let db_url = format!("sqlite:{}", db_path.to_string_lossy());
        
        let pool = SqlitePool::connect(&db_url).await.map_err(|e| e.to_string())?;
        
        let db = Self { pool };
        db.initialize_tables().await?;
        
        Ok(db)
    }

    async fn initialize_tables(&self) -> Result<(), String> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS kv (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cowork_sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                claude_session_id TEXT,
                status TEXT NOT NULL DEFAULT 'idle',
                pinned INTEGER NOT NULL DEFAULT 0,
                cwd TEXT NOT NULL,
                system_prompt TEXT NOT NULL DEFAULT '',
                execution_mode TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cowork_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT,
                created_at INTEGER NOT NULL,
                sequence INTEGER,
                FOREIGN KEY (session_id) REFERENCES cowork_sessions(id) ON DELETE CASCADE
            );"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cowork_messages_session_id ON cowork_messages(session_id);"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                system_prompt TEXT NOT NULL DEFAULT '',
                identity TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL DEFAULT '',
                icon TEXT NOT NULL DEFAULT '',
                skill_ids TEXT NOT NULL DEFAULT '[]',
                enabled INTEGER NOT NULL DEFAULT 1,
                is_default INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'custom',
                preset_id TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                transport_type TEXT NOT NULL DEFAULT 'stdio',
                config_json TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }
}
