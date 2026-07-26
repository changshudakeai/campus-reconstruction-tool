//! 公开 API 快照测试（执法清单 2.5）。
//!
//! 任何公开类型的增删都会反映在此快照中，PR diff 可见。
//!
//! 简单方式：只检查所有公开类型都可引用并实例化。

use generation_engine::Generator;

#[test]
fn public_api_types_exist() {
    // GenerationEngine 门面
    let table = manifest_generator::MaterialTable::v1_20_4_school();
    let engine = generation_engine::GenerationEngine::new(table);
    assert_eq!(
        engine.version(),
        manifest_generator::MinecraftVersion::V1_20_4
    );
    let _adapter: &generation_engine::MaterialsAdapter = engine.materials();

    // BuildingCandidate 构造器链 + 高度规则
    let candidate = generation_engine::BuildingCandidate::new("id", 10, 10)
        .with_levels(3)
        .with_roof_shape("gabled");
    assert_eq!(generation_engine::estimate_height(&candidate), 14); // 3×4+2
    let with_height = candidate.clone().with_height_m(15.0);
    assert_eq!(generation_engine::estimate_height(&with_height), 15); // height 优先

    // 屋顶形状规范化
    assert_eq!(
        generation_engine::normalize_roof_shape(Some("gable")),
        "gabled"
    );

    // BlockModel / BlockPosition / Block / BoundingBox
    let mut model = generation_engine::BlockModel::new();
    model.set_block(
        generation_engine::BlockPosition::new(0, 0, 0),
        "minecraft:stone_bricks",
    );
    assert!(!model.is_empty());
    let _bb: generation_engine::BoundingBox = model.bounding_box().unwrap();
    let _blocks: Vec<generation_engine::Block> = model.blocks().collect();

    // MaterialRole 与错误类型
    let _wall = generation_engine::MaterialRole::BuildingWall;
    let _material_err = generation_engine::MaterialError::MaterialUnavailable("x".into());
    let _generation_err = generation_engine::GenerationError::MaterialNotAvailable("x".into());

    // 六类生成器接口（Generator trait 的六个实现）
    assert_eq!(
        BuildingLikeCategory::of(&generation_engine::BuildingGenerator),
        shared_domain_types::CandidateCategory::Building
    );
    assert_eq!(
        BuildingLikeCategory::of(&generation_engine::RoadGenerator),
        shared_domain_types::CandidateCategory::Road
    );
    assert_eq!(
        BuildingLikeCategory::of(&generation_engine::WaterGenerator),
        shared_domain_types::CandidateCategory::Water
    );
    assert_eq!(
        BuildingLikeCategory::of(&generation_engine::VegetationGenerator),
        shared_domain_types::CandidateCategory::Vegetation
    );
    assert_eq!(
        BuildingLikeCategory::of(&generation_engine::SportsGenerator),
        shared_domain_types::CandidateCategory::Sports
    );
    assert_eq!(
        BuildingLikeCategory::of(&generation_engine::OtherGenerator),
        shared_domain_types::CandidateCategory::Other
    );
    let _area = generation_engine::AreaInput {
        width_blocks: 1,
        length_blocks: 1,
    };
    let _other = generation_engine::OtherCandidate::new("id");
}

/// 辅助：统一读取任意 Generator 实现声明的类别。
struct BuildingLikeCategory;

impl BuildingLikeCategory {
    fn of<G: Generator>(generator: &G) -> shared_domain_types::CandidateCategory {
        generator.category()
    }
}
