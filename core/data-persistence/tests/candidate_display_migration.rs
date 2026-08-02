#![allow(
    clippy::disallowed_types,
    reason = "B2 migration tests must construct legacy SQLite schemas before exercising Database::open"
)]

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
    CandidateShape, CandidateValidation, Database, Error, RawObservation, RawObservationsApi,
    ReviewDecisionsApi, LATEST_SCHEMA_VERSION,
};
use rusqlite::{params, Connection};
use shared_domain_types::CandidateCategory;
use tempfile::TempDir;

const LEGACY_MIGRATIONS: &[(u32, &str, &str)] = &[
    (
        1,
        "initial",
        include_str!("../migrations/001_initial_schema.sql"),
    ),
    (
        2,
        "persistence",
        include_str!("../migrations/002_add_persistence_tables.sql"),
    ),
    (
        3,
        "anchors",
        include_str!("../migrations/003_add_anchor_columns_to_campuses.sql"),
    ),
    (
        4,
        "address",
        include_str!("../migrations/004_add_campus_address.sql"),
    ),
    (
        5,
        "candidates",
        include_str!("../migrations/005_add_candidate_projections.sql"),
    ),
    (
        6,
        "display",
        include_str!("../migrations/006_add_candidate_display.sql"),
    ),
];

fn legacy_database(version: u32) -> (TempDir, std::path::PathBuf, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now')),
                description TEXT
            );",
        )
        .unwrap();
    for (migration_version, description, sql) in LEGACY_MIGRATIONS {
        if *migration_version > version {
            break;
        }
        connection.execute_batch(sql).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, description) VALUES (?1, ?2)",
                params![migration_version, description],
            )
            .unwrap();
    }
    (directory, path, connection)
}

fn insert_raw_observation(
    connection: &Connection,
    raw_id: &str,
    source_entity_id: &str,
    source_data: &str,
    data_source_tag: &str,
) {
    connection
        .execute(
            "INSERT INTO raw_observations (
                id, plan_id, entity_type, entity_id, source_data, data_source_tag,
                digest, created_at, updated_at
            ) VALUES (?1, 'plan-1', 'Building', ?2, ?3, ?4, 'digest', ?5, ?5)",
            params![
                raw_id,
                source_entity_id,
                source_data,
                data_source_tag,
                "2026-07-31T00:00:00Z"
            ],
        )
        .unwrap();
}

fn insert_published_batch(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO candidate_batches (id, plan_id, status, created_at, published_at)
             VALUES ('batch-1', 'plan-1', 'published', ?1, ?1)",
            ["2026-07-31T00:00:00Z"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO current_candidate_batches (plan_id, collection_batch_id)
             VALUES ('plan-1', 'batch-1')",
            [],
        )
        .unwrap();
}

fn insert_v5_projection(
    connection: &Connection,
    candidate_id: &str,
    raw_id: &str,
    source_entity_id: &str,
    geometry_part_id: &str,
) {
    connection
        .execute(
            "INSERT INTO candidate_projections (
                collection_batch_id, candidate_id, plan_id, raw_observation_id,
                data_source_tag, source_entity_id, geometry_part_id, category,
                geometry_kind, normalized_geometry, validation, eligibility,
                isolation_reason, automatically_repaired, missing_in_latest_batch,
                created_at, updated_at
            ) VALUES (
                'batch-1', ?1, 'plan-1', ?2, 'overpass', ?3, ?4,
                'Building', 'polygon', '[]', 'retained', 'reviewable', NULL, 0, 0, ?5, ?5
            )",
            params![
                candidate_id,
                raw_id,
                source_entity_id,
                geometry_part_id,
                "2026-07-31T00:00:00Z"
            ],
        )
        .unwrap();
}

