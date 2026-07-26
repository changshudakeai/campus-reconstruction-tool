//! 用料表配置结构（按 MC 版本区分）
//!
//! 依据 ADR-0024：用料表与 MC 版本强绑定，只准用目标版本存在的方块。
//! 默认用料沿 v1.x Arnis 规则（见 `arnis-rule-lineage.md`）。

use serde::{Deserialize, Serialize};

/// Minecraft 版本枚举（支持的受控版本列表）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinecraftVersion {
    /// 1.20.4 (推荐)
    V1_20_4,
    /// 1.20.2
    V1_20_2,
    /// 1.21
    V1_21,
}

impl std::fmt::Display for MinecraftVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V1_20_4 => write!(f, "1.20.4"),
            Self::V1_20_2 => write!(f, "1.20.2"),
            Self::V1_21 => write!(f, "1.21"),
        }
    }
}

/// 建筑组件用料配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub struct BuildingBlocks {
    /// 地基方块
    pub foundation: String,
    /// 墙体方块
    pub wall: String,
    /// 窗户方块
    pub window: String,
    /// 地板方块
    pub floor: String,
    /// 屋顶方块
    pub roof: String,
    /// 入口方块
    pub entrance: String,
    /// 装饰方块
    pub accent: String,
}

impl Default for BuildingBlocks {
    fn default() -> Self {
        Self::school_preset()
    }
}

impl BuildingBlocks {
    /// School 预设（v1.x Arnis 默认值）
    pub fn school_preset() -> Self {
        Self {
            foundation: "minecraft:stone_bricks".to_string(),
            wall: "minecraft:bricks".to_string(),
            window: "minecraft:glass_pane".to_string(),
            floor: "minecraft:oak_planks".to_string(),
            roof: "minecraft:dark_oak_slab".to_string(),
            entrance: "minecraft:dark_oak_door".to_string(),
            accent: "minecraft:smooth_sandstone".to_string(),
        }
    }

    /// Residential 预设
    pub fn residential_preset() -> Self {
        Self {
            foundation: "minecraft:smooth_stone".to_string(),
            wall: "minecraft:white_concrete".to_string(),
            window: "minecraft:glass_pane".to_string(),
            floor: "minecraft:spruce_planks".to_string(),
            roof: "minecraft:stone_slab".to_string(),
            entrance: "minecraft:spruce_door".to_string(),
            accent: "minecraft:light_gray_concrete".to_string(),
        }
    }
}

/// 用料表 - 包含所有类别的方块配置
///
/// 目前主要关注建筑，其他类别的用料可后续扩展。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MaterialTable {
    /// Minecraft 版本
    pub minecraft_version: MinecraftVersion,
    /// 建筑用料配置（多套预设）
    pub building_presets: BuildingPresets,
}

impl MaterialTable {
    /// 创建 1.20.4 版本的 School 预设用料表
    pub fn v1_20_4_school() -> Self {
        Self {
            minecraft_version: MinecraftVersion::V1_20_4,
            building_presets: BuildingPresets {
                school: BuildingBlocks::school_preset(),
                residential: BuildingBlocks::residential_preset(),
            },
        }
    }

    /// 验证所有方块在目标 MC 版本是否存在
    ///
    /// 返回所有验证通过的方块列表。
    pub fn validate_blocks(&self, blocks: &[String]) -> Result<Vec<String>, ValidationError> {
        let mut valid_blocks = Vec::new();
        let mut invalid_blocks = Vec::new();

        for block in blocks {
            if self.is_valid_block(block) {
                valid_blocks.push(block.clone());
            } else {
                invalid_blocks.push(block.clone());
            }
        }

        if invalid_blocks.is_empty() {
            Ok(valid_blocks)
        } else {
            Err(ValidationError {
                invalid_blocks,
                version: self.minecraft_version.to_string(),
            })
        }
    }

    /// 检查单个方块是否在目标版本存在
    ///
    /// # 注意
    /// 这是简化实现。实际项目应从 MC 官方数据库或权威来源加载版本方块列表。
    /// 这里仅校验格式是否规范。
    fn is_valid_block(&self, block: &str) -> bool {
        // 简化：只校验 format，实际应动态检测版本兼容性
        // 例如：minecraft:crafter 只在 1.21+ 可用
        block.starts_with("minecraft:") && !block.is_empty()
    }
}

/// 多套建筑预设
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub struct BuildingPresets {
    /// School 预设（默认）
    pub school: BuildingBlocks,
    /// Residential 预设
    pub residential: BuildingBlocks,
    // 可后续添加：commercial, office, hospital 等
}

impl Default for BuildingPresets {
    fn default() -> Self {
        Self {
            school: BuildingBlocks::school_preset(),
            residential: BuildingBlocks::residential_preset(),
        }
    }
}

/// 用料表验证错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub invalid_blocks: Vec<String>,
    pub version: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "以下方块在 MC {} 中不存在：{} (共 {} 个无效方块)",
            self.version,
            self.invalid_blocks.join(", "),
            self.invalid_blocks.len()
        )
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_material_table_creation() {
        let table = MaterialTable::v1_20_4_school();

        assert_eq!(table.minecraft_version, MinecraftVersion::V1_20_4);
        assert_eq!(
            table.building_presets.school.foundation,
            "minecraft:stone_bricks"
        );
        assert_eq!(
            table.building_presets.residential.wall,
            "minecraft:white_concrete"
        );
    }

    #[test]
    fn test_validate_blocks() {
        let table = MaterialTable::v1_20_4_school();
        let blocks = vec![
            "minecraft:stone_bricks".to_string(),
            "minecraft:bricks".to_string(),
            "minecraft:glass_pane".to_string(),
        ];

        let result = table.validate_blocks(&blocks);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_validate_invalid_format() {
        let table = MaterialTable::v1_20_4_school();
        let blocks = vec!["air".to_string(), "tnt".to_string()];

        // air 不是有效方块 ID 格式（无 namespace）
        let result = table.validate_blocks(&blocks);
        assert!(result.is_err());
    }

    #[test]
    fn test_display_minecraft_version() {
        assert_eq!(format!("{}", MinecraftVersion::V1_20_4), "1.20.4");
        assert_eq!(format!("{}", MinecraftVersion::V1_20_2), "1.20.2");
        assert_eq!(format!("{}", MinecraftVersion::V1_21), "1.21");
    }
}
