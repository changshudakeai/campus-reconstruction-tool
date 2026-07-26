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
    fn all_three_tables_exist_after_open() {
        let db = Database::open_in_memory().unwrap();
        for table in ["raw_observations", "review_decisions", "trash"] {
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
    }
}
