//! 用料表验证逻辑（方块是否存在检查）
//!
//! 依据 ADR-0024：用料表与 MC 版本强绑定，只准用目标版本存在的方块。

use crate::materials::{block_id_is_allowed, MinecraftVersion, ValidationError};

/// 用料表验证器
pub struct MaterialValidator;

impl MaterialValidator {
    /// 创建新的验证器
    pub fn new() -> Self {
        Self
    }

    /// 验证一系列方块在指定 MC 版本是否可用
    ///
    /// # Arguments
    /// * `version` - Minecraft 版本
    /// * `blocks` - 待验证的方块列表
    ///
    /// # Returns
    /// * `Ok(Vec<String>)` - 所有方块都有效，返回原列表
    /// * `Err(ValidationError)` - 存在无效方块
    ///
    /// # 注意事项
    pub fn validate_blocks_for_version(
        &self,
        version: MinecraftVersion,
        blocks: &[String],
    ) -> Result<Vec<String>, ValidationError> {
        let mut valid_blocks = Vec::new();
        let mut invalid_blocks = Vec::new();

        for block in blocks {
            if self.is_valid_block(version, block) {
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
                version: version.to_string(),
            })
        }
    }

    /// Check one block ID against the authoritative allowlist for its Minecraft version.
    fn is_valid_block(&self, version: MinecraftVersion, block: &str) -> bool {
        block_id_is_allowed(version, block)
    }

    /// 获取指定版本的基础方块列表（Arnis 默认用料）
    pub fn get_default_blocks_for_version(&self, _version: MinecraftVersion) -> Vec<String> {
        vec![
            "minecraft:stone_bricks".to_string(),
            "minecraft:bricks".to_string(),
            "minecraft:glass_pane".to_string(),
            "minecraft:oak_planks".to_string(),
            "minecraft:dark_oak_slab".to_string(),
            "minecraft:dark_oak_door".to_string(),
            "minecraft:smooth_sandstone".to_string(),
        ]
    }
}

impl Default for MaterialValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::MaterialTable;

    #[test]
    fn test_validate_blocks_all_valid() {
        let validator = MaterialValidator::new();
        let blocks = vec![
            "minecraft:stone_bricks".to_string(),
            "minecraft:bricks".to_string(),
            "minecraft:glass_pane".to_string(),
        ];

        let result = validator.validate_blocks_for_version(MinecraftVersion::V1_20_4, &blocks);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_validate_invalid_format() {
        let validator = MaterialValidator::new();
        let blocks = vec!["air".to_string(), "tnt".to_string()];

        let result = validator.validate_blocks_for_version(MinecraftVersion::V1_20_4, &blocks);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_version_specific_blocks() {
        let validator = MaterialValidator::new();

        // crafter 块只在 1.21+ 可用
        let crafter_block = vec!["minecraft:crafter".to_string()];

        // 1.20.4 中不应认可 crafter
        let result =
            validator.validate_blocks_for_version(MinecraftVersion::V1_20_4, &crafter_block);
        assert!(result.is_err());

        // 1.21 中应认可 crafter
        let result = validator.validate_blocks_for_version(MinecraftVersion::V1_21, &crafter_block);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_default_blocks() {
        let validator = MaterialValidator::new();
        let blocks = validator.get_default_blocks_for_version(MinecraftVersion::V1_20_4);

        assert_eq!(blocks.len(), 7);
        assert!(blocks.contains(&"minecraft:stone_bricks".to_string()));
    }

    #[test]
    fn test_integration_with_material_table() {
        let table = MaterialTable::v1_20_4_school();
        let validator = MaterialValidator::new();

        // 从用料表提取所有方块
        let blocks = vec![
            table.building_presets.school.foundation.clone(),
            table.building_presets.school.wall.clone(),
            table.building_presets.school.window.clone(),
            table.building_presets.school.floor.clone(),
            table.building_presets.school.roof.clone(),
            table.building_presets.school.entrance.clone(),
            table.building_presets.school.accent.clone(),
        ];

        // 验证这些方块在当前版本都存在
        let result = validator.validate_blocks_for_version(table.minecraft_version, &blocks);
        assert!(result.is_ok(), "用料表中的方块应在当前版本存在");
    }

    #[test]
    fn test_mixed_valid_invalid_blocks() {
        let validator = MaterialValidator::new();
        let blocks = vec![
            "minecraft:stone_bricks".to_string(), // 有效
            "air".to_string(),                    // 无效
            "minecraft:bricks".to_string(),       // 有效
        ];

        let result = validator.validate_blocks_for_version(MinecraftVersion::V1_20_4, &blocks);
        assert!(result.is_err());
        assert_eq!(result.as_ref().unwrap_err().invalid_blocks.len(), 1);
        assert_eq!(result.as_ref().unwrap_err().invalid_blocks[0], "air");
    }
}