#[allow(
    clippy::too_many_arguments,
    reason = "migration fixture mirrors persisted projection columns"
)]
fn insert_v6_projection(
    connection: &Connection,
    candidate_id: &str,
    raw_id: &str,
    data_source_tag: &str,
    source_entity_id: &str,
    geometry_part_id: &str,
    display_title: &str,
    display_tags: &str,
) {
    connection
        .execute(
            "INSERT INTO candidate_projections (
                collection_batch_id, candidate_id, plan_id, raw_observation_id,
                data_source_tag, source_entity_id, geometry_part_id, category,
                display_title, display_tags, geometry_kind, normalized_geometry,
                validation, eligibility, isolation_reason, automatically_repaired,
                missing_in_latest_batch, created_at, updated_at
            ) VALUES (
                'batch-1', ?1, 'plan-1', ?2, ?3, ?4, ?5, 'Building', ?6, ?7,
                'polygon', '[]', 'retained', 'reviewable', NULL, 0, 0, ?8, ?8
            )",
            params![
                candidate_id,
                raw_id,
                data_source_tag,
                source_entity_id,
                geometry_part_id,
                display_title,
                display_tags,
                "2026-08-02T00:00:00Z"
            ],
        )
        .unwrap();
}

fn mark_as_version_six_backfill(connection: &Connection, candidate_id: &str) {
    connection
        .execute(
            "INSERT INTO candidate_display_backfill_audit (
                 collection_batch_id, candidate_id, recorded_at
             ) VALUES ('batch-1', ?1, '2026-08-01T00:00:00Z')",
            [candidate_id],
        )
        .unwrap();
}

#[test]
fn version_five_upgrade_preserves_top_level_name_scalars_and_geometry_parts() {
    let (_directory, path, connection) = legacy_database(5);
    insert_raw_observation(
        &connection,
        "raw-1",
        "way/1",
        r#"{"name":"第一教学楼","levels":3,"active":true,"tags":{"building":"school","covered":false,"lanes":2},"geometry":[1,2]}"#,
        "overpass",
    );
    insert_published_batch(&connection);
    insert_v5_projection(
        &connection,
        "overpass:way/1:outer",
        "raw-1",
        "way/1",
        "outer",
    );
    insert_v5_projection(
        &connection,
        "overpass:way/1:inner",
        "raw-1",
        "way/1",
        "inner",
    );
    drop(connection);

    let database = Database::open(&path).unwrap();
    let projections = database
        .list_reviewable_candidate_projections("plan-1")
        .unwrap();
    assert_eq!(projections.len(), 2);
    assert_eq!(projections[0].candidate_id, "overpass:way/1:inner");
    assert_eq!(projections[1].candidate_id, "overpass:way/1:outer");
    for projection in projections {
        assert_eq!(projection.display.title, "第一教学楼");
        assert_eq!(
            projection.display.tags,
            vec![
                ("active".to_owned(), "true".to_owned()),
                ("building".to_owned(), "school".to_owned()),
                ("covered".to_owned(), "false".to_owned()),
                ("lanes".to_owned(), "2".to_owned()),
                ("levels".to_owned(), "3".to_owned()),
                ("name".to_owned(), "第一教学楼".to_owned()),
            ]
        );
    }
}

#[test]
fn applied_version_six_repairs_legacy_rows_without_overwriting_complete_display() {
    let (_directory, path, connection) = legacy_database(6);
    insert_raw_observation(
        &connection,
        "raw-gaode",
        "poi/7",
        r#"{"name":"游泳池","rating":4.5,"tags":{"leisure":"swimming_pool"}}"#,
        "gaode",
    );
    insert_raw_observation(
        &connection,
        "raw-overpass",
        "way/9",
        r#"{"name":"源数据名称","tags":{"building":"school"}}"#,
        "overpass",
    );
    insert_published_batch(&connection);
    insert_v6_projection(
        &connection,
        "gaode:poi/7:footprint",
        "raw-gaode",
        "gaode",
        "poi/7",
        "footprint",
        "poi/7",
        r#"[["leisure","swimming_pool"]]"#,
    );
    mark_as_version_six_backfill(&connection, "gaode:poi/7:footprint");
    insert_v6_projection(
        &connection,
        "overpass:way/9:outer",
        "raw-overpass",
        "overpass",
        "way/9",
        "outer",
        "人工确认的完整标题",
        r#"[["custom","preserved"],["name","人工确认的完整标题"]]"#,
    );
    drop(connection);

    let database = Database::open(&path).unwrap();
    let gaode = database
        .get_current_candidate_projection("plan-1", "gaode:poi/7:footprint")
        .unwrap()
        .unwrap();
    assert_eq!(gaode.display.title, "游泳池");
    assert_eq!(
        gaode.display.tags,
        vec![
            ("leisure".to_owned(), "swimming_pool".to_owned()),
            ("name".to_owned(), "游泳池".to_owned()),
            ("rating".to_owned(), "4.5".to_owned()),
        ]
    );

    let overpass = database
        .get_current_candidate_projection("plan-1", "overpass:way/9:outer")
        .unwrap()
        .unwrap();
    assert_eq!(overpass.display.title, "人工确认的完整标题");
    assert_eq!(
        overpass.display.tags,
        vec![
            ("custom".to_owned(), "preserved".to_owned()),
            ("name".to_owned(), "人工确认的完整标题".to_owned()),
        ]
    );
}

