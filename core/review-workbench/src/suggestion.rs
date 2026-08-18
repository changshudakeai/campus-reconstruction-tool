//! F5 轻量建议辅助：确定性、可解释、只使用现有候选数据的建议生成。
//!
//! 本模块不访问数据库、不调用外部服务。规则输入全部来自进台时从 B2 读入的
//! 候选投影既有字段：
//! - 几何验证结果（`validation` / `automatically_repaired`，B14）
//! - 名称来源（`name_source`，E：OSM / 高德 / 缓存 / 未命名 / 补名失败）
//! - 标签完整度（`tags`）
//! - 重复嫌疑（同来源/同原始观测、同名称、同质心的成对比较）
//! - 来源类型（`data_source_tag`，用于"建议保留"的成熟度条件）
//! - 形状复杂度（几何点数、种类：点/线/面）
//! - 现有隔离/警告理由（D 的字符串理由 `isolation_reason`，防御性保留）
//!
//! 边界外与不可评审对象（Isolated）由 B2 资格接口排除，不进入评审台，
//! 本模块不会再次处理它们（ADR-0040）。
//!
//! ## 确定性
//!
//! [`compute`] 内部按稳定候选标识排序后再做成对分析，同一份输入必然产生
//! 同一份建议；本模块不产生随机数、不依赖当前系统时间。
//!
//! ## 置信度分档（T51）
//!
//! 置信度不是数值评分，而是对既有建议规则的确定性派生分档：
//!
//! - **高** = 建议保留（名称清晰、形状完整、无异常，R11）；
//! - **中** = 存在不确定信号、需人工确认（隔离理由、本次未找到、形状可疑、
//!   标签稀疏、缺少来源）；
//! - **低** = 未命名、重复投影、重复嫌疑、重叠、自动修复过等需关注。
//!
//! 卡片仍显示原有的动作标签与一句话理由；置信度只用于筛选芯片、排序与
//! 一键应用建议，不改变三态语义。
//!
//! ## 与 ReviewState 的关系（验收 5）
//!
//! 生成建议只读候选数据，绝不修改 `ReviewState`。只有用户在 UI 上点击
//! "应用建议"并确认后，[`crate::workbench::ReviewWorkbench`] 才会通过既有
//! 批量状态变更机制把建议写成评审决定。

use data_persistence::{CandidateNameSource, CandidateShape};
use shared_domain_types::CandidateCategory;

use crate::candidate::{Candidate, CandidateKey};
use crate::confidence::ConfidenceTier;
use crate::view_models::text_keys;

/// 建议类别（验收 1：至少覆盖"未命名 / 需要关注 / 无需处理"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionCategory {
    /// 未命名：候选没有可用名称，需要人工确认。
    Unnamed,
    /// 需要关注：重叠、形状可疑、重复嫌疑、修复/隔离理由等。
    NeedsAttention,
    /// 无需处理：名称明确、形状完整且无异常。
    NoActionNeeded,
}

/// 建议动作：筛选"建议保留 / 建议人工确认 / 建议剔除"与一键应用的执行依据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionAction {
    /// 建议保留（名称明确、形状完整、无异常）。
    Keep,
    /// 建议人工确认（存在不确定信号，不应自动裁决）。
    HumanReview,
    /// 建议剔除（与另一候选为同一来源对象的重复投影，证据明确）。
    Remove,
}

/// 一条可解释建议：类别 + 动作 + 一句话可读理由（文本键与插值参数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSuggestion {
    /// 建议类别（未命名 / 需要关注 / 无需处理）。
    pub category: SuggestionCategory,
    /// 建议动作（保留 / 人工确认 / 剔除）。
    pub action: SuggestionAction,
    /// 一句话可读理由的文本键（zh-CN.json 解析，占位符见
    /// [`CandidateSuggestion::reason_args`]）。
    pub reason_key: &'static str,
    /// 理由文本的插值参数（`l10n.t_with_args(reason_key, reason_args)`）。
    pub reason_args: serde_json::Value,
    /// 无参数的理由摘要标签键（确认框"主要理由分布"聚合用）。
    pub summary_key: &'static str,
}

