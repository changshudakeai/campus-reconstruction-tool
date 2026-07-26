//! 建筑生成规则 —— Arnis 血统迁移（ADR-0024，血统见 v1.x arnis-rule-lineage.md）。
//!
//! 核心规则：
//! - `arnis-explicit-height-overrides-levels`：height 标注优先于层数；
//! - `arnis-levels-to-height`：无 height 时按"层数 × 4 + 2"估算；
//! - `arnis-roof-shape-synonyms` / `arnis-default-flat-roof`：屋顶同义词
//!   规范化，无标注一律平顶；
//! - 自动起楼：地基 → 外墙（带窗）→ 层间楼板 → 屋顶 → 入口门。
//!
//! 零文件 IO：纯内存 `BlockModel` 落笔，落盘 .schem 是 B4 的事（缝 6）。

use crate::rules::GenerationError;
use crate::{BlockModel, BlockPosition, MaterialRole, MaterialsAdapter};

/// 建筑候选参数（B18 自有输入结构，由 F9 从评审保留数据装配）。
#[derive(Debug, Clone)]
pub struct BuildingCandidate {
    /// 候选 ID（溯源用，不参与几何计算）。
    pub id: String,
    /// 数据源 height 标注（米，1 米 = 1 格）；有值时优先。
    pub height_m: Option<f64>,
    /// 数据源楼层数标注；无 height 时按"层数 × 4 + 2"估算。
    pub levels: Option<u32>,
    /// 数据源屋顶形状标注（如 gabled/hipped）；无标注按平顶。
    pub roof_shape: Option<String>,
    /// 底面宽（格，x 方向，≥3）。
    pub width_blocks: i32,
    /// 底面长（格，z 方向，≥3）。
    pub length_blocks: i32,
}

impl BuildingCandidate {
    pub fn new(id: impl Into<String>, width_blocks: i32, length_blocks: i32) -> Self {
        Self {
            id: id.into(),
            height_m: None,
            levels: None,
            roof_shape: None,
            width_blocks,
            length_blocks,
        }
    }

    pub fn with_height_m(mut self, height_m: f64) -> Self {
        self.height_m = Some(height_m);
        self
    }

    pub fn with_levels(mut self, levels: u32) -> Self {
        self.levels = Some(levels);
        self
    }

    pub fn with_roof_shape(mut self, shape: impl Into<String>) -> Self {
        self.roof_shape = Some(shape.into());
        self
    }
}

/// 楼高估算（格）：height 标注优先；无则"层数 × 4 + 2"（默认 3 层）。
pub fn estimate_height(candidate: &BuildingCandidate) -> i32 {
    let levels = candidate.levels.unwrap_or(3).clamp(1, 64) as i32;
    candidate
        .height_m
        .map(|height| height.ceil() as i32)
        .unwrap_or(levels * 4 + 2)
        .max(4)
}

/// 屋顶形状规范化：同义词归入标准形，未识别/未标注一律平顶。
pub fn normalize_roof_shape(shape: Option<&str>) -> &'static str {
    match shape.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("gable" | "gabled") => "gabled",
        Some("hip" | "hipped") => "hipped",
        Some("pyramid" | "pyramidal") => "pyramidal",
        Some("skillion") => "skillion",
        Some("dome") => "dome",
        Some("cone") => "cone",
        Some("onion") => "onion",
        _ => "flat",
    }
}

/// 屋顶起始高度：从总高里给屋顶留出层数（沿 v1.x roof_base_height 思路）。
fn roof_base(total_height: i32, roof_shape: &str) -> i32 {
    let reserve = match roof_shape {
        "flat" => 2,
        "dome" | "cone" | "onion" => (total_height / 3).clamp(3, 8),
        _ => (total_height / 4).clamp(2, 6),
    };
    (total_height - reserve).max(1)
}

/// 判定外墙格是否开窗：非角柱、离地 ≥2 格、按层带 + 奇偶节奏分布。
fn is_window_cell(x: i32, y: i32, z: i32, w: i32, l: i32) -> bool {
    let corner = (x == 0 || x == w - 1) && (z == 0 || z == l - 1);
    !corner && y >= 2 && y % 3 != 0 && (x + z) % 2 == 0
}