#[test]
fn applied_version_six_does_not_rewrite_new_display_that_matches_legacy_signature() {
    let (_directory, path, connection) = legacy_database(6);
    insert_raw_observation(
        &connection,
        "raw-collision",
        "way/collision",
        r#"{"name":"top-level-name","tags":{"name":"nested-name","building":"school"}}"#,
        "overpass",
    );
    insert_published_batch(&connection);
    insert_v6_projection(
        &connection,
        "overpass:way/collision:outer",
        "raw-collision",
        "overpass",
        "way/collision",
        "outer",
        "nested-name",
        r#"[["building","school"],["name","nested-name"]]"#,
    );
    drop(connection);

    let database = Database::open(&path).unwrap();
    let projection = database
        .get_current_candidate_projection("plan-1", "overpass:way/collision:outer")
        .unwrap()
        .unwrap();
    assert_eq!(projection.display.title, "nested-name");
    assert_eq!(
        projection.display.tags,
        vec![
            ("building".to_owned(), "school".to_owned()),
            ("name".to_owned(), "nested-name".to_owned()),
        ]
    );
}

#[test]
fn applied_version_six_without_backfill_audit_fails_instead_of_guessing() {
    let (_directory, path, connection) = legacy_database(6);
    insert_raw_observation(
        &connection,
        "raw-ambiguous",
        "way/ambiguous",
        r#"{"name":"top-level-name","tags":{"name":"nested-name"}}"#,
        "overpass",
    );
    insert_published_batch(&connection);
    insert_v6_projection(
        &connection,
        "overpass:way/ambiguous:outer",
        "raw-ambiguous",
        "overpass",
        "way/ambiguous",
        "outer",
        "nested-name",
        r#"[["name","nested-name"]]"#,
    );
    connection
        .execute_batch("DROP TABLE candidate_display_backfill_audit;")
        .unwrap();
    drop(connection);

    let error = Database::open(&path).unwrap_err();
    assert!(matches!(
        error,
        Error::MigrationFailed {
            version: 8,
            ref message
        } if message.contains("provenance") && message.contains("1 candidate display row")
    ));

    let connection = Connection::open(&path).unwrap();
    let version: u32 = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 7);
    let title: String = connection
        .query_row(
            "SELECT display_title FROM candidate_projections
             WHERE candidate_id = 'overpass:way/ambiguous:outer'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(title, "nested-name");
}

#[test]
fn existing_raw_observation_without_name_falls_back_to_source_entity_id() {
    let (_directory, path, connection) = legacy_database(5);
    insert_raw_observation(
        &connection,
        "raw-unnamed",
        "way/unnamed",
        r#"{"levels":2,"tags":{"building":"yes"}}"#,
        "overpass",
    );
    insert_published_batch(&connection);
    insert_v5_projection(
        &connection,
        "overpass:way/unnamed:outer",
        "raw-unnamed",
        "way/unnamed",
        "outer",
    );
    drop(connection);

    let database = Database::open(&path).unwrap();
    let projection = database
        .get_current_candidate_projection("plan-1", "overpass:way/unnamed:outer")
        .unwrap()
        .unwrap();
    assert_eq!(projection.display.title, "way/unnamed");
    assert_eq!(
        projection.display.tags,
        vec![
            ("building".to_owned(), "yes".to_owned()),
            ("levels".to_owned(), "2".to_owned()),
        ]
    );
}