impl CandidateSuggestion {
    /// 由建议动作/类别/理由确定性映射出置信度分档（T51）。
    ///
    /// - 建议保留 → 高；
    /// - 建议剔除（重复投影）→ 低；
    /// - 建议人工确认按理由细分：未命名/自动修复/重叠/重复嫌疑 → 低，
    ///   其余不确定信号（隔离理由、本次未找到、形状可疑、标签稀疏、
    ///   缺少来源）→ 中。
    pub fn confidence_tier(&self) -> ConfidenceTier {
        match self.action {
            SuggestionAction::Keep => ConfidenceTier::High,
            SuggestionAction::Remove => ConfidenceTier::Low,
            SuggestionAction::HumanReview => match self.category {
                SuggestionCategory::Unnamed => ConfidenceTier::Low,
                SuggestionCategory::NeedsAttention => match self.reason_key {
                    text_keys::SUGGESTION_REASON_REPAIRED
                    | text_keys::SUGGESTION_REASON_OVERLAP
                    | text_keys::SUGGESTION_REASON_DUPLICATE_SUSPECT => ConfidenceTier::Low,
                    _ => ConfidenceTier::Medium,
                },
                SuggestionCategory::NoActionNeeded => ConfidenceTier::High,
            },
        }
    }
}

/// 确认框"主要理由分布"中的一行：摘要标签 + 数量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonLine {
    /// 无参数理由摘要标签文本键。
    pub summary_key: &'static str,
    /// 命中该理由的可执行对象数。
    pub count: usize,
}

/// 建议应用确认请求：对象数量 + 保留/剔除拆分 + 主要理由分布（验收 4）。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SuggestionApplyRequest {
    /// 将改变评审状态的对象数（保留 + 剔除）。
    pub count: usize,
    /// 其中建议保留的对象数。
    pub keep_count: usize,
    /// 其中建议剔除的对象数。
    pub remove_count: usize,
    /// 主要理由分布（按数量降序，最多保留前几条）。
    pub reason_lines: Vec<ReasonLine>,
}

/// 最近一批已应用建议的追溯记录（验收 6：记录批次与理由；封账后不可撤销）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedSuggestionBatch {
    /// 本批实际改变状态的候选标识（按候选标识稳定排序）。
    pub targets: Vec<CandidateKey>,
    /// 本批应用的保留建议数。
    pub keep_count: usize,
    /// 本批应用的剔除建议数。
    pub remove_count: usize,
    /// 本批主要理由分布。
    pub reason_lines: Vec<ReasonLine>,
    /// 应用前各候选的三态（撤销恢复依据）。
    pub before_states: Vec<(CandidateKey, shared_domain_types::ReviewState)>,
}

/// 规则引擎：纯函数，无状态。
#[derive(Debug, Default, Clone, Copy)]
pub struct SuggestionEngine;

impl SuggestionEngine {
    /// 为一组候选生成建议。
    ///
    /// 返回按稳定候选标识升序排列的 `(key, suggestion)`；内部先按
    /// candidate_id 排序再做成对分析，保证相同输入产生相同建议。
    pub fn compute(candidates: &[Candidate]) -> Vec<(CandidateKey, CandidateSuggestion)> {
        let mut ordered: Vec<&Candidate> = candidates.iter().collect();
        ordered.sort_by(|a, b| a.key.cmp(&b.key));

        // 成对信号：只对排序后的 i < j 比较一次，先到先得（确定性）。
        let mut exact_duplicate_of: Vec<Option<String>> = vec![None; ordered.len()];
        let mut overlap_of: Vec<Option<String>> = vec![None; ordered.len()];
        let mut duplicate_suspect_of: Vec<Option<String>> = vec![None; ordered.len()];
        for i in 0..ordered.len() {
            for j in (i + 1)..ordered.len() {
                let (a, b) = (ordered[i], ordered[j]);
                if exact_duplicate(a, b) {
                    // 重复投影只建议剔除"后者"，保留先出现的那个。
                    if exact_duplicate_of[j].is_none() {
                        exact_duplicate_of[j] = Some(a.title.clone());
                    }
                    // 重复对不再同时标记重叠/同名嫌疑，避免把"应保留者"误标为需关注。
                    continue;
                }
                if both_buildings(a, b) && polygons_overlap(&a.shape, &b.shape) {
                    if overlap_of[i].is_none() {
                        overlap_of[i] = Some(b.title.clone());
                    }
                    if overlap_of[j].is_none() {
                        overlap_of[j] = Some(a.title.clone());
                    }
                }
                if same_category(a, b)
                    && (same_trimmed_name(a, b) || same_centroid(&a.shape, &b.shape))
                {
                    if duplicate_suspect_of[i].is_none() {
                        duplicate_suspect_of[i] = Some(b.title.clone());
                    }
                    if duplicate_suspect_of[j].is_none() {
                        duplicate_suspect_of[j] = Some(a.title.clone());
                    }
                }
            }
        }

        ordered
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                let suggestion = rule_for(
                    candidate,
                    exact_duplicate_of[index].as_deref(),
                    overlap_of[index].as_deref(),
                    duplicate_suspect_of[index].as_deref(),
                );
                (candidate.key.clone(), suggestion)
            })
            .collect()
    }
}

