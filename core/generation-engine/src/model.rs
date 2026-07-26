//! 纯内存方块模型 —— B18 的唯一输出形态（缝 6）。
//!
//! B18 是"设计师"：只算"哪些方块摆在哪"，产出本模块定义的内存结构，
//! 不碰文件 IO；落盘成 .schem 是 B4 的事，两模块互不依赖，由 F9 居中适配。

use std::collections::BTreeMap;

/// 模型内一个方块的位置（方案局部坐标，y 向上）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPosition {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// 一个已定位的方块：位置 + 方块 ID（如 `minecraft:bricks`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub position: BlockPosition,
    pub block_id: String,
}

impl Block {
    pub fn new(position: BlockPosition, block_id: impl Into<String>) -> Self {
        Self {
            position,
            block_id: block_id.into(),
        }
    }
}

/// 模型包围盒（含端点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBox {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

impl BoundingBox {
    /// x 方向尺寸（格）。
    pub fn width(&self) -> u32 {
        (self.max_x - self.min_x + 1) as u32
    }

    /// y 方向尺寸（格）——建筑"楼高"即此值。
    pub fn height(&self) -> u32 {
        (self.max_y - self.min_y + 1) as u32
    }

    /// z 方向尺寸（格）。
    pub fn length(&self) -> u32 {
        (self.max_z - self.min_z + 1) as u32
    }
}

/// 纯内存方块模型：一次生成的全部非空气方块 + 包围盒 + 调色板。
///
/// 相同坐标后写覆盖先写（生成器按 地基 → 墙体 → 屋顶 的顺序落笔）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockModel {
    blocks: BTreeMap<BlockPosition, String>,
}

impl BlockModel {
    /// 建一个空模型。
    pub fn new() -> Self {
        Self {
            blocks: BTreeMap::new(),
        }
    }

    /// 落一个方块（同位重写按后写为准）。
    pub fn set_block(&mut self, position: BlockPosition, block_id: impl Into<String>) {
        self.blocks.insert(position, block_id.into());
    }

    /// 非空气方块数。
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// 模型是否为空。
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// 按坐标序迭代全部方块。
    pub fn blocks(&self) -> impl Iterator<Item = Block> + '_ {
        self.blocks
            .iter()
            .map(|(position, id)| Block::new(*position, id.clone()))
    }

    /// 用到的方块 ID 去重列表（字典序，保证确定性）。
    pub fn palette(&self) -> Vec<String> {
        let mut palette: Vec<String> = self
            .blocks
            .values()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        palette.sort();
        palette
    }

    /// 包围盒；空模型返回 `None`。
    pub fn bounding_box(&self) -> Option<BoundingBox> {
        if self.blocks.is_empty() {
            return None;
        }
        let mut positions = self.blocks.keys();
        let first = positions.next().copied().expect("非空模型必有方块");
        let init = BoundingBox {
            min_x: first.x,
            min_y: first.y,
            min_z: first.z,
            max_x: first.x,
            max_y: first.y,
            max_z: first.z,
        };
        Some(self.blocks.keys().fold(init, |acc, p| BoundingBox {
            min_x: acc.min_x.min(p.x),
            min_y: acc.min_y.min(p.y),
            min_z: acc.min_z.min(p.z),
            max_x: acc.max_x.max(p.x),
            max_y: acc.max_y.max(p.y),
            max_z: acc.max_z.max(p.z),
        }))
    }

    /// 是否含有指定方块 ID（测试断言"有墙、有窗、有屋顶"用）。
    pub fn contains_block_id(&self, block_id: &str) -> bool {
        self.blocks.values().any(|id| id == block_id)
    }
}

impl Default for BlockModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounding_box_measures_all_axes() {
        let mut model = BlockModel::new();
        model.set_block(BlockPosition::new(0, 0, 0), "minecraft:stone_bricks");
        model.set_block(BlockPosition::new(9, 14, 4), "minecraft:bricks");

        let bb = model.bounding_box().expect("非空模型");
        assert_eq!(bb.width(), 10);
        assert_eq!(bb.height(), 15);
        assert_eq!(bb.length(), 5);
    }

    #[test]
    fn same_position_is_overwritten_by_later_write() {
        let mut model = BlockModel::new();
        let p = BlockPosition::new(1, 1, 1);
        model.set_block(p, "minecraft:stone");
        model.set_block(p, "minecraft:bricks");

        assert_eq!(model.block_count(), 1);
        assert!(model.contains_block_id("minecraft:bricks"));
        assert!(!model.contains_block_id("minecraft:stone"));
    }

    #[test]
    fn palette_is_deterministic_and_deduplicated() {
        let mut model = BlockModel::new();
        model.set_block(BlockPosition::new(0, 0, 0), "minecraft:bricks");
        model.set_block(BlockPosition::new(1, 0, 0), "minecraft:bricks");
        model.set_block(BlockPosition::new(2, 0, 0), "minecraft:glass_pane");

        assert_eq!(
            model.palette(),
            vec![
                "minecraft:bricks".to_string(),
                "minecraft:glass_pane".to_string()
            ]
        );
    }

    #[test]
    fn empty_model_has_no_bounding_box() {
        assert!(BlockModel::new().bounding_box().is_none());
    }
}