#[test]
fn version_five_upgrade_rejects_missing_raw_observation() {
    let (_directory, path, connection) = legacy_database(5);
    insert_published_batch(&connection);
    insert_v5_projection(
        &connection,
        "overpass:way/missing:outer",
        "raw-missing",
        "way/missing",
        "outer",
    );
    drop(connection);

    let error = Database::open(&path).unwrap_err();
    assert!(matches!(
        error,
        Error::MigrationFailed {
            version: 8,
            ref message
        } if message.contains("raw-missing") && message.contains("missing")
    ));
}

#[test]
fn applied_version_six_upgrade_rejects_damaged_raw_json() {
    let (_directory, path, connection) = legacy_database(6);
    insert_raw_observation(
        &connection,
        "raw-damaged",
        "way/damaged",
        r#"{"tags":{"building":"yes"}}"#,
        "overpass",
    );
    insert_published_batch(&connection);
    insert_v6_projection(
        &connection,
        "overpass:way/damaged:outer",
        "raw-damaged",
        "overpass",
        "way/damaged",
        "outer",
        "way/damaged",
        r#"[["building","yes"]]"#,
    );
    mark_as_version_six_backfill(&connection, "overpass:way/damaged:outer");
    connection
        .execute(
            "UPDATE raw_observations SET source_data = '{damaged' WHERE id = 'raw-damaged'",
            [],
        )
        .unwrap();
    drop(connection);

    let error = Database::open(&path).unwrap_err();
    assert!(matches!(
        error,
        Error::MigrationFailed {
            version: 8,
            ref message
        } if message.contains("raw-damaged") && message.contains("invalid")
    ));

    let connection = Connection::open(&path).unwrap();
    let version: u32 = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 7);
    let (title, repaired_at): (String, Option<String>) = connection
        .query_row(
            "SELECT projection.display_title, audit.repaired_at
             FROM candidate_projections AS projection
             JOIN candidate_display_backfill_audit AS audit
               ON audit.collection_batch_id = projection.collection_batch_id
              AND audit.candidate_id = projection.candidate_id
             WHERE projection.candidate_id = 'overpass:way/damaged:outer'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(title, "way/damaged");
    assert_eq!(repaired_at, None);
}

#[test]
fn new_install_reaches_latest_and_preserves_new_candidate_display() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("new.db");
    let mut database = Database::open(&path).unwrap();
    assert_eq!(
        database.schema_version().unwrap(),
        Some(LATEST_SCHEMA_VERSION)
    );

    let observation = RawObservation::new(
        "plan-new",
        CandidateCategory::Sports,
        "way/42",
        serde_json::json!({"name": "源名称", "tags": {"leisure": "pitch"}}),
        "overpass",
    );
    let raw_id = observation.id.clone();
    database.write_raw_observations(&[observation]).unwrap();
    let batch = database.prepare_candidate_batch("plan-new").unwrap();
    database
        .write_candidate_projections(
            &batch.id,
            &[CandidateProjection::new(
                "overpass:way/42:outer",
                "plan-new",
                raw_id,
                "overpass",
                "way/42",
                "outer",
                CandidateCategory::Sports,
                CandidateDisplay::new(
                    "人工完整标题",
                    vec![("custom".to_owned(), "complete".to_owned())],
                ),
                CandidateShape::polygon(serde_json::json!([])),
                CandidateValidation::Retained,
                CandidateEligibility::Reviewable,
            )],
        )
        .unwrap();
    database.publish_candidate_batch(&batch.id).unwrap();
    drop(database);

    let database = Database::open(&path).unwrap();
    let projection = database
        .get_current_candidate_projection("plan-new", "overpass:way/42:outer")
        .unwrap()
        .unwrap();
    assert_eq!(projection.display.title, "人工完整标题");
    assert_eq!(
        projection.display.tags,
        vec![("custom".to_owned(), "complete".to_owned())]
    );
}