/// 单候选按优先级判定：第一条命中的规则决定建议（规则表见模块文档）。
fn rule_for(
    candidate: &Candidate,
    exact_duplicate_of: Option<&str>,
    overlap_of: Option<&str>,
    duplicate_suspect_of: Option<&str>,
) -> CandidateSuggestion {
    // R1 重复投影（同原始观测或同来源+同几何，剔除后者）——证据最明确。
    if let Some(other) = exact_duplicate_of {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::Remove,
            text_keys::SUGGESTION_REASON_EXACT_DUPLICATE,
            arg_other(other),
            text_keys::SUGGESTION_SUMMARY_EXACT_DUPLICATE,
        );
    }
    // R2 未命名：无可用名称（E 标记 Unnamed/Failed，或标题回退为实体 ID）。
    if !candidate.named
        || matches!(
            candidate.name_source,
            CandidateNameSource::Unnamed | CandidateNameSource::Failed
        )
    {
        return suggestion(
            SuggestionCategory::Unnamed,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_UNNAMED,
            serde_json::json!({}),
            text_keys::SUGGESTION_SUMMARY_UNNAMED,
        );
    }
    // R3 现有隔离/警告理由（D 的字符串理由；Reviewable 候选防御性保留）。
    if let Some(reason) = candidate.isolation_reason.as_deref() {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_ISOLATED,
            serde_json::json!({ "reason": reason }),
            text_keys::SUGGESTION_SUMMARY_ISOLATED,
        );
    }
    // R4 几何经自动修复（B14 唯一修复，外观不变但值得人工复核）。
    if candidate.automatically_repaired {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_REPAIRED,
            serde_json::json!({}),
            text_keys::SUGGESTION_SUMMARY_REPAIRED,
        );
    }
    // R5 本次采集未找到（继承上批投影并显式标记）。
    if candidate.missing_in_latest_batch {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_MISSING_LATEST,
            serde_json::json!({}),
            text_keys::SUGGESTION_SUMMARY_MISSING_LATEST,
        );
    }
    // R6 疑似与另一建筑重叠。
    if let Some(other) = overlap_of {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_OVERLAP,
            arg_other(other),
            text_keys::SUGGESTION_SUMMARY_OVERLAP,
        );
    }
    // R7 重复嫌疑：同类别同名或同质心。
    if let Some(other) = duplicate_suspect_of {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_DUPLICATE_SUSPECT,
            arg_other(other),
            text_keys::SUGGESTION_SUMMARY_DUPLICATE_SUSPECT,
        );
    }
    // R8 形状可疑：建筑为点/线，或面环有效点少于 4。
    if candidate.category == CandidateCategory::Building && suspicious_shape(&candidate.shape) {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_SUSPICIOUS_SHAPE,
            serde_json::json!({}),
            text_keys::SUGGESTION_SUMMARY_SUSPICIOUS_SHAPE,
        );
    }
    // R9 标签完整度低：没有任何来源标签。
    if candidate.tags.is_empty() {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_SPARSE_TAGS,
            serde_json::json!({}),
            text_keys::SUGGESTION_SUMMARY_SPARSE_TAGS,
        );
    }
    // R10 来源类型：缺少来源信息（data_source_tag 为空）时无法追溯，建议人工确认。
    if candidate.source.is_empty() {
        return suggestion(
            SuggestionCategory::NeedsAttention,
            SuggestionAction::HumanReview,
            text_keys::SUGGESTION_REASON_MISSING_SOURCE,
            serde_json::json!({}),
            text_keys::SUGGESTION_SUMMARY_MISSING_SOURCE,
        );
    }
    // R11 无需处理：名称明确、形状完整且无异常，建议保留。
    suggestion(
        SuggestionCategory::NoActionNeeded,
        SuggestionAction::Keep,
        text_keys::SUGGESTION_REASON_KEEP,
        serde_json::json!({}),
        text_keys::SUGGESTION_SUMMARY_KEEP,
    )
}

