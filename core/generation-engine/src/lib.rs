//! B18 初始校园生成引擎（ADR-0024）。
//!
//! 职责：输入评审保留数据，输出纯内存方块模型；承载全部生成规则（Arnis 建筑
//! 规则、道路/水域/植被/体育/其他生成规则）与用料表版本绑定查询。
//!
//! 窗口契约：缝 6（F9 → B18 → B4）。B18 产出 `BlockModel` 后由 F9 交给 B4
//! 落成 .schem——两模块互不依赖，生成规则可脱离文件 IO 独立测试。
//!
//! # 架构纪律
//! - 零文件 IO、零数据库访问、零 UI 耦合；
//! - 只依赖 B1（shared-domain-types）与 B17（manifest-generator，只读调用）；
//! - 用料表版本强绑定：查不到目标版本的方块直接报错，禁止静默替换。

pub mod building;
mod materials;
mod model;
pub mod rules;

pub use building::{build_model, estimate_height, normalize_roof_shape, BuildingCandidate};
pub use materials::{MaterialError, MaterialRole, MaterialsAdapter};
pub use model::{Block, BlockModel, BlockPosition, BoundingBox};
pub use rules::{
    generate_flat_ground, generate_other, generate_other_rail, generate_road,
    generate_sports_court, generate_vegetation, generate_water, AreaInput, BuildingGenerator,
    GenerationError, Generator, OtherCandidate, OtherGenerator, RoadGenerator, SportsGenerator,
    VegetationGenerator, WaterGenerator,
};

/// 生成引擎门面：持有版本绑定的用料适配器，按类别派发生成规则。
///
/// F9 用全局 MC 版本对应的用料表建一个引擎实例，之后逐候选调用。
pub struct GenerationEngine {
    adapter: MaterialsAdapter,
}

impl GenerationEngine {
    /// 用某个 MC 版本的用料表创建引擎（表由 B17 提供）。
    pub fn new(table: manifest_generator::MaterialTable) -> Self {
        Self {
            adapter: MaterialsAdapter::new(table),
        }
    }

    /// 当前引擎绑定的 MC 版本。
    pub fn version(&self) -> manifest_generator::MinecraftVersion {
        self.adapter.version()
    }

    /// 用料适配器（供直接调用类别生成函数时复用同一份版本绑定）。
    pub fn materials(&self) -> &MaterialsAdapter {
        &self.adapter
    }

    /// 建筑：按 Arnis 规则自动起楼。
    pub fn generate_building(
        &self,
        candidate: &BuildingCandidate,
    ) -> Result<BlockModel, GenerationError> {
        building::build_model(candidate, &self.adapter)
    }

    /// 生成边界直出所需的最小平整场地。
    pub fn generate_flat_ground(
        &self,
        width_blocks: usize,
        length_blocks: usize,
    ) -> Result<BlockModel, GenerationError> {
        let width = i32::try_from(width_blocks).map_err(|_| {
            GenerationError::InvalidParameter("平整场地宽度超出生成引擎范围".to_owned())
        })?;
        let length = i32::try_from(length_blocks).map_err(|_| {
            GenerationError::InvalidParameter("平整场地长度超出生成引擎范围".to_owned())
        })?;
        generate_flat_ground(width, length, &self.adapter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_binds_material_table_version() {
        let engine = GenerationEngine::new(manifest_generator::MaterialTable::v1_20_4_school());
        assert_eq!(
            engine.version(),
            manifest_generator::MinecraftVersion::V1_20_4
        );
    }

    #[test]
    fn engine_generates_building_via_facade() {
        let engine = GenerationEngine::new(manifest_generator::MaterialTable::v1_20_4_school());
        let candidate = BuildingCandidate::new("b", 10, 10).with_height_m(15.0);
        let model = engine.generate_building(&candidate).unwrap();
        assert_eq!(model.bounding_box().unwrap().height(), 15);
    }

    #[test]
    fn engine_generates_boundary_ground_via_facade() {
        let engine = GenerationEngine::new(manifest_generator::MaterialTable::v1_20_4_school());
        let model = engine.generate_flat_ground(3, 2).unwrap();
        assert_eq!(model.block_count(), 6);
        assert!(model.contains_block_id("minecraft:stone_bricks"));
    }
}
