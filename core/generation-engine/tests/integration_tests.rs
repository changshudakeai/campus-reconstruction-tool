//! B18 集成测试 —— 工单 T10 测试决定章的四项要求，全部脱离文件 IO。
//!
//! 1. 给定带 height=15 的建筑候选 → 断言楼高 15；
//! 2. 给定 levels=3 无 height → 断言按 3×4+2=14 估算；
//! 3. 用料表版本绑定：请求目标版本不存在的方块 → 报错而非替换；
//! 4. 六类生成规则均产出纯内存方块模型。

use generation_engine::{
    AreaInput, BuildingCandidate, BuildingGenerator, GenerationEngine, GenerationError, Generator,
    MaterialsAdapter, OtherCandidate, OtherGenerator, RoadGenerator, SportsGenerator,
    VegetationGenerator, WaterGenerator,
};
use manifest_generator::MaterialTable;
use shared_domain_types::CandidateCategory;

fn engine() -> GenerationEngine {
    GenerationEngine::new(MaterialTable::v1_20_4_school())
}

fn adapter() -> MaterialsAdapter {
    MaterialsAdapter::new(MaterialTable::v1_20_4_school())
}

#[test]
fn building_with_height_15_is_15_blocks_tall() {
    let candidate = BuildingCandidate::new("b-height", 10, 10).with_height_m(15.0);
    let model = engine().generate_building(&candidate).unwrap();
    assert_eq!(model.bounding_box().unwrap().height(), 15);
}

#[test]
fn building_with_3_levels_is_estimated_at_14_blocks() {
    let candidate = BuildingCandidate::new("b-levels", 10, 10).with_levels(3);
    let model = engine().generate_building(&candidate).unwrap();
    // 3 层 × 4 + 2 = 14（arnis-levels-to-height，无 height 标注时生效）
    assert_eq!(model.bounding_box().unwrap().height(), 14);
}

#[test]
fn building_auto_construction_has_walls_windows_and_roof() {
    let candidate = BuildingCandidate::new("b-full", 10, 10).with_height_m(15.0);
    let model = engine().generate_building(&candidate).unwrap();
    // school 预设（Arnis/v1.x 默认用料）：墙=bricks，窗=glass_pane，顶=dark_oak_slab
    assert!(model.contains_block_id("minecraft:bricks"), "应有墙");
    assert!(model.contains_block_id("minecraft:glass_pane"), "应有窗");
    assert!(
        model.contains_block_id("minecraft:dark_oak_slab"),
        "应有屋顶"
    );
    assert!(
        model.contains_block_id("minecraft:dark_oak_door"),
        "应有入口门"
    );
}

#[test]
fn missing_block_in_target_version_aborts_instead_of_substituting() {
    // minecraft:crafter 只在 1.21+ 存在；1.20.4 的适配器必须报错。
    let err = adapter().validate_block("minecraft:crafter").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("1.20.4"), "错误应报出目标版本：{message}");

    // 经由生成错误转换后仍是"材料不可用"，供 F9 按弹窗铁律分派。
    let generation_err: GenerationError = err.into();
    assert!(matches!(
        generation_err,
        GenerationError::MaterialNotAvailable(_)
    ));
}

#[test]
fn all_six_generators_declare_their_category_and_produce_models() {
    let mat = adapter();
    let area = AreaInput {
        width_blocks: 10,
        length_blocks: 5,
    };

    let building = BuildingGenerator;
    assert_eq!(building.category(), CandidateCategory::Building);
    let candidate = BuildingCandidate::new("b", 10, 10).with_levels(3);
    assert!(!building.generate(&candidate, &mat).unwrap().is_empty());

    let road = RoadGenerator;
    assert_eq!(road.category(), CandidateCategory::Road);
    assert!(!road.generate(&area, &mat).unwrap().is_empty());

    let water = WaterGenerator;
    assert_eq!(water.category(), CandidateCategory::Water);
    assert!(!water.generate(&area, &mat).unwrap().is_empty());

    let vegetation = VegetationGenerator;
    assert_eq!(vegetation.category(), CandidateCategory::Vegetation);
    assert!(!vegetation.generate(&(), &mat).unwrap().is_empty());

    let sports = SportsGenerator;
    assert_eq!(sports.category(), CandidateCategory::Sports);
    assert!(!sports.generate(&area, &mat).unwrap().is_empty());

    let other = OtherGenerator;
    assert_eq!(other.category(), CandidateCategory::Other);
    let mut rail = OtherCandidate::new("rail");
    rail.tags.insert("railway".to_string(), "rail".to_string());
    let model = other.generate(&rail, &mat).unwrap();
    assert!(
        model.contains_block_id("minecraft:rail"),
        "铁路家族应生成铁轨"
    );
}