fn allow_duplicate_legacy_review_decisions(connection: &Connection) {
    connection
        .execute_batch(
            "DROP INDEX idx_review_decisions_state;
             DROP TABLE review_decisions;
             CREATE TABLE review_decisions (
                 plan_id TEXT NOT NULL,
                 entity_type TEXT NOT NULL,
                 entity_id TEXT NOT NULL,
                 review_state TEXT NOT NULL,
                 reviewer_id TEXT,
                 updated_at TEXT NOT NULL
             );
             CREATE INDEX idx_review_decisions_state
                 ON review_decisions(review_state);",
        )
        .unwrap();
}

fn insert_legacy_review_decision(
    connection: &Connection,
    category: &str,
    state: &str,
    reviewer_id: Option<&str>,
    updated_at: &str,
) {
    connection
        .execute(
            "INSERT INTO review_decisions (
                plan_id, entity_type, entity_id, review_state, reviewer_id, updated_at
             ) VALUES ('plan-1', ?1, 'overpass:way/1:outer', ?2, ?3, ?4)",
            params![category, state, reviewer_id, updated_at],
        )
        .unwrap();
}

fn assert_version_seven_conflict_rolls_back(path: &std::path::Path, expected_rows: usize) {
    let error = Database::open(path).unwrap_err();
    match error {
        Error::MigrationFailed { version, message } => {
            assert_eq!(version, 7);
            assert!(message.contains("plan-1"));
            assert!(message.contains("overpass:way/1:outer"));
        }
        other => panic!("expected version 7 migration failure, got {other:?}"),
    }

    let connection = Connection::open(path).unwrap();
    let version: u32 = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 6);
    let rows: usize = connection
        .query_row("SELECT COUNT(*) FROM review_decisions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, expected_rows);
    let legacy_column_count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('review_decisions')
             WHERE name IN ('entity_type', 'entity_id')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_column_count, 2);
}

#[test]
fn version_six_review_decisions_collapse_only_equivalent_duplicates() {
    let (_directory, path, connection) = legacy_database(6);
    allow_duplicate_legacy_review_decisions(&connection);
    insert_legacy_review_decision(
        &connection,
        "Building",
        "keep",
        Some("reviewer-1"),
        "2026-08-01T00:00:00Z",
    );
    insert_legacy_review_decision(
        &connection,
        "Building",
        "keep",
        Some("reviewer-1"),
        "2026-08-01T00:00:00Z",
    );
    drop(connection);

    let database = Database::open(&path).unwrap();
    let decisions = database.list_review_decisions("plan-1").unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].candidate_id, "overpass:way/1:outer");
    assert_eq!(decisions[0].category, CandidateCategory::Building);
    assert!(decisions[0].review_state.is_keep());
    assert_eq!(decisions[0].reviewer_id.as_deref(), Some("reviewer-1"));
}

#[test]
fn version_six_review_decision_state_conflict_fails_and_rolls_back() {
    let (_directory, path, connection) = legacy_database(6);
    allow_duplicate_legacy_review_decisions(&connection);
    insert_legacy_review_decision(
        &connection,
        "Building",
        "keep",
        Some("reviewer-1"),
        "2026-08-01T00:00:00Z",
    );
    insert_legacy_review_decision(
        &connection,
        "Building",
        "remove",
        Some("reviewer-1"),
        "2026-08-01T00:00:00Z",
    );
    drop(connection);

    assert_version_seven_conflict_rolls_back(&path, 2);
}

#[test]
fn version_six_review_decision_category_conflict_fails_and_rolls_back() {
    let (_directory, path, connection) = legacy_database(6);
    insert_legacy_review_decision(
        &connection,
        "Building",
        "keep",
        Some("reviewer-1"),
        "2026-08-01T00:00:00Z",
    );
    insert_legacy_review_decision(
        &connection,
        "Road",
        "keep",
        Some("reviewer-1"),
        "2026-08-01T00:00:00Z",
    );
    drop(connection);

    assert_version_seven_conflict_rolls_back(&path, 2);
}
