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

#[test]
fn version_six_review_decisions_collapse_category_duplicates_by_candidate_id() {
    let (_directory, path, connection) = legacy_database(6);
    connection
        .execute(
            "INSERT INTO review_decisions (
                plan_id, entity_type, entity_id, review_state, updated_at
             ) VALUES ('plan-1', 'Building', 'overpass:way/1:outer', 'keep', ?1)",
            ["2026-08-01T00:00:00Z"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO review_decisions (
                plan_id, entity_type, entity_id, review_state, updated_at
             ) VALUES ('plan-1', 'Road', 'overpass:way/1:outer', 'remove', ?1)",
            ["2026-08-02T00:00:00Z"],
        )
        .unwrap();
    drop(connection);

    let database = Database::open(&path).unwrap();
    let decisions = database.list_review_decisions("plan-1").unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].candidate_id, "overpass:way/1:outer");
    assert_eq!(decisions[0].category, CandidateCategory::Road);
    assert!(decisions[0].review_state.is_remove());
}
