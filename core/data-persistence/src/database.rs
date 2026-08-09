//! 数据库句柄
//!
//! [`Database`] 是 B2 对外的唯一存储入口：打开即自动迁移到最新 schema，
//! 三组 API（原始观测/评审终态/回收站）都以 trait 实现在这个句柄上。
//! 写操作一律走事务（ADR-0002：崩溃后自动回滚到最近一致状态）。

use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;
use crate::migrations;

/// SQLite 数据库句柄（打开即迁移到最新版本）
pub struct Database {
    pub(crate) conn: Connection,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// 打开（或创建）指定路径的数据库文件，并迁移到最新 schema
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// 打开内存数据库（测试与临时用途），并迁移到最新 schema
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrations::run_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// 当前 schema 版本号（迁移完成后等于 [`crate::LATEST_SCHEMA_VERSION`]）
    pub fn schema_version(&self) -> Result<Option<u32>> {
        migrations::current_version(&self.conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_migrates_to_latest() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.schema_version().unwrap(),
            Some(crate::LATEST_SCHEMA_VERSION)
        );
    }

    #[test]
    fn fresh_database_has_final_schema_without_dev_repair_artifacts() {
        let db = Database::open_in_memory().unwrap();
        for table in [
            "raw_observations",
            "review_decisions",
            "trash",
            "candidate_batches",
            "candidate_projections",
            "current_candidate_batches",
            "regeo_name_cache",
        ] {
            let exists: bool = db
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "表 {table} 应在迁移后存在");
        }

        let audit_exists: bool = db
            .conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                    WHERE type='table' AND name='candidate_display_backfill_audit'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!audit_exists, "新装 schema 不应包含开发期回填审计表");

        let projection_columns = table_columns(&db.conn, "candidate_projections");
        assert!(projection_columns.contains(&("display_title".to_owned(), true, 0)));
        assert!(projection_columns.contains(&("display_tags".to_owned(), true, 0)));

        let decision_columns = table_columns(&db.conn, "review_decisions");
        assert!(decision_columns.contains(&("plan_id".to_owned(), true, 1)));
        assert!(decision_columns.contains(&("candidate_id".to_owned(), true, 2)));
        assert!(decision_columns.contains(&("category".to_owned(), true, 0)));
        assert!(!decision_columns
            .iter()
            .any(|(name, _, _)| name == "entity_id"));
    }

    fn table_columns(connection: &Connection, table: &str) -> Vec<(String, bool, usize)> {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((row.get(1)?, row.get::<_, usize>(3)? == 1, row.get(5)?))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }
}
