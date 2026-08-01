//! 候选对象内存模型
//!
//! 缝 4 契约：候选集在进台时从 B2 已发布的可评审投影一次性读入本模型，
//! 评审期间所有状态都在这里改，不碰数据库。
//! 类别与三态复用 B1 共享领域类型（不重新定义）。

use data_persistence::{CandidateProjection, RawObservation};
use shared_domain_types::{CandidateCategory, ReviewState};

/// 候选对象的稳定标识：类别 + 真实世界对象 ID
///
/// 与 B2 两张表（raw_observations / review_decisions）的
/// `(entity_type, entity_id)` 组合键一一对应。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CandidateKey {
    /// 实体类别（六类别之一）
    pub category: CandidateCategory,
    /// 真实世界对象 ID（来自数据源）
    pub entity_id: String,
}

impl CandidateKey {
    /// 构建候选标识
    pub fn new(category: CandidateCategory, entity_id: impl Into<String>) -> Self {
        Self {
            category,
            entity_id: entity_id.into(),
        }
    }
}

impl std::fmt::Display for CandidateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}/{}", self.category, self.entity_id)
    }
}

/// 评审台上的一个候选（纯内存状态）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 稳定标识（类别 + 实体 ID）
    pub key: CandidateKey,
    /// 卡片标题：原始标签里的 `name`，没有则回落到实体 ID
    pub title: String,
    /// 标签与属性（key=value 对，信息面板展示用）
    pub tags: Vec<(String, String)>,
    /// 评审三态（初始一律"待定"，ADR-0022）
    pub state: ReviewState,
    /// 卡片复选框勾选状态（批量操作的输入）
    pub selected: bool,
}

impl Candidate {
    /// 从 B2 已发布的可评审投影构建候选（初始态"待定"、未勾选）。
    pub fn from_projection(projection: &CandidateProjection) -> Self {
        let tags = Vec::new();
        let title = projection.candidate_id.clone();
        Self {
            key: CandidateKey::new(projection.category, projection.candidate_id.clone()),
            title,
            tags,
            state: ReviewState::Pending,
            selected: false,
        }
    }

    /// 兼容旧测试的原始观测投影；生产进台只使用 [`Self::from_projection`].
    pub fn from_observation(observation: &RawObservation) -> Self {
        let tags = flatten_source_data(&observation.source_data);
        let title = tags
            .iter()
            .find(|(key, _)| key == "name")
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| observation.entity_id.clone());
        Self {
            key: CandidateKey::new(observation.entity_type, observation.entity_id.clone()),
            title,
            tags,
            state: ReviewState::Pending,
            selected: false,
        }
    }
}

/// 把 source_data JSON 摊平成 key=value 对：
/// 顶层标量直接收录；顶层 `tags` 对象里的标量也收录（两种采集格式都兼容）。
fn flatten_source_data(source_data: &serde_json::Value) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    if let serde_json::Value::Object(map) = source_data {
        for (key, value) in map {
            match value {
                serde_json::Value::Object(nested) if key == "tags" => {
                    for (tag_key, tag_value) in nested {
                        if let Some(text) = scalar_text(tag_value) {
                            pairs.push((tag_key.clone(), text));
                        }
                    }
                }
                other => {
                    if let Some(text) = scalar_text(other) {
                        pairs.push((key.clone(), text));
                    }
                }
            }
        }
    }
    pairs
}

/// 标量 JSON 值转文本；数组/对象不进标签列表（信息面板只展示扁平键值）
fn scalar_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(source_data: serde_json::Value) -> RawObservation {
        RawObservation::new(
            "plan-1",
            CandidateCategory::Building,
            "way/100",
            source_data,
            "overpass",
        )
    }

    #[test]
    fn title_prefers_name_tag() {
        let candidate = Candidate::from_observation(&observation(serde_json::json!({
            "tags": { "name": "体育馆", "building": "gymnasium" }
        })));
        assert_eq!(candidate.title, "体育馆");
        assert_eq!(candidate.state, ReviewState::Pending);
        assert!(!candidate.selected);
    }

    #[test]
    fn title_falls_back_to_entity_id() {
        let candidate = Candidate::from_observation(&observation(serde_json::json!({
            "building": "yes"
        })));
        assert_eq!(candidate.title, "way/100");
        assert_eq!(
            candidate.tags,
            vec![("building".to_owned(), "yes".to_owned())]
        );
    }

    #[test]
    fn flatten_merges_top_level_and_tags_object() {
        let candidate = Candidate::from_observation(&observation(serde_json::json!({
            "levels": 3,
            "tags": { "building": "school" },
            "geometry": [[1, 2]]
        })));
        assert!(candidate
            .tags
            .contains(&("levels".to_owned(), "3".to_owned())));
        assert!(candidate
            .tags
            .contains(&("building".to_owned(), "school".to_owned())));
        // 数组不进标签列表
        assert!(!candidate.tags.iter().any(|(key, _)| key == "geometry"));
    }
}