/// 自动起楼：输入候选 + 用料适配器，产出纯内存方块模型。
///
/// 模型总高恰为 `estimate_height`（y ∈ [0, 高度-1]），屋顶不超出总高。
pub fn build_model(
    candidate: &BuildingCandidate,
    materials: &MaterialsAdapter,
) -> Result<BlockModel, GenerationError> {
    let w = candidate.width_blocks;
    let l = candidate.length_blocks;
    if w < 3 || l < 3 {
        return Err(GenerationError::InvalidParameter(format!(
            "建筑底面至少 3x3 格，收到 {w}x{l}"
        )));
    }

    // 用料七件套：任何一个在目标版本查不到都会在这里报错终止（ADR-0024）。
    let foundation = materials.block_for(MaterialRole::BuildingFoundation)?;
    let wall = materials.block_for(MaterialRole::BuildingWall)?;
    let window = materials.block_for(MaterialRole::BuildingWindow)?;
    let floor = materials.block_for(MaterialRole::BuildingFloor)?;
    let roof = materials.block_for(MaterialRole::BuildingRoof)?;
    let entrance = materials.block_for(MaterialRole::BuildingEntrance)?;

    let height = estimate_height(candidate);
    let shape = normalize_roof_shape(candidate.roof_shape.as_deref());
    let roof_start = roof_base(height, shape);

    let mut model = BlockModel::new();

    // 1) 地基：y=0 全底面。
    for x in 0..w {
        for z in 0..l {
            model.set_block(BlockPosition::new(x, 0, z), &foundation);
        }
    }

    // 2) 主体：外墙带窗，每 4 格一层楼板。
    for y in 1..roof_start {
        for x in 0..w {
            for z in 0..l {
                let on_boundary = x == 0 || x == w - 1 || z == 0 || z == l - 1;
                if on_boundary {
                    let block = if is_window_cell(x, y, z, w, l) {
                        &window
                    } else {
                        &wall
                    };
                    model.set_block(BlockPosition::new(x, y, z), block);
                } else if y % 4 == 0 {
                    model.set_block(BlockPosition::new(x, y, z), &floor);
                }
            }
        }
    }

    // 3) 屋顶：从 roof_start 铺到总高顶（含 y = height-1），不超出总高。
    for y in roof_start..height {
        for x in 0..w {
            for z in 0..l {
                let layer = y - roof_start;
                let covered = match shape {
                    // 平顶：整层铺满。
                    "flat" => true,
                    // 单坡：沿 x 方向逐层缩进一侧。
                    "skillion" => x >= layer.min(w - 1),
                    // 其余形状：逐层四边收拢（近似坡顶/穹顶轮廓）。
                    _ => {
                        x >= layer.min(w / 2)
                            && x < w - layer.min(w / 2).min(w - 1)
                            && z >= layer.min(l / 2)
                            && z < l - layer.min(l / 2).min(l - 1)
                    }
                };
                if covered {
                    model.set_block(BlockPosition::new(x, y, z), &roof);
                }
            }
        }
    }

    // 4) 入口门：南侧（z 最大）中央，两格高。
    let door_x = w / 2;
    for y in 1..=2 {
        model.set_block(BlockPosition::new(door_x, y, l - 1), &entrance);
    }

    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifest_generator::MaterialTable;

    fn adapter() -> MaterialsAdapter {
        MaterialsAdapter::new(MaterialTable::v1_20_4_school())
    }

    #[test]
    fn explicit_height_overrides_levels() {
        let candidate = BuildingCandidate::new("b", 10, 10)
            .with_height_m(15.0)
            .with_levels(2);
        assert_eq!(estimate_height(&candidate), 15);
    }

    #[test]
    fn levels_times_four_plus_two_when_height_missing() {
        assert_eq!(
            estimate_height(&BuildingCandidate::new("b", 10, 10).with_levels(3)),
            14
        );
        assert_eq!(
            estimate_height(&BuildingCandidate::new("b", 10, 10).with_levels(5)),
            22
        );
    }

    #[test]
    fn default_three_levels_when_nothing_annotated() {
        assert_eq!(estimate_height(&BuildingCandidate::new("b", 10, 10)), 14);
    }

    #[test]
    fn roof_synonyms_normalize_and_default_is_flat() {
        assert_eq!(normalize_roof_shape(Some("gable")), "gabled");
        assert_eq!(normalize_roof_shape(Some("HIP")), "hipped");
        assert_eq!(normalize_roof_shape(Some("thatched")), "flat");
        assert_eq!(normalize_roof_shape(None), "flat");
    }

    #[test]
    fn model_height_equals_estimated_height() {
        let candidate = BuildingCandidate::new("b", 10, 10).with_height_m(15.0);
        let model = build_model(&candidate, &adapter()).unwrap();
        assert_eq!(model.bounding_box().unwrap().height(), 15);
    }

    #[test]
    fn footprint_smaller_than_3x3_is_rejected() {
        let err = build_model(&BuildingCandidate::new("b", 2, 10), &adapter()).unwrap_err();
        assert!(matches!(err, GenerationError::InvalidParameter(_)));
    }
}
