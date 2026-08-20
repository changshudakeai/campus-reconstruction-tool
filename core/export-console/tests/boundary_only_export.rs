//! M1 端到端验收：已确认边界即可导出最小平整场地。
//!
//! 这些测试先固定完整导出用例的用户可见结果，再实现 F9 内部协调。

use std::path::Path;

use export_console::{
    BoundaryError, BoundaryExportRequest, Error, ExportArtifactTargets, ExportConsole,
    ExportPlanContext, ExportPlanState, MockSealGate, VersionError,
};
use manifest_generator::{FoundationManifest, ManifestOrientationSource};
use shared_domain_types::{Boundary, Orientation, PlanId};

fn boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.0000, 39.0000],
            [116.0010, 39.0000],
            [116.0010, 39.0010],
            [116.0000, 39.0010],
            [116.0000, 39.0000]
        ]]),
    }
}

fn request(
    dir: &Path,
    orientation: Option<Orientation>,
    boundary: Option<Boundary>,
    confirmed: bool,
) -> BoundaryExportRequest {
    BoundaryExportRequest::new(
        ExportPlanContext::new("测试校区", PlanId::generate(), "边界直出方案", "26.1.2"),
        ExportPlanState::new(boundary, confirmed, orientation),
        ExportArtifactTargets::new(
            dir.join("campus.schem"),
            dir.join("foundation_manifest.json"),
        ),
    )
}

fn read_manifest(path: &Path) -> FoundationManifest {
    let json = std::fs::read_to_string(path).expect("manifest 必须实际写入");
    FoundationManifest::from_json(&json).expect("manifest 必须是有效 JSON")
}

fn oversized_boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [0.0, 39.0],
            [100_000.0, 39.0],
            [100_000.0, 39.001],
            [0.0, 39.001],
            [0.0, 39.0]
        ]]),
    }
}

#[test]
fn confirmed_boundary_without_orientation_or_candidates_exports_real_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let gate = MockSealGate::new();
    let probe = gate.clone();
    let mut console = ExportConsole::new(gate);

    let result = console
        .export_confirmed_boundary(request(dir.path(), None, Some(boundary()), true))
        .expect("确认边界后不应被朝向或候选阻塞");

    assert!(!probe.is_sealed(), "空候选边界直出不应伪造评审封账");
    assert!(result.schematic_path.exists());
    assert!(result.manifest_path.exists());

    let inspection = sponge_export::verify_worldedit_import_contract(&result.schematic_path)
        .expect("必须生成可导入的 Sponge .schem");
    assert_eq!(result.schematic_dimensions, inspection.dimensions);
    assert_eq!(inspection.data_version, 3955);
    assert_eq!(inspection.dimensions[1], 1, "最小路径只生成一层平整场地");
    assert!(inspection.non_air_voxels > 0);
    assert!(
        inspection.palette.contains_key("minecraft:grass_block"),
        "底座表层必须是草方块"
    );

    let manifest = read_manifest(&result.manifest_path);
    assert_eq!(manifest.minecraft_version, "26.1.2");
    let orientation = manifest.orientation.expect("完整导出必须记录实际朝向");
    assert_eq!(orientation.degree, 0.0);
    assert_eq!(orientation.source, ManifestOrientationSource::MapNorth);
    assert_eq!(manifest.candidate_facts.candidate_projection_count, 0);
    assert_eq!(manifest.candidate_facts.review_decision_count, 0);
    assert_eq!(manifest.candidate_facts.retained_candidate_count, 0);
    assert!(manifest
        .categories
        .iter()
        .all(|category| !category.included));
}

#[test]
fn confirmed_boundary_with_custom_orientation_records_user_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = ExportConsole::new(MockSealGate::new());

    let result = console
        .export_confirmed_boundary(request(
            dir.path(),
            Some(Orientation::new(127.5).unwrap()),
            Some(boundary()),
            true,
        ))
        .expect("自定义朝向只应改变导出事实，不应改变导出资格");

    let manifest = read_manifest(&result.manifest_path);
    let orientation = manifest.orientation.expect("必须记录朝向");
    assert_eq!(orientation.degree, 127.5);
    assert_eq!(orientation.source, ManifestOrientationSource::Custom);
}

#[test]
fn missing_or_unconfirmed_boundary_returns_structured_failure_without_artifacts() {
    for (boundary, confirmed, expected) in [
        (None, true, BoundaryError::Missing),
        (Some(boundary()), false, BoundaryError::NotConfirmed),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut console = ExportConsole::new(MockSealGate::new());
        let error = console
            .export_confirmed_boundary(request(dir.path(), None, boundary, confirmed))
            .expect_err("没有有效且已确认的边界不得导出");

        assert!(matches!(error, Error::Boundary(expected_error) if expected_error == expected));
        assert!(!dir.path().join("campus.schem").exists());
        assert!(!dir.path().join("foundation_manifest.json").exists());
        assert!(!console.progress_view().is_done);
    }
}

#[test]
fn generation_failure_is_explicit_and_does_not_leave_success_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = ExportConsole::new(MockSealGate::new());

    let error = console
        .export_confirmed_boundary(request(dir.path(), None, Some(oversized_boundary()), true))
        .expect_err("B18 材料失败必须向上返回");

    assert!(matches!(error, Error::Generation(_)));
    assert!(!dir.path().join("campus.schem").exists());
    assert!(!dir.path().join("foundation_manifest.json").exists());
    assert!(!console.progress_view().is_done);
}

