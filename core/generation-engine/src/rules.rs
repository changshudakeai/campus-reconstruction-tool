//! 六类生成规则 —— 建筑之外的五类最简可用版（ADR-0024）。
//!
//! 建筑为完整实现（见 `building` 模块）；道路（铺面）、水域（水面）、
//! 植被（单树）、体育（场地铺面）为最简可用版；"其他"按标签家族生成
//! （铁路 → 铁轨，家族规则逐步补齐，ADR-0011）。
//!
//! 所有生成器只向 `MaterialsAdapter` 要方块（版本校验在那一层），
//! 输出纯内存 `BlockModel`，零文件 IO。

use shared_domain_types::CandidateCategory;

use crate::building::BuildingCandidate;
use crate::{BlockModel, BlockPosition, MaterialError, MaterialRole, MaterialsAdapter};

/// 生成错误（窗口契约章：错误是带类型的值一路向上传递，由功能层分派弹窗）。
#[derive(Debug, thiserror::Error)]
pub enum GenerationError {
    /// 用料表查不到目标版本的方块——终止生成，绝不静默替换（ADR-0024）。
    #[error("材料不可用：{0}")]
    MaterialNotAvailable(String),

    /// 候选参数非法（尺寸/坐标越界等）。
    #[error("参数无效：{0}")]
    InvalidParameter(String),

    /// "其他"类候选的标签家族尚无生成规则（规则逐步补齐，ADR-0011）。
    #[error("不支持的标签家族：{0}")]
    UnsupportedTagFamily(String),
}

impl From<MaterialError> for GenerationError {
    fn from(err: MaterialError) -> Self {
        Self::MaterialNotAvailable(err.to_string())
    }
}

/// "其他"类候选：携带原始标签，按标签家族路由生成规则。
#[derive(Debug, Clone, Default)]
pub struct OtherCandidate {
    pub id: String,
    pub tags: std::collections::HashMap<String, String>,
}

impl OtherCandidate {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            tags: Default::default(),
        }
    }

    pub fn has_tag(&self, key: &str) -> bool {
        self.tags.keys().any(|k| k.eq_ignore_ascii_case(key))
    }
}

/// 在 y=0 平面铺一块 w × l 的矩形（道路/水面/场地共用的落笔方式）。
fn fill_flat_rect(
    role: MaterialRole,
    w: i32,
    l: i32,
    mat: &MaterialsAdapter,
) -> Result<BlockModel, GenerationError> {
    if w <= 0 || l <= 0 {
        return Err(GenerationError::InvalidParameter(format!(
            "矩形尺寸必须为正：{w}x{l}"
        )));
    }
    let block = mat.block_for(role)?;
    let mut model = BlockModel::new();
    for x in 0..w {
        for z in 0..l {
            model.set_block(BlockPosition::new(x, 0, z), &block);
        }
    }
    Ok(model)
}

/// 道路：初始铺面。
pub fn generate_road(
    w: i32,
    l: i32,
    mat: &MaterialsAdapter,
) -> Result<BlockModel, GenerationError> {
    fill_flat_rect(MaterialRole::Road, w, l, mat)
}

/// 水域：初始水面。
pub fn generate_water(
    w: i32,
    l: i32,
    mat: &MaterialsAdapter,
) -> Result<BlockModel, GenerationError> {
    fill_flat_rect(MaterialRole::Water, w, l, mat)
}

/// 植被：单棵树（树干 3 格 + 树叶簇）。
pub fn generate_vegetation(mat: &MaterialsAdapter) -> Result<BlockModel, GenerationError> {
    let trunk = mat.block_for(MaterialRole::TreeTrunk)?;
    let leaves = mat.block_for(MaterialRole::TreeLeaves)?;
    let mut model = BlockModel::new();
    for y in 0..3 {
        model.set_block(BlockPosition::new(0, y, 0), &trunk);
    }
    for dy in 0..2 {
        for dx in -1..2 {
            for dz in -1..2 {
                if dx == 0 && dz == 0 && dy == 0 {
                    continue;
                }
                model.set_block(BlockPosition::new(dx, 3 + dy, dz), &leaves);
            }
        }
    }
    Ok(model)
}

/// 体育：场地铺面 + 中线划线。
pub fn generate_sports_court(
    w: i32,
    l: i32,
    mat: &MaterialsAdapter,
) -> Result<BlockModel, GenerationError> {
    let mut model = fill_flat_rect(MaterialRole::SportsCourt, w, l, mat)?;
    let line = mat.block_for(MaterialRole::SportsLine)?;
    let mid_x = w / 2;
    for z in 0..l {
        model.set_block(BlockPosition::new(mid_x, 0, z), &line);
    }
    Ok(model)
}

/// "其他"·铁路家族：一段直线铁轨。
pub fn generate_other_rail(
    length: i32,
    mat: &MaterialsAdapter,
) -> Result<BlockModel, GenerationError> {
    fill_flat_rect(MaterialRole::OtherRail, length, 1, mat)
}

/// "其他"类入口：按标签家族路由；无规则的家族如实报错，禁止静默丢弃。
pub fn generate_other(
    candidate: &OtherCandidate,
    mat: &MaterialsAdapter,
) -> Result<BlockModel, GenerationError> {
    if candidate.has_tag("railway") {
        return generate_other_rail(10, mat);
    }
    let families: Vec<&str> = candidate.tags.keys().map(String::as_str).collect();
    Err(GenerationError::UnsupportedTagFamily(families.join(",")))
}

// ===== 六类生成器接口（工单验收项：BuildingGenerator, RoadGenerator ...）=====

