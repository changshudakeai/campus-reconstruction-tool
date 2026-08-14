//! 数据库迁移执行器
//!
//! 迁移脚本编译期嵌入（`include_str!`），按版本号顺序执行；
//! 每个版本在一个事务内完成（脚本 + schema_migrations 记账原子提交）。
//! 已应用的版本跳过，重复打开数据库是幂等操作。

use rusqlite::Connection;

use crate::error::{Error, Result};

/// 单个迁移：版本号 + 描述 + 嵌入的 SQL 脚本
struct Migration {
    version: u32,
    description: &'static str,
    sql: &'static str,
}

/// 全部迁移，按版本号升序排列。
///
/// V2 正式发布前，开发数据库在 schema 基线变化后删除重建；因此此处只描述
/// 新装数据库的当前基线，不保留旧开发数据库的恢复分支。
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "初始 schema：全局设置、校区、方案",
        sql: include_str!("../migrations/001_initial_schema.sql"),
    },
    Migration {
        version: 2,
        description: "原始观测表 + 评审终态表 + 回收站",
        sql: include_str!("../migrations/002_add_persistence_tables.sql"),
    },
    Migration {
        version: 3,
        description: "校区锚点列：anchor_lng/anchor_lat",
        sql: include_str!("../migrations/003_add_anchor_columns_to_campuses.sql"),
    },
    Migration {
        version: 4,
        description: "校区地址列：address（最近使用记录展示校区地址，ADR-0006）",
        sql: include_str!("../migrations/004_add_campus_address.sql"),
    },
    Migration {
        version: 5,
        description: "候选投影与完整采集批次（ADR-0040）",
        sql: include_str!("../migrations/005_add_candidate_projections.sql"),
    },
    Migration {
        version: 6,
        description: "校区高德地点标识列：poi_id（ADR-0008，重复点选只切换）",
        sql: include_str!("../migrations/006_add_campus_poi_id.sql"),
    },
    Migration {
        version: 7,
        description: "T36: regeo 补名持久化缓存（坐标键 -> 名称）",
        sql: include_str!("../migrations/007_add_regeo_name_cache.sql"),
    },
    Migration {
        version: 8,
        description: "候选投影保存名称来源（OSM/高德/缓存/仍未命名/失败）",
        sql: include_str!("../migrations/008_add_candidate_name_source.sql"),
    },
];

/// 当前最新 schema 版本号
pub const LATEST_SCHEMA_VERSION: u32 = 8;

/// 把数据库迁移到最新版本（幂等）
pub(crate) fn run_migrations(conn: &mut Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL DEFAULT (datetime('now')),
            description TEXT
        )",
        [],
    )?;

    for migration in MIGRATIONS {
        let applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [migration.version],
            |row| row.get(0),
        )?;
        if applied {
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)
            .map_err(|error| Error::MigrationFailed {
                version: migration.version,
                message: error.to_string(),
            })?;
        tx.execute(
            "INSERT INTO schema_migrations (version, description) VALUES (?1, ?2)",
            rusqlite::params![migration.version, migration.description],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// 查询当前已应用的最高版本号（空库返回 None）
pub(crate) fn current_version(conn: &Connection) -> Result<Option<u32>> {
    let version: Option<u32> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_and_match_latest() {
        let versions: Vec<u32> = MIGRATIONS
            .iter()
            .map(|migration| migration.version)
            .collect();
        assert!(versions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(*versions.last().unwrap(), LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn fresh_migration_chain_is_idempotent() {
        let mut connection = Connection::open_in_memory().unwrap();
        run_migrations(&mut connection).unwrap();
        run_migrations(&mut connection).unwrap();
        assert_eq!(
            current_version(&connection).unwrap(),
            Some(LATEST_SCHEMA_VERSION)
        );
    }
}
