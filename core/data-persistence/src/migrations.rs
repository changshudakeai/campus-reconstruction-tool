//! 数据库迁移执行器
//!
//! 迁移脚本编译期嵌入（`include_str!`），按版本号顺序执行；
//! 每个版本在一个事务内完成（脚本 + schema_migrations 记账原子提交）。
//! 已应用的版本跳过，重复打开数据库是幂等操作。

#![allow(
    clippy::items_after_test_module,
    reason = "existing migration-runner tests stay adjacent to the runner while the v8 repair helpers remain grouped below"
)]

use rusqlite::{params, Connection, Transaction};

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
    Migration {
        version: 7,
        description: "评审决定使用稳定候选 ID 作为唯一身份（ADR-0040）",
        sql: include_str!("../migrations/007_review_decision_candidate_identity.sql"),
    },
    Migration {
        version: 8,
        description: "修复候选投影展示属性回填（ADR-0040）",
        sql: include_str!("../migrations/008_repair_candidate_display.sql"),
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
        if migration.version == 7 {
            validate_review_decision_identity(&tx).map_err(|message| Error::MigrationFailed {
                version: migration.version,
                message,
            })?;
        }
        tx.execute_batch(migration.sql)
            .map_err(|err| Error::MigrationFailed {
                version: migration.version,
                message: err.to_string(),
            })?;
        if migration.version == 8 {
            repair_candidate_display(&tx).map_err(|message| Error::MigrationFailed {
                version: migration.version,
                message,
            })?;
        }
        tx.execute(
            "INSERT INTO schema_migrations (version, description) VALUES (?1, ?2)",
            rusqlite::params![migration.version, migration.description],
        )?;
        tx.commit()?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct LegacyReviewDecision {
    category: String,
    review_state: String,
    reviewer_id: Option<String>,
    updated_at: String,
}

fn validate_review_decision_identity(tx: &Transaction<'_>) -> std::result::Result<(), String> {
    let mut statement = tx
        .prepare(
            "SELECT plan_id, entity_id, entity_type, review_state, reviewer_id, updated_at
             FROM review_decisions
             ORDER BY plan_id, entity_id, rowid",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut previous: Option<(String, String, LegacyReviewDecision)> = None;

    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let plan_id: String = row.get(0).map_err(|error| error.to_string())?;
        let candidate_id: String = row.get(1).map_err(|error| error.to_string())?;
        let decision = LegacyReviewDecision {
            category: row.get(2).map_err(|error| error.to_string())?,
            review_state: row.get(3).map_err(|error| error.to_string())?,
            reviewer_id: row.get(4).map_err(|error| error.to_string())?,
            updated_at: row.get(5).map_err(|error| error.to_string())?,
        };

        if let Some((previous_plan_id, previous_candidate_id, previous_decision)) = &previous {
            if previous_plan_id == &plan_id
                && previous_candidate_id == &candidate_id
                && previous_decision != &decision
            {
                return Err(format!(
                    "conflicting legacy review decisions for plan '{plan_id}', candidate '{candidate_id}'; category, review_state, reviewer_id, and updated_at must be identical"
                ));
            }
        }
        previous = Some((plan_id, candidate_id, decision));
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

fn repair_candidate_display(tx: &Transaction<'_>) -> std::result::Result<(), String> {
    let audit_table_exists: bool = tx
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_schema
                 WHERE type = 'table' AND name = 'candidate_display_backfill_audit'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !audit_table_exists {
        let projection_count: usize = tx
            .query_row("SELECT COUNT(*) FROM candidate_projections", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
        if projection_count > 0 {
            return Err(format!(
                "cannot safely repair {projection_count} candidate display row(s): version 6 backfill provenance is missing"
            ));
        }
        tx.execute_batch(
            "CREATE TABLE candidate_display_backfill_audit (
                 collection_batch_id TEXT NOT NULL,
                 candidate_id TEXT NOT NULL,
                 recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
                 repaired_at TEXT,
                 PRIMARY KEY(collection_batch_id, candidate_id)
             );",
        )
        .map_err(|error| error.to_string())?;
    }

    let mut statement = tx
        .prepare(
            "SELECT projection.collection_batch_id,
                    projection.candidate_id,
                    projection.raw_observation_id,
                    projection.source_entity_id,
                    projection.display_title,
                    projection.display_tags,
                    observation.source_data
             FROM candidate_display_backfill_audit AS audit
             JOIN candidate_projections AS projection
               ON projection.collection_batch_id = audit.collection_batch_id
              AND projection.candidate_id = audit.candidate_id
             LEFT JOIN raw_observations AS observation
               ON observation.id = projection.raw_observation_id
             WHERE audit.repaired_at IS NULL
             ORDER BY projection.collection_batch_id, projection.candidate_id",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut updates = Vec::new();
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let batch_id: String = row.get(0).map_err(|error| error.to_string())?;
        let candidate_id: String = row.get(1).map_err(|error| error.to_string())?;
        let raw_observation_id: String = row.get(2).map_err(|error| error.to_string())?;
        let source_entity_id: String = row.get(3).map_err(|error| error.to_string())?;
        let _display_title: String = row.get(4).map_err(|error| error.to_string())?;
        let display_tags_json: String = row.get(5).map_err(|error| error.to_string())?;
        let source_data_json: Option<String> = row.get(6).map_err(|error| error.to_string())?;
        let source_data_json = source_data_json.ok_or_else(|| {
            format!(
                "candidate '{candidate_id}' references missing raw observation '{raw_observation_id}'"
            )
        })?;
        let source_data: serde_json::Value = serde_json::from_str(&source_data_json).map_err(|error| {
            format!(
                "candidate '{candidate_id}' references invalid raw observation '{raw_observation_id}': {error}"
            )
        })?;
        let _current_tags: Vec<(String, String)> = serde_json::from_str(&display_tags_json)
            .map_err(|error| {
                format!("candidate '{candidate_id}' has invalid display tags: {error}")
            })?;
        let (repaired_title, repaired_tags) = complete_display(&source_data, &source_entity_id);
        updates.push((batch_id, candidate_id, repaired_title, repaired_tags));
    }
    drop(rows);
    drop(statement);

    let mut update = tx
        .prepare(
            "UPDATE candidate_projections
             SET display_title = ?3, display_tags = ?4
             WHERE collection_batch_id = ?1 AND candidate_id = ?2",
        )
        .map_err(|error| error.to_string())?;
    for (batch_id, candidate_id, title, tags) in updates {
        let tags = serde_json::to_string(&tags).map_err(|error| error.to_string())?;
        update
            .execute(params![batch_id, candidate_id, title, tags])
            .map_err(|error| error.to_string())?;
        tx.execute(
            "UPDATE candidate_display_backfill_audit
             SET repaired_at = datetime('now')
             WHERE collection_batch_id = ?1 AND candidate_id = ?2",
            params![batch_id, candidate_id],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn complete_display(
    source_data: &serde_json::Value,
    source_entity_id: &str,
) -> (String, Vec<(String, String)>) {
    let top_level = source_data.as_object();
    let nested_tags = top_level
        .and_then(|values| values.get("tags"))
        .and_then(serde_json::Value::as_object);
    let title = top_level
        .and_then(|values| values.get("name"))
        .and_then(scalar_text)
        .or_else(|| {
            nested_tags
                .and_then(|values| values.get("name"))
                .and_then(scalar_text)
        })
        .unwrap_or_else(|| source_entity_id.to_owned());

    let mut tags = Vec::new();
    if let Some(values) = top_level {
        for (key, value) in values {
            if key != "tags" {
                if let Some(text) = scalar_text(value) {
                    tags.push((key.clone(), text));
                }
            }
        }
    }
    if let Some(values) = nested_tags {
        for (key, value) in values {
            if let Some(text) = scalar_text(value) {
                tags.push((key.clone(), text));
            }
        }
    }
    tags.sort();
    (title, tags)
}

fn scalar_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}
