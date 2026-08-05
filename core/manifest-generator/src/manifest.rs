//! `foundation_manifest.json` 数据结构定义

use serde::{Deserialize, Serialize};
use shared_domain_types::CandidateCategory;

/// Manifest 版本号（用于未来格式变更）
const MANIFEST_VERSION: &str = "1.0.0";

/// 类别状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryStatus {
    /// 英文标识符（building, road, water...）
    pub name: String,
    /// 中文显示名（建筑，道路，水域...）
    pub display_name: String,
    /// 是否包含
    pub included: bool,
}

/// 完整导出实际采用的朝向来源。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManifestOrientationSource {
    /// 用户没有自定义朝向，完整导出用例采用地图正北。
    MapNorth,
    /// 用户明确提供了朝向。
    Custom,
}

/// Manifest 中记录的实际朝向。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestOrientation {
    /// 方位角（正北为 0°，顺时针增加）。
    pub degree: f32,
    /// 朝向来源；MapNorth 不代表用户设置了自定义朝向。
    pub source: ManifestOrientationSource,
}

/// 候选链事实：不因缺少采集/评审而伪造记录。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CandidateFacts {
    /// B2/B14 候选投影数量。
    #[serde(default)]
    pub candidate_projection_count: usize,
    /// 已存在的评审决定数量。
    #[serde(default)]
    pub review_decision_count: usize,
    /// 本次实际保留候选数量。
    #[serde(default)]
    pub retained_candidate_count: usize,
}

impl CategoryStatus {
    pub fn new(name: impl Into<String>, display_name: impl Into<String>, included: bool) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            included,
        }
    }
}

/// Foundation Manifest — 导出时生成的清单文件
///
/// 如实记录本次导出包含/缺失哪些类别（建筑✓、水域✗、其他✓等）。
/// 格式符合 ADR-0012 要求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationManifest {
    /// Manifest 规范版本号
    pub version: String,
    /// 生成 UUID（由上层传入或 F9 生成）
    pub id: String,
    /// 校区名称
    pub campus_name: String,
    /// 方案 ID
    pub plan_id: String,
    /// 方案名称
    pub plan_name: String,
    /// MC 版本
    pub minecraft_version: String,
    /// 导出时间戳（RFC3339 文本）
    pub exported_at: String,
    /// 类别状态列表（含包含/缺失信息）
    pub categories: Vec<CategoryStatus>,
    /// 完整导出实际采用的朝向；旧调用方未提供时保持 None。
    #[serde(default)]
    pub orientation: Option<ManifestOrientation>,
    /// 候选采集与评审事实。
    #[serde(default)]
    pub candidate_facts: CandidateFacts,
}

impl FoundationManifest {
    /// 创建新的 Manifest 实例
    ///
    /// # Arguments
    /// * `id` - Manifest UUID
    /// * `campus_name` - 校区名称
    /// * `plan_id` - 方案 ID
    /// * `plan_name` - 方案名称
    /// * `minecraft_version` - Minecraft 版本号
    /// * `included_categories` - 已评审保留的类别集合
    /// * `exported_at` - 导出时间戳 (RFC3339)
    pub fn new(
        id: impl Into<String>,
        campus_name: impl Into<String>,
        plan_id: impl Into<String>,
        plan_name: impl Into<String>,
        minecraft_version: impl Into<String>,
        included_categories: &[CandidateCategory],
        exported_at: impl Into<String>,
    ) -> Self {
        let minecraft_version = minecraft_version.into();
        let mut categories = Vec::new();

        // 固定顺序：建筑→道路→水域→植被→体育→其他
        for category in [
            CandidateCategory::Building,
            CandidateCategory::Road,
            CandidateCategory::Water,
            CandidateCategory::Vegetation,
            CandidateCategory::Sports,
            CandidateCategory::Other,
        ]
        .iter()
        {
            let included = included_categories.contains(category);
            let status = CategoryStatus::new(
                category.display_name().to_string(),
                category.display_name().to_string(),
                included,
            );
            categories.push(status);
        }

        Self {
            version: MANIFEST_VERSION.to_string(),
            id: id.into(),
            campus_name: campus_name.into(),
            plan_id: plan_id.into(),
            plan_name: plan_name.into(),
            minecraft_version,
            exported_at: exported_at.into(),
            categories,
            orientation: None,
            candidate_facts: CandidateFacts::default(),
        }
    }

    /// 从 JSON 字符串解析
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// 序列化为 JSON 字符串（带缩进）
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 获取包含的类别列表
    pub fn included(&self) -> Vec<&CategoryStatus> {
        self.categories.iter().filter(|c| c.included).collect()
    }

    /// 获取缺失的类别列表
    pub fn excluded(&self) -> Vec<&CategoryStatus> {
        self.categories.iter().filter(|c| !c.included).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_creation() {
        let included = vec![CandidateCategory::Building, CandidateCategory::Vegetation];

        let manifest = FoundationManifest::new(
            "manifest-id-1",
            "测试校区",
            "plan-id-1",
            "测试方案",
            "1.20.4",
            &included,
            "2026-01-01T00:00:00+00:00",
        );

        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.campus_name, "测试校区");
        assert_eq!(manifest.minecraft_version, "1.20.4");
        assert_eq!(manifest.categories.len(), 6); // 所有六类别

        // 检查包含/缺失状态
        let included_count = manifest.included().len();
        let excluded_count = manifest.excluded().len();
        assert_eq!(included_count, 2);
        assert_eq!(excluded_count, 4);

        // 验证具体类别状态
        assert!(manifest
            .categories
            .iter()
            .any(|c| c.name == "建筑" && c.included));
        assert!(manifest
            .categories
            .iter()
            .any(|c| c.name == "植被" && c.included));
        assert!(manifest
            .categories
            .iter()
            .any(|c| c.name == "水域" && !c.included));
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = FoundationManifest::new(
            "manifest-id-2",
            "测试校区",
            "plan-id-2",
            "测试方案",
            "1.20.4",
            &[CandidateCategory::Building],
            "2026-01-01T00:00:00+00:00",
        );

        let json = manifest.to_json_pretty().unwrap();
        let parsed = FoundationManifest::from_json(&json).unwrap();

        assert_eq!(manifest.version, parsed.version);
        assert_eq!(manifest.campus_name, parsed.campus_name);
        assert_eq!(manifest.plan_name, parsed.plan_name);
        assert_eq!(manifest.minecraft_version, parsed.minecraft_version);
        assert_eq!(manifest.categories.len(), parsed.categories.len());

        // 验证 includes/excludes 方法
        assert_eq!(manifest.included().len(), parsed.included().len());
        assert_eq!(manifest.excluded().len(), parsed.excluded().len());
    }
}