/// 生成器统一接口：每类一个实现，声明自己负责的类别（T02 六类别）
/// 并把自己的候选输入变成方块模型。数据源适配器那样的开放扩展不适用
/// 于此：六类别是产品铁律（ADR-0011），故实现集就是这六个。
pub trait Generator {
    /// 本生成器接受的候选输入类型。
    type Input;

    /// 本生成器负责的候选类别。
    fn category(&self) -> CandidateCategory;

    /// 把一个评审保留候选变成纯内存方块模型。
    fn generate(
        &self,
        input: &Self::Input,
        materials: &MaterialsAdapter,
    ) -> Result<BlockModel, GenerationError>;
}

/// 面状候选的尺寸输入（道路/水域/体育共用）。
#[derive(Debug, Clone, Copy)]
pub struct AreaInput {
    pub width_blocks: i32,
    pub length_blocks: i32,
}

/// 建筑生成器（完整实现：Arnis 规则自动起楼）。
pub struct BuildingGenerator;

impl Generator for BuildingGenerator {
    type Input = BuildingCandidate;

    fn category(&self) -> CandidateCategory {
        CandidateCategory::Building
    }

    fn generate(
        &self,
        input: &BuildingCandidate,
        materials: &MaterialsAdapter,
    ) -> Result<BlockModel, GenerationError> {
        crate::building::build_model(input, materials)
    }
}

/// 道路生成器（最简版：初始铺面）。
pub struct RoadGenerator;

impl Generator for RoadGenerator {
    type Input = AreaInput;

    fn category(&self) -> CandidateCategory {
        CandidateCategory::Road
    }

    fn generate(
        &self,
        input: &AreaInput,
        materials: &MaterialsAdapter,
    ) -> Result<BlockModel, GenerationError> {
        generate_road(input.width_blocks, input.length_blocks, materials)
    }
}

/// 水域生成器（最简版：初始水面）。
pub struct WaterGenerator;

impl Generator for WaterGenerator {
    type Input = AreaInput;

    fn category(&self) -> CandidateCategory {
        CandidateCategory::Water
    }

    fn generate(
        &self,
        input: &AreaInput,
        materials: &MaterialsAdapter,
    ) -> Result<BlockModel, GenerationError> {
        generate_water(input.width_blocks, input.length_blocks, materials)
    }
}

/// 植被生成器（最简版：单棵树，无需额外输入）。
pub struct VegetationGenerator;

impl Generator for VegetationGenerator {
    type Input = ();

    fn category(&self) -> CandidateCategory {
        CandidateCategory::Vegetation
    }

    fn generate(
        &self,
        _input: &(),
        materials: &MaterialsAdapter,
    ) -> Result<BlockModel, GenerationError> {
        generate_vegetation(materials)
    }
}

/// 体育生成器（最简版：场地铺面 + 中线划线）。
pub struct SportsGenerator;

impl Generator for SportsGenerator {
    type Input = AreaInput;

    fn category(&self) -> CandidateCategory {
        CandidateCategory::Sports
    }

    fn generate(
        &self,
        input: &AreaInput,
        materials: &MaterialsAdapter,
    ) -> Result<BlockModel, GenerationError> {
        generate_sports_court(input.width_blocks, input.length_blocks, materials)
    }
}

/// "其他"生成器（按标签家族路由，家族规则逐步补齐）。
pub struct OtherGenerator;

impl Generator for OtherGenerator {
    type Input = OtherCandidate;

    fn category(&self) -> CandidateCategory {
        CandidateCategory::Other
    }

    fn generate(
        &self,
        input: &OtherCandidate,
        materials: &MaterialsAdapter,
    ) -> Result<BlockModel, GenerationError> {
        generate_other(input, materials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifest_generator::MaterialTable;

    fn adapter() -> MaterialsAdapter {
        MaterialsAdapter::new(MaterialTable::v1_20_4_school())
    }

    #[test]
    fn road_water_sports_are_flat_surfaces() {
        for model in [
            generate_road(10, 5, &adapter()).unwrap(),
            generate_water(8, 8, &adapter()).unwrap(),
            generate_sports_court(10, 5, &adapter()).unwrap(),
        ] {
            let bb = model.bounding_box().expect("铺面非空");
            assert_eq!(bb.height(), 1, "初始铺面应只占一层");
        }
    }

    #[test]
    fn sports_court_has_line_markings() {
        let model = generate_sports_court(10, 5, &adapter()).unwrap();
        assert!(model.contains_block_id("minecraft:white_concrete"));
        assert!(model.contains_block_id("minecraft:red_concrete"));
    }

    #[test]
    fn vegetation_tree_has_trunk_and_leaves() {
        let model = generate_vegetation(&adapter()).unwrap();
        assert!(model.contains_block_id("minecraft:oak_log"));
        assert!(model.contains_block_id("minecraft:oak_leaves"));
    }

    #[test]
    fn other_railway_family_generates_rails() {
        let mut candidate = OtherCandidate::new("rail-1");
        candidate
            .tags
            .insert("railway".to_string(), "rail".to_string());
        let model = generate_other(&candidate, &adapter()).unwrap();
        assert!(model.contains_block_id("minecraft:rail"));
    }

    #[test]
    fn other_unknown_family_errors_instead_of_silently_dropping() {
        let mut candidate = OtherCandidate::new("statue-1");
        candidate
            .tags
            .insert("artwork_type".to_string(), "statue".to_string());
        let err = generate_other(&candidate, &adapter()).unwrap_err();
        assert!(matches!(err, GenerationError::UnsupportedTagFamily(_)));
    }

    #[test]
    fn invalid_rect_size_is_rejected() {
        let err = generate_road(0, 5, &adapter()).unwrap_err();
        assert!(matches!(err, GenerationError::InvalidParameter(_)));
    }
}