fn suggestion(
    category: SuggestionCategory,
    action: SuggestionAction,
    reason_key: &'static str,
    reason_args: serde_json::Value,
    summary_key: &'static str,
) -> CandidateSuggestion {
    CandidateSuggestion {
        category,
        action,
        reason_key,
        reason_args,
        summary_key,
    }
}

fn arg_other(other: &str) -> serde_json::Value {
    serde_json::json!({ "other": other })
}

/// 重复投影：同原始观测，或同来源实体且规范化几何完全一致。
fn exact_duplicate(a: &Candidate, b: &Candidate) -> bool {
    a.raw_observation_id == b.raw_observation_id
        || (a.source_entity_id == b.source_entity_id && a.shape == b.shape)
}

/// 同类别候选。
fn same_category(a: &Candidate, b: &Candidate) -> bool {
    a.category == b.category
}

/// 同名（去除首尾空白后相同且非空）。
fn same_trimmed_name(a: &Candidate, b: &Candidate) -> bool {
    let (na, nb) = (a.title.trim(), b.title.trim());
    !na.is_empty() && na == nb
}

/// 同质心（坐标差异小于 1e-6 度，约 0.1 米）。
fn same_centroid(a: &CandidateShape, b: &CandidateShape) -> bool {
    match (centroid(&a.coordinates), centroid(&b.coordinates)) {
        (Some((x1, y1)), Some((x2, y2))) => (x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6,
        _ => false,
    }
}

fn both_buildings(a: &Candidate, b: &Candidate) -> bool {
    a.category == CandidateCategory::Building && b.category == CandidateCategory::Building
}

/// 疑似重叠：两个面环包围盒相交，且任一环的顶点或质心落在另一环内。
fn polygons_overlap(a: &CandidateShape, b: &CandidateShape) -> bool {
    let (Some(ring_a), Some(ring_b)) = (ring(a), ring(b)) else {
        return false;
    };
    let (Some(bbox_a), Some(bbox_b)) = (bounding_box(&ring_a), bounding_box(&ring_b)) else {
        return false;
    };
    if !boxes_intersect(bbox_a, bbox_b) {
        return false;
    }
    let probes_a: Vec<(f64, f64)> = ring_a
        .iter()
        .copied()
        .chain(centroid(&a.coordinates))
        .collect();
    let probes_b: Vec<(f64, f64)> = ring_b
        .iter()
        .copied()
        .chain(centroid(&b.coordinates))
        .collect();
    probes_a.iter().any(|p| point_in_ring(&ring_b, *p))
        || probes_b.iter().any(|p| point_in_ring(&ring_a, *p))
}

/// 建筑形状可疑：点/线，或面环有效（去重）点数少于 4。
fn suspicious_shape(shape: &CandidateShape) -> bool {
    match shape.kind.as_str() {
        "point" | "line_string" => true,
        "polygon" => distinct_point_count(&shape.coordinates) < 4,
        _ => true,
    }
}

/// 从形状坐标提取点列表。
fn points_of(value: &serde_json::Value) -> Vec<(f64, f64)> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|item| {
            let parts = item.as_array()?;
            if parts.len() < 2 {
                return None;
            }
            Some((
                parts[0].as_f64().unwrap_or(f64::NAN),
                parts[1].as_f64().unwrap_or(f64::NAN),
            ))
        })
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect()
}

/// 面环点列表（多边形坐标数组）。
fn ring(shape: &CandidateShape) -> Option<Vec<(f64, f64)>> {
    if shape.kind != "polygon" {
        return None;
    }
    let points = points_of(&shape.coordinates);
    if points.len() < 3 {
        return None;
    }
    Some(points)
}

