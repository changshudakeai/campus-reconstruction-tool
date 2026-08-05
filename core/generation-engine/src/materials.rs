//! 用料表适配器 —— 按 MC 版本取方块并验证（缝 6）。
//!
//! B18 与 B17 的接口是"只读调用"：生成规则通过 `MaterialRole` 向本模块要
//! 方块 ID，本模块从 B17 用料表取值并用 B17 的 `MaterialValidator` 做
//! 版本校验。查不到目标版本的方块直接报错而非替换（ADR-0024）。

use manifest_generator::{MaterialTable, MaterialValidator, MinecraftVersion};

/// 用料查询错误：目标版本查不到该方块，直接报错，禁止静默替换（ADR-0024）。
#[derive(Debug, thiserror::Error)]
pub enum MaterialError {
    /// 方块未通过目标 MC 版本的用料表校验。
    #[error("用料表版本绑定校验未通过：{0}")]
    MaterialUnavailable(String),
}

/// 方块用途角色（集中配置的查询键，禁止在生成逻辑里散写方块 ID）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialRole {
    /// 边界直出：最小平整场地的地面。
    FoundationGround,
    /// 建筑：地基。
    BuildingFoundation,
    /// 建筑：墙体。
    BuildingWall,
    /// 建筑：窗户。
    BuildingWindow,
    /// 建筑：地板。
    BuildingFloor,
    /// 建筑：屋顶。
    BuildingRoof,
    /// 建筑：入口门。
    BuildingEntrance,
    /// 建筑：装饰条。
    BuildingAccent,
    /// 道路：铺面。
    Road,
    /// 水域：水面。
    Water,
    /// 植被：树干。
    TreeTrunk,
    /// 植被：树叶。
    TreeLeaves,
    /// 体育：场地地面。
    SportsCourt,
    /// 体育：划线。
    SportsLine,
    /// 其他：铁轨。
    OtherRail,
}

/// 角色 → 方块 ID 的集中映射。
///
/// 建筑七件套来自 B17 用料表的 school 预设（Arnis/v1.x 默认用料）；
/// 其余五类是本 crate 的集中常量（B17 用料表当前只覆盖建筑，见其模块文档
/// "其他类别的用料可后续扩展"——扩表后此处改为读表，调用方无感）。
fn block_id_for_role(role: MaterialRole, table: &MaterialTable) -> String {
    let school = &table.building_presets.school;
    match role {
        // 边界直出复用 B17 学校预设的地基方块，保持版本绑定与既有用料表一致。
        MaterialRole::FoundationGround => school.foundation.clone(),
        MaterialRole::BuildingFoundation => school.foundation.clone(),
        MaterialRole::BuildingWall => school.wall.clone(),
        MaterialRole::BuildingWindow => school.window.clone(),
        MaterialRole::BuildingFloor => school.floor.clone(),
        MaterialRole::BuildingRoof => school.roof.clone(),
        MaterialRole::BuildingEntrance => school.entrance.clone(),
        MaterialRole::BuildingAccent => school.accent.clone(),
        MaterialRole::Road => "minecraft:smooth_stone".to_string(),
        MaterialRole::Water => "minecraft:water".to_string(),
        MaterialRole::TreeTrunk => "minecraft:oak_log".to_string(),
        MaterialRole::TreeLeaves => "minecraft:oak_leaves".to_string(),
        MaterialRole::SportsCourt => "minecraft:red_concrete".to_string(),
        MaterialRole::SportsLine => "minecraft:white_concrete".to_string(),
        MaterialRole::OtherRail => "minecraft:rail".to_string(),
    }
}

/// 用料表适配器：桥接 B17 并提供带版本校验的查询。
pub struct MaterialsAdapter {
    table: MaterialTable,
}

impl MaterialsAdapter {
    pub fn new(table: MaterialTable) -> Self {
        Self { table }
    }

    /// 当前用料表绑定的 MC 版本。
    pub fn version(&self) -> MinecraftVersion {
        self.table.minecraft_version
    }

    /// 按角色查询方块 ID；未通过目标版本校验则返回错误。
    pub fn block_for(&self, role: MaterialRole) -> Result<String, MaterialError> {
        self.validate_block(block_id_for_role(role, &self.table))
    }

    /// 校验一个方块 ID 在目标版本是否可用（走 B17 MaterialValidator）。
    pub fn validate_block(&self, block_id: impl Into<String>) -> Result<String, MaterialError> {
        let block_id = block_id.into();
        MaterialValidator::new()
            .validate_blocks_for_version(
                self.table.minecraft_version,
                std::slice::from_ref(&block_id),
            )
            .map_err(|err| MaterialError::MaterialUnavailable(err.to_string()))?;
        Ok(block_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter() -> MaterialsAdapter {
        MaterialsAdapter::new(MaterialTable::v1_20_4_school())
    }

    #[test]
    fn every_role_resolves_on_v1_20_4() {
        let adapter = adapter();
        let roles = [
            MaterialRole::FoundationGround,
            MaterialRole::BuildingFoundation,
            MaterialRole::BuildingWall,
            MaterialRole::BuildingWindow,
            MaterialRole::BuildingFloor,
            MaterialRole::BuildingRoof,
            MaterialRole::BuildingEntrance,
            MaterialRole::BuildingAccent,
            MaterialRole::Road,
            MaterialRole::Water,
            MaterialRole::TreeTrunk,
            MaterialRole::TreeLeaves,
            MaterialRole::SportsCourt,
            MaterialRole::SportsLine,
            MaterialRole::OtherRail,
        ];
        for role in roles {
            assert!(adapter.block_for(role).is_ok(), "角色 {role:?} 应可解析");
        }
        assert_eq!(
            adapter.block_for(MaterialRole::BuildingWall).unwrap(),
            "minecraft:bricks"
        );
    }

    #[test]
    fn block_missing_in_target_version_is_an_error_not_a_substitute() {
        // crafter 只在 1.21+ 存在：1.20.4 必须报错，绝不悄悄换块。
        let err = adapter().validate_block("minecraft:crafter").unwrap_err();
        assert!(matches!(err, MaterialError::MaterialUnavailable(_)));

        // 同一个块在 1.21 的表里合法——证明拦截的是"版本"而不是"块名"。
        let v1_21 = MaterialsAdapter::new(MaterialTable {
            minecraft_version: MinecraftVersion::V1_21,
            building_presets: Default::default(),
        });
        assert!(v1_21.validate_block("minecraft:crafter").is_ok());
    }

    #[test]
    fn block_without_namespace_is_rejected() {
        let err = adapter().validate_block("bricks").unwrap_err();
        assert!(matches!(err, MaterialError::MaterialUnavailable(_)));
    }
}
