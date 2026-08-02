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

/// 全部迁移，按版本号升序排列
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
        description: "候选投影展示属性（ADR-0040）",
        sql: include_str!("../migrations/006_add_candidate_display.sql"),
    },
];

/// 当前最新 schema 版本号
pub const LATEST_SCHEMA_VERSION: u32 = 6;

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
            .map_err(|err| Error::MigrationFailed {
                version: migration.version,
                message: err.to_string(),
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
        let versions: Vec<u32> = MIGRATIONS.iter().map(|m| m.version).collect();
        assert!(versions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(*versions.last().unwrap(), LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn run_migrations_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        run_migrations(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), Some(LATEST_SCHEMA_VERSION));
    }

    #[test]
    fn version_six_backfills_display_from_raw_observation_without_candidate_id_fallback() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now')),
                description TEXT
            );",
        )
        .unwrap();
        for migration in MIGRATIONS.iter().filter(|migration| migration.version <= 5) {
            conn.execute_batch(migration.sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (version, description) VALUES (?1, ?2)",
                rusqlite::params![migration.version, migration.description],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO raw_observations (
                id, plan_id, entity_type, entity_id, source_data, data_source_tag,
                digest, created_at, updated_at
            ) VALUES (?1, ?2, 'Building', ?3, ?4, 'overpass', 'digest', ?5, ?5)",
            rusqlite::params![
                "raw-1",
                "plan-1",
                "way/1",
                serde_json::json!({
                    "tags": { "name": "第一教学楼", "building": "school" }
                })
                .to_string(),
                "2026-08-01T00:00:00Z"
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO candidate_batches (id, plan_id, status, created_at, published_at)
             VALUES ('batch-1', 'plan-1', 'published', ?1, ?1)",
            ["2026-08-01T00:00:00Z"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO candidate_projections (
                collection_batch_id, candidate_id, plan_id, raw_observation_id,
                data_source_tag, source_entity_id, geometry_part_id, category,
                geometry_kind, normalized_geometry, validation, eligibility,
                isolation_reason, automatically_repaired, missing_in_latest_batch,
                created_at, updated_at
            ) VALUES (
                'batch-1', 'overpass:way/1:outer', 'plan-1', 'raw-1',
                'overpass', 'way/1', 'outer', 'Building', 'polygon', '[]',
                'retained', 'reviewable', NULL, 0, 0, ?1, ?1
            )",
            ["2026-08-01T00:00:00Z"],
        )
        .unwrap();

        run_migrations(&mut conn).unwrap();

        let (title, tags): (String, String) = conn
            .query_row(
                "SELECT display_title, display_tags
                 FROM candidate_projections
                 WHERE candidate_id = 'overpass:way/1:outer'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "第一教学楼");
        let tags: Vec<(String, String)> = serde_json::from_str(&tags).unwrap();
        assert!(tags.contains(&("building".to_owned(), "school".to_owned())));
        assert!(tags.contains(&("name".to_owned(), "第一教学楼".to_owned())));
    }
}