#[test]
fn unsupported_target_version_is_rejected_without_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = ExportConsole::new(MockSealGate::new());

    let error = console
        .export_confirmed_boundary(BoundaryExportRequest::new(
            ExportPlanContext::new("测试校区", PlanId::generate(), "不支持版本方案", "1.20.4"),
            ExportPlanState::new(Some(boundary()), true, None),
            ExportArtifactTargets::new(
                dir.path().join("campus.schem"),
                dir.path().join("foundation_manifest.json"),
            ),
        ))
        .expect_err("不支持版本不得静默回退到其他用料表");

    assert!(matches!(error, Error::Version(_)));
    assert!(!dir.path().join("campus.schem").exists());
    assert!(!dir.path().join("foundation_manifest.json").exists());
    assert!(!console.progress_view().is_done);
}

#[test]
fn material_table_version_mismatch_is_rejected_even_for_supported_target() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = ExportConsole::new_with_material_table(
        MockSealGate::new(),
        manifest_generator::MaterialTable::v1_20_4_school(),
    );

    let error = console
        .export_confirmed_boundary(request(dir.path(), None, Some(boundary()), true))
        .expect_err("目标版本与用料表版本不一致时必须失败");

    assert!(matches!(error, Error::Version(_)));
    assert!(!dir.path().join("campus.schem").exists());
    assert!(!dir.path().join("foundation_manifest.json").exists());
}

#[test]
fn invalid_material_block_id_is_rejected_before_generation() {
    let dir = tempfile::tempdir().unwrap();
    let mut table = manifest_generator::MaterialTable::v26_1_2_school();
    table.building_presets.school.foundation = "minecraft:not_a_real_block".to_owned();
    let mut console = ExportConsole::new_with_material_table(MockSealGate::new(), table);

    let error = console
        .export_confirmed_boundary(request(dir.path(), None, Some(boundary()), true))
        .expect_err("unknown configured block must be rejected by F9");

    assert!(matches!(
        error,
        Error::Version(VersionError::InvalidMaterialTable { .. })
    ));
    assert!(!dir.path().join("campus.schem").exists());
    assert!(!dir.path().join("foundation_manifest.json").exists());
}
#[test]
fn non_square_custom_orientation_changes_generated_footprint_not_only_manifest() {
    let north_dir = tempfile::tempdir().unwrap();
    let custom_dir = tempfile::tempdir().unwrap();
    let non_square = Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.0000, 39.0000],
            [116.0100, 39.0000],
            [116.0100, 39.0010],
            [116.0000, 39.0010],
            [116.0000, 39.0000]
        ]]),
    };

    let mut north = ExportConsole::new(MockSealGate::new());
    let north_result = north
        .export_confirmed_boundary(request(
            north_dir.path(),
            None,
            Some(non_square.clone()),
            true,
        ))
        .unwrap();
    let north_dimensions = sponge_export::inspect_schematic(&north_result.schematic_path)
        .unwrap()
        .dimensions;

    let mut custom = ExportConsole::new(MockSealGate::new());
    let custom_result = custom
        .export_confirmed_boundary(request(
            custom_dir.path(),
            Some(Orientation::new(127.5).unwrap()),
            Some(non_square),
            true,
        ))
        .unwrap();
    let custom_dimensions = sponge_export::inspect_schematic(&custom_result.schematic_path)
        .unwrap()
        .dimensions;

    assert_ne!(north_dimensions, custom_dimensions);
    assert_eq!(
        read_manifest(&custom_result.manifest_path)
            .orientation
            .unwrap()
            .degree,
        127.5
    );
}
#[test]
fn corrected_boundary_changes_schematic_dimensions() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first_boundary = boundary();
    let second_boundary = Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.0000, 39.0000],
            [116.0050, 39.0000],
            [116.0050, 39.0040],
            [116.0000, 39.0040],
            [116.0000, 39.0000]
        ]]),
    };

    let mut first_console = ExportConsole::new(MockSealGate::new());
    let first_result = first_console
        .export_confirmed_boundary(request(first_dir.path(), None, Some(first_boundary), true))
        .unwrap();
    let first_dimensions = sponge_export::inspect_schematic(&first_result.schematic_path)
        .unwrap()
        .dimensions;

    let mut second_console = ExportConsole::new(MockSealGate::new());
    let second_result = second_console
        .export_confirmed_boundary(request(
            second_dir.path(),
            None,
            Some(second_boundary),
            true,
        ))
        .unwrap();
    let second_dimensions = sponge_export::inspect_schematic(&second_result.schematic_path)
        .unwrap()
        .dimensions;

    assert!(second_dimensions[0] > first_dimensions[0]);
    assert!(second_dimensions[2] > first_dimensions[2]);
}

#[test]
fn multipolygon_exports_all_separated_outer_rings() {
    let dir = tempfile::tempdir().unwrap();
    let first = boundary();
    let multi = Boundary {
        r#type: "MultiPolygon".to_owned(),
        coordinates: serde_json::json!([
            [first.coordinates[0].clone()],
            [[
                [116.0200, 39.0200],
                [116.0210, 39.0200],
                [116.0210, 39.0210],
                [116.0200, 39.0210],
                [116.0200, 39.0200]
            ]]
        ]),
    };
    let mut console = ExportConsole::new(MockSealGate::new());

    let result = console
        .export_confirmed_boundary(request(dir.path(), None, Some(multi), true))
        .expect("MultiPolygon 的所有分片都应参与边界直出");
    let inspection = sponge_export::inspect_schematic(&result.schematic_path).unwrap();

    assert_eq!(result.schematic_dimensions, inspection.dimensions);
    assert!(inspection.dimensions[0] > 1_000);
    assert!(inspection.dimensions[2] > 1_000);
}