/// 点列表质心（空列表返回 None）。
fn centroid(value: &serde_json::Value) -> Option<(f64, f64)> {
    let points = points_of(value);
    if points.is_empty() {
        return None;
    }
    let sum = points
        .iter()
        .fold((0.0, 0.0), |acc, (x, y)| (acc.0 + x, acc.1 + y));
    Some((sum.0 / points.len() as f64, sum.1 / points.len() as f64))
}

/// 环内点判定：射线法（点在边界上视为内部，保守倾向"重叠"）。
fn point_in_ring(ring: &[(f64, f64)], point: (f64, f64)) -> bool {
    let (x, y) = point;
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

type Bbox = (f64, f64, f64, f64);

fn bounding_box(points: &[(f64, f64)]) -> Option<Bbox> {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (x, y) in points {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    if min_x.is_finite() && min_y.is_finite() {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

fn boxes_intersect(a: Bbox, b: Bbox) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

fn distinct_point_count(value: &serde_json::Value) -> usize {
    let points = points_of(value);
    let mut seen: Vec<(f64, f64)> = Vec::new();
    for point in points {
        if !seen
            .iter()
            .any(|(x, y)| (x - point.0).abs() < 1e-9 && (y - point.1).abs() < 1e-9)
        {
            seen.push(point);
        }
    }
    seen.len()
}

/// 把可执行建议（保留/剔除）聚合为确认请求与主要理由分布。
pub(crate) fn apply_request(
    keep: &[CandidateKey],
    remove: &[CandidateKey],
    candidates: &[Candidate],
) -> SuggestionApplyRequest {
    let mut reason_counts: Vec<(&'static str, usize)> = Vec::new();
    for key in keep.iter().chain(remove) {
        if let Some(candidate) = candidates.iter().find(|c| &c.key == key) {
            if let Some(suggestion) = candidate.suggestion.as_ref() {
                if let Some(entry) = reason_counts
                    .iter_mut()
                    .find(|(summary_key, _)| *summary_key == suggestion.summary_key)
                {
                    entry.1 += 1;
                } else {
                    reason_counts.push((suggestion.summary_key, 1));
                }
            }
        }
    }
    reason_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let reason_lines: Vec<ReasonLine> = reason_counts
        .into_iter()
        .map(|(summary_key, count)| ReasonLine { summary_key, count })
        .collect();
    SuggestionApplyRequest {
        count: keep.len() + remove.len(),
        keep_count: keep.len(),
        remove_count: remove.len(),
        reason_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confidence::ConfidenceFilter;
    use data_persistence::{CandidateNameSource, CandidateValidation};
    use shared_domain_types::ReviewState;

    fn polygon(coordinates: serde_json::Value) -> CandidateShape {
        CandidateShape::polygon(coordinates)
    }

    fn building(
        key: &str,
        title: &str,
        name_source: CandidateNameSource,
        shape: CandidateShape,
        tags: Vec<(String, String)>,
    ) -> Candidate {
        Candidate {
            key: CandidateKey::new(key),
            category: CandidateCategory::Building,
            title: title.to_owned(),
            named: !title.is_empty() && title != key,
            source: "overpass".to_owned(),
            tags,
            shape,
            state: ReviewState::Pending,
            selected: false,
            suggestion: None,
            name_source,
            validation: CandidateValidation::Retained,
            automatically_repaired: false,
            missing_in_latest_batch: false,
            isolation_reason: None,
            source_entity_id: title.split('/').next_back().unwrap_or(title).to_owned(),
            raw_observation_id: format!("raw:{key}"),
        }
    }

    fn distinct_ring(offset: f64) -> serde_json::Value {
        serde_json::json!([
            [121.4 + offset, 31.2],
            [121.5 + offset, 31.2],
            [121.5 + offset, 31.3],
            [121.4 + offset, 31.3],
            [121.4 + offset, 31.2]
        ])
    }

    #[test]
    fn clean_named_buildings_get_keep_suggestion_with_readable_reason() {
        let candidates = vec![building(
            "overpass:way/1:outer",
            "第一教学楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "school".to_owned())],
        )];
        let suggestions = SuggestionEngine::compute(&candidates);
        assert_eq!(suggestions.len(), 1);
        let (key, suggestion) = &suggestions[0];
        assert_eq!(key.candidate_id, "overpass:way/1:outer");
        assert_eq!(suggestion.category, SuggestionCategory::NoActionNeeded);
        assert_eq!(suggestion.action, SuggestionAction::Keep);
        assert_eq!(suggestion.reason_key, text_keys::SUGGESTION_REASON_KEEP);
    }

    #[test]
    fn unnamed_candidate_gets_unnamed_human_review_suggestion() {
        let candidates = vec![building(
            "overpass:way/2:outer",
            "way/2",
            CandidateNameSource::Unnamed,
            polygon(distinct_ring(0.0)),
            Vec::new(),
        )];
        let suggestions = SuggestionEngine::compute(&candidates);
        let (_, suggestion) = &suggestions[0];
        assert_eq!(suggestion.category, SuggestionCategory::Unnamed);
        assert_eq!(suggestion.action, SuggestionAction::HumanReview);
        assert_eq!(suggestion.reason_key, text_keys::SUGGESTION_REASON_UNNAMED);
    }

    #[test]
    fn overlapping_buildings_get_needs_attention_with_partner_reason() {
        // 同一位置的两栋建筑：包围盒相交且质心互在对方环内。
        let candidates = vec![
            building(
                "overpass:way/3:outer",
                "教学楼甲",
                CandidateNameSource::Osm,
                polygon(distinct_ring(0.0)),
                vec![("building".to_owned(), "school".to_owned())],
            ),
            building(
                "overpass:way/4:outer",
                "教学楼乙",
                CandidateNameSource::Osm,
                polygon(distinct_ring(0.001)),
                vec![("building".to_owned(), "school".to_owned())],
            ),
        ];
        let suggestions = SuggestionEngine::compute(&candidates);
        assert_eq!(suggestions.len(), 2);
        for (_, suggestion) in &suggestions {
            assert_eq!(suggestion.category, SuggestionCategory::NeedsAttention);
            assert_eq!(suggestion.action, SuggestionAction::HumanReview);
            assert_eq!(suggestion.reason_key, text_keys::SUGGESTION_REASON_OVERLAP);
            let other = suggestion.reason_args["other"]
                .as_str()
                .expect("other 参数");
            assert!(other == "教学楼甲" || other == "教学楼乙");
        }
    }

    #[test]
    fn exact_duplicate_suggests_removing_the_later_candidate_only() {
        let shape = polygon(distinct_ring(0.0));
        let candidates = vec![
            building(
                "overpass:way/5:outer",
                "体育馆",
                CandidateNameSource::Osm,
                shape.clone(),
                vec![("building".to_owned(), "yes".to_owned())],
            ),
            building(
                "overpass:way/6:outer",
                "体育馆",
                CandidateNameSource::Osm,
                shape,
                vec![("building".to_owned(), "yes".to_owned())],
            ),
        ];
        // 同一来源实体 + 同一几何 → 后者建议剔除。
        let mut candidates = candidates;
        candidates[1].source_entity_id = candidates[0].source_entity_id.clone();
        let suggestions = SuggestionEngine::compute(&candidates);
        let first = suggestions
            .iter()
            .find(|(key, _)| key.candidate_id == "overpass:way/5:outer")
            .map(|(_, s)| s)
            .expect("前序候选存在");
        let second = suggestions
            .iter()
            .find(|(key, _)| key.candidate_id == "overpass:way/6:outer")
            .map(|(_, s)| s)
            .expect("重复候选存在");
        assert_eq!(first.action, SuggestionAction::Keep, "先出现者保留");
        assert_eq!(second.action, SuggestionAction::Remove, "后出现者建议剔除");
        assert_eq!(
            second.reason_key,
            text_keys::SUGGESTION_REASON_EXACT_DUPLICATE
        );
        assert_eq!(second.reason_args["other"], "体育馆");
    }

    #[test]
    fn same_name_same_category_is_duplicate_suspicion_for_human_review() {
        let candidates = vec![
            building(
                "overpass:way/7:outer",
                "图书馆",
                CandidateNameSource::Osm,
                polygon(distinct_ring(0.0)),
                vec![("building".to_owned(), "yes".to_owned())],
            ),
            building(
                "overpass:way/8:outer",
                "图书馆",
                CandidateNameSource::Gaode,
                polygon(distinct_ring(0.2)),
                vec![("building".to_owned(), "yes".to_owned())],
            ),
        ];
        let suggestions = SuggestionEngine::compute(&candidates);
        for (_, suggestion) in &suggestions {
            assert_eq!(
                suggestion.reason_key,
                text_keys::SUGGESTION_REASON_DUPLICATE_SUSPECT
            );
            assert_eq!(suggestion.action, SuggestionAction::HumanReview);
        }
    }

    #[test]
    fn repaired_and_point_building_shapes_are_flagged_for_human_review() {
        let mut repaired = building(
            "overpass:way/9:outer",
            "实验楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "yes".to_owned())],
        );
        repaired.automatically_repaired = true;
        let point_building = building(
            "overpass:way/10:outer",
            "岗亭",
            CandidateNameSource::Osm,
            CandidateShape::point(serde_json::json!([121.9, 31.9])),
            vec![("building".to_owned(), "yes".to_owned())],
        );
        let suggestions = SuggestionEngine::compute(&[repaired, point_building]);
        let repaired_suggestion = suggestions
            .iter()
            .find(|(key, _)| key.candidate_id == "overpass:way/9:outer")
            .map(|(_, s)| s)
            .expect("修复候选存在");
        let point_suggestion = suggestions
            .iter()
            .find(|(key, _)| key.candidate_id == "overpass:way/10:outer")
            .map(|(_, s)| s)
            .expect("点位建筑存在");
        assert_eq!(
            repaired_suggestion.reason_key,
            text_keys::SUGGESTION_REASON_REPAIRED
        );
        assert_eq!(
            point_suggestion.reason_key,
            text_keys::SUGGESTION_REASON_SUSPICIOUS_SHAPE
        );
        for (_, suggestion) in &suggestions {
            assert_eq!(suggestion.action, SuggestionAction::HumanReview);
        }
    }

    #[test]
    fn d_isolation_reason_string_is_surfaced_when_present() {
        let mut candidate = building(
            "overpass:way/11:outer",
            "边界建筑",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "yes".to_owned())],
        );
        candidate.isolation_reason = Some("outside_confirmed_plan_boundary".to_owned());
        let suggestions = SuggestionEngine::compute(&[candidate]);
        assert_eq!(
            suggestions[0].1.reason_key,
            text_keys::SUGGESTION_REASON_ISOLATED
        );
        assert_eq!(
            suggestions[0].1.reason_args["reason"],
            "outside_confirmed_plan_boundary"
        );
        assert_eq!(suggestions[0].1.action, SuggestionAction::HumanReview);
    }

    #[test]
    fn missing_source_type_is_flagged_for_human_review() {
        let mut candidate = building(
            "overpass:way/17:outer",
            "教学楼丙",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "school".to_owned())],
        );
        candidate.source = String::new();
        let suggestions = SuggestionEngine::compute(&[candidate]);
        assert_eq!(
            suggestions[0].1.reason_key,
            text_keys::SUGGESTION_REASON_MISSING_SOURCE
        );
        assert_eq!(suggestions[0].1.action, SuggestionAction::HumanReview);
    }

    #[test]
    fn identical_input_produces_identical_suggestions() {
        let candidates = vec![
            building(
                "overpass:way/12:outer",
                "食堂",
                CandidateNameSource::Osm,
                polygon(distinct_ring(0.0)),
                vec![("building".to_owned(), "yes".to_owned())],
            ),
            building(
                "overpass:way/13:outer",
                "way/13",
                CandidateNameSource::Failed,
                polygon(distinct_ring(0.005)),
                Vec::new(),
            ),
            building(
                "overpass:way/14:outer",
                "食堂",
                CandidateNameSource::Cache,
                polygon(distinct_ring(0.001)),
                vec![("building".to_owned(), "yes".to_owned())],
            ),
        ];
        let first = SuggestionEngine::compute(&candidates);
        let second = SuggestionEngine::compute(&candidates);
        assert_eq!(first, second);
        // 打乱输入顺序也不影响结果（确定性以候选标识排序为准）。
        let mut shuffled = candidates.clone();
        shuffled.reverse();
        assert_eq!(SuggestionEngine::compute(&shuffled), first);
    }

    #[test]
    fn apply_request_aggregates_reason_distribution() {
        let mut a = building(
            "overpass:way/15:outer",
            "教学楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "yes".to_owned())],
        );
        a.suggestion = Some(rule_for(&a, None, None, None));
        let mut b = building(
            "overpass:way/16:outer",
            "岗亭",
            CandidateNameSource::Osm,
            CandidateShape::point(serde_json::json!([121.4, 31.2])),
            Vec::new(),
        );
        b.suggestion = Some(rule_for(&b, None, None, None));
        let request = apply_request(&[a.key.clone()], &[b.key.clone()], &[a, b]);
        assert_eq!(request.count, 2);
        assert_eq!(request.keep_count, 1);
        assert_eq!(request.remove_count, 1);
        assert_eq!(request.reason_lines.len(), 2);
        assert!(request
            .reason_lines
            .iter()
            .any(|line| line.summary_key == text_keys::SUGGESTION_SUMMARY_KEEP));
        assert!(request
            .reason_lines
            .iter()
            .any(|line| line.summary_key == text_keys::SUGGESTION_SUMMARY_SUSPICIOUS_SHAPE));
    }

    #[test]
    fn confidence_tier_is_derived_deterministically_from_rules() {
        let suggest = |candidate: &Candidate| rule_for(candidate, None, None, None);

        // 高：建议保留。
        let keep = building(
            "overpass:way/20:outer",
            "教学楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "school".to_owned())],
        );
        assert_eq!(suggest(&keep).confidence_tier(), ConfidenceTier::High);

        // 低：未命名。
        let unnamed = building(
            "overpass:way/21:outer",
            "way/21",
            CandidateNameSource::Unnamed,
            polygon(distinct_ring(0.0)),
            Vec::new(),
        );
        assert_eq!(suggest(&unnamed).confidence_tier(), ConfidenceTier::Low);

        // 低：自动修复过。
        let mut repaired = building(
            "overpass:way/22:outer",
            "实验楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "lab".to_owned())],
        );
        repaired.automatically_repaired = true;
        assert_eq!(suggest(&repaired).confidence_tier(), ConfidenceTier::Low);

        // 低：重复投影（建议剔除）。
        let duplicate = building(
            "overpass:way/23:outer",
            "体育馆",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "yes".to_owned())],
        );
        let tier = rule_for(&duplicate, Some("体育馆"), None, None).confidence_tier();
        assert_eq!(tier, ConfidenceTier::Low);

        // 中：形状可疑。
        let suspicious = building(
            "overpass:way/24:outer",
            "岗亭",
            CandidateNameSource::Osm,
            CandidateShape::point(serde_json::json!([121.9, 31.9])),
            vec![("building".to_owned(), "yes".to_owned())],
        );
        assert_eq!(
            suggest(&suspicious).confidence_tier(),
            ConfidenceTier::Medium
        );

        // 中：标签稀疏。
        let sparse = building(
            "overpass:way/25:outer",
            "教学楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            Vec::new(),
        );
        assert_eq!(suggest(&sparse).confidence_tier(), ConfidenceTier::Medium);

        // 中：缺少来源。
        let mut no_source = building(
            "overpass:way/26:outer",
            "教学楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "school".to_owned())],
        );
        no_source.source = String::new();
        assert_eq!(
            suggest(&no_source).confidence_tier(),
            ConfidenceTier::Medium
        );

        // 中：本次采集未找到。
        let mut missing_latest = building(
            "overpass:way/27:outer",
            "教学楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "school".to_owned())],
        );
        missing_latest.missing_in_latest_batch = true;
        assert_eq!(
            suggest(&missing_latest).confidence_tier(),
            ConfidenceTier::Medium
        );
    }

    #[test]
    fn confidence_filter_matches_derived_tier() {
        let mut high = building(
            "overpass:way/28:outer",
            "教学楼",
            CandidateNameSource::Osm,
            polygon(distinct_ring(0.0)),
            vec![("building".to_owned(), "school".to_owned())],
        );
        high.suggestion = Some(rule_for(&high, None, None, None));
        let suggestion = high.suggestion.as_ref().unwrap();
        assert!(ConfidenceFilter::All.matches(suggestion));
        assert!(ConfidenceFilter::High.matches(suggestion));
        assert!(!ConfidenceFilter::Medium.matches(suggestion));
        assert!(!ConfidenceFilter::Low.matches(suggestion));
    }
}
