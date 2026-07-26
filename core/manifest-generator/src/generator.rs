//! Manifest 生成逻辑
//!
//! 依据窗口契约：缝 6，B17 同时读取评审终态 + 方案信息生成 manifest。
//! F9 从 data-persistence 读取评审终态后调用 B17 API（B17 自身不碰数据库，
//! 保持基础层横向零依赖）。

use shared_domain_types::{CandidateCategory, PlanId};

use crate::manifest::FoundationManifest;

/// 方案信息（由 F9 传入）
#[derive(Debug, Clone)]
pub struct PlanInfo {
    /// 校区名称
    pub campus_name: String,
    /// 方案 ID
    pub plan_id: PlanId,
    /// 方案名称
    pub plan_name: String,
    /// Minecraft 版本（全局设置）
    pub minecraft_version: String,
}

impl PlanInfo {
    /// 创建新的方案信息
    pub fn new(
        campus_name: impl Into<String>,
        plan_id: PlanId,
        plan_name: impl Into<String>,
        minecraft_version: impl Into<String>,
    ) -> Self {
        Self {
            campus_name: campus_name.into(),
            plan_id,
            plan_name: plan_name.into(),
            minecraft_version: minecraft_version.into(),
        }
    }
}

/// Manifest 生成器
pub struct ManifestGenerator;

impl ManifestGenerator {
    /// 创建新的 Manifest 生成器
    pub fn new() -> Self {
        Self
    }

    /// 从评审终态生成 Manifest
    ///
    /// # Arguments
    /// * `plan_info` - 方案信息（含校区名、MC 版本等）
    /// * `review_decisions` - 各类别的保留情况 [(类别, 是否保留)]
    /// * `manifest_id` - Manifest 唯一 ID（由上层生成，B17 不依赖 uuid）
    /// * `exported_at` - 导出时间戳 RFC3339 文本（由上层生成，B17 不依赖 chrono）
    pub fn generate_manifest(
        &self,
        plan_info: &PlanInfo,
        review_decisions: &[(CandidateCategory, bool)],
        manifest_id: impl Into<String>,
        exported_at: impl Into<String>,
    ) -> Result<FoundationManifest, GeneratorError> {
        // 提取已保留的类别集合
        let included_categories: Vec<CandidateCategory> = review_decisions
            .iter()
            .filter(|(_, is_keep)| *is_keep)
            .map(|(category, _)| *category)
            .collect();

        Ok(FoundationManifest::new(
            manifest_id,
            &plan_info.campus_name,
            plan_info.plan_id.to_string(),
            &plan_info.plan_name,
            &plan_info.minecraft_version,
            &included_categories,
            exported_at,
        ))
    }

    /// 生成并写入文件
    ///
    /// # Arguments
    /// * `manifest` - Manifest 实例
    /// * `dir_path` - 目标目录路径
    /// * `filename` - 文件名（建议："foundation_manifest.json"）
    ///
    /// # Errors
    /// IO 错误或 JSON 序列化错误
    pub fn write_to_file(
        &self,
        manifest: &FoundationManifest,
        dir_path: impl AsRef<std::path::Path>,
        filename: &str,
    ) -> Result<(), GeneratorError> {
        let json = manifest
            .to_json_pretty()
            .map_err(GeneratorError::Serialization)?;
        let path = dir_path.as_ref().join(filename);
        std::fs::write(&path, json).map_err(|e| GeneratorError::Io(e, path))
    }
}

impl Default for ManifestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Manifest 生成错误（带类型的值一路向上传递，窗口契约章）
#[derive(Debug)]
pub enum GeneratorError {
    /// JSON 序列化失败
    Serialization(serde_json::Error),
    /// 文件写入失败（含目标路径）
    Io(std::io::Error, std::path::PathBuf),
}

impl std::fmt::Display for GeneratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialization(err) => write!(f, "JSON 序列化失败：{err}"),
            Self::Io(err, path) => write!(f, "IO 失败：{err}（路径：{}）", path.display()),
        }
    }
}

impl std::error::Error for GeneratorError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_info() -> PlanInfo {
        PlanInfo::new("测试校区", PlanId::generate(), "测试方案", "1.20.4")
    }

    fn generate(decisions: &[(CandidateCategory, bool)]) -> FoundationManifest {
        ManifestGenerator::new()
            .generate_manifest(
                &plan_info(),
                decisions,
                "manifest-test-id",
                "2026-01-01T00:00:00+00:00",
            )
            .unwrap()
    }

    #[test]
    fn test_generate_manifest_with_partial_review() {
        // 模拟评审终态：建筑保留，水域剔除，其他保留
        let manifest = generate(&[
            (CandidateCategory::Building, true),
            (CandidateCategory::Water, false),
            (CandidateCategory::Other, true),
            (CandidateCategory::Vegetation, false),
        ]);

        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.campus_name, "测试校区");
        assert_eq!(manifest.minecraft_version, "1.20.4");
        assert_eq!(manifest.included().len(), 2);
        assert_eq!(manifest.excluded().len(), 4);

        // 验证具体类别状态
        let building = manifest
            .categories
            .iter()
            .find(|c| c.name == "建筑")
            .unwrap();
        assert!(building.included);

        let water = manifest
            .categories
            .iter()
            .find(|c| c.name == "水域")
            .unwrap();
        assert!(!water.included);

        let other = manifest
            .categories
            .iter()
            .find(|c| c.name == "其他")
            .unwrap();
        assert!(other.included);
    }

    #[test]
    fn test_generate_manifest_empty_included() {
        // 所有类别都剔除 → 最小路径（仅边界 + 朝向），manifest 全部记为缺失
        let manifest = generate(&[
            (CandidateCategory::Building, false),
            (CandidateCategory::Road, false),
            (CandidateCategory::Water, false),
            (CandidateCategory::Vegetation, false),
            (CandidateCategory::Sports, false),
            (CandidateCategory::Other, false),
        ]);

        assert_eq!(manifest.included().len(), 0);
        assert_eq!(manifest.excluded().len(), 6);
    }

    #[test]
    fn test_generate_manifest_all_included() {
        let manifest = generate(&[
            (CandidateCategory::Building, true),
            (CandidateCategory::Road, true),
            (CandidateCategory::Water, true),
            (CandidateCategory::Vegetation, true),
            (CandidateCategory::Sports, true),
            (CandidateCategory::Other, true),
        ]);

        assert_eq!(manifest.included().len(), 6);
        assert_eq!(manifest.excluded().len(), 0);
    }

    #[test]
    fn test_write_to_file() {
        let generator = ManifestGenerator::new();
        let manifest = generate(&[(CandidateCategory::Building, true)]);

        // 沙箱只准写工作区内，临时目录放 target/ 下
        let temp_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/manifest-generator-test-tmp");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = generator.write_to_file(&manifest, &temp_dir, "test_manifest.json");
        assert!(result.is_ok());

        // 验证文件存在且内容正确
        let file_path = temp_dir.join("test_manifest.json");
        assert!(file_path.exists());

        let content = std::fs::read_to_string(&file_path).unwrap();
        let parsed = FoundationManifest::from_json(&content).unwrap();
        assert_eq!(parsed.campus_name, "测试校区");
        assert_eq!(parsed.included().len(), 1);

        // 清理临时文件
        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
