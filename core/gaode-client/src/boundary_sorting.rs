//! T24: OSM 边界候选排序与选择 (Rust side)
//!
//! ADR-0029 要求：多个 Overpass 结果 → **自动选取最匹配一条**，无候选列表 UI
//! 排序规则：包含锚点优先 → 名称匹配 → 距离最近 (全部在 Rust 可单测)
//!
//! 归属说明：排序器操作 B3 自有的 `OsmElement` 类型，且 ADR-0029 职责分层
//! 明确“Rust domain（B3/B5）：候选排序选最佳”——落在 B3 满足
//! “基础层横向零依赖”架构红线（B5 → B3 被 xtask arch 禁止）。
//!
//! 输入：`OsmElement` × N + 校区锚点 (WGS-84)
//! 输出：最佳匹配的 WGS-84 原始坐标（由壳经 evaluate_script 发回 JS，
//! AMap.convertFrom 转 GCJ-02 后才上屏——未转换坐标禁止上屏）

use crate::OsmElement;

/// OSM 边界候选评分
#[derive(Debug, Clone)]
pub struct BoundaryCandidateScore {
    pub element: OsmElement,
    /// 是否包含锚点 (最高权重)
    pub contains_anchor: bool,
    /// 名称匹配度 (0.0~1.0)
    pub name_match_score: f64,
    /// 中心点距离锚点 (米)
    pub distance_to_anchor_m: f64,
    /// 综合排名分 (用于最终排序)
    pub rank_score: f64,
}

impl BoundaryCandidateScore {
    /// 新建候选评分
    pub fn new(element: OsmElement) -> Self {
        Self {
            contains_anchor: false,
            name_match_score: 0.0,
            distance_to_anchor_m: f64::MAX,
            rank_score: 0.0,
            element,
        }
    }
}

/// T24: OSM 边界排序器（B3 域内，纯函数可单测）
pub struct BoundarySorter {}

impl BoundarySorter {
    /// 从多个 OSM 元素中选取最佳匹配 (T24 核心算法)
    ///
    /// **排序优先级**:
    /// 1. `contains_anchor`: 多边形 bbox 包含锚点 (布尔，最高权)
    /// 2. `name_match_score`: name 标签 vs 校区名 (0~1, 高优)
    /// 3. `distance_to_anchor_m`: 中心点直线距离 (低权，tie-breaker)
    ///
    /// 返回排序后的向量 (最佳在前); 若为空或所有要素无效则返回空
    pub fn sort_candidates(
        elements: Vec<OsmElement>,
        anchor_lon: f64,
        anchor_lat: f64,
        requested_campus_name: Option<&str>,
    ) -> Vec<BoundaryCandidateScore> {
        let mut candidates: Vec<BoundaryCandidateScore> = elements
            .into_iter()
            .filter_map(|elem| {
                // 只保留有 geometry 的要素（提取坐标后 move elem）
                let coords = elem.geometry.clone()?;
                if coords.is_empty() {
                    return None;
                }

                let mut candidate = BoundaryCandidateScore::new(elem);

                // 1. 检查是否包含锚点 (bbox 粗略判断)
                candidate.contains_anchor =
                    Self::polygon_contains_bbox(&coords, anchor_lon, anchor_lat);

                // 2. 名称匹配
                if let Some(name) = requested_campus_name {
                    if let Some(elem_name) = candidate.element.tags.get("name") {
                        candidate.name_match_score = Self::calculate_name_match(name, elem_name);
                    }
                }

                // 3. 计算距离
                let center = Self::compute_center(&coords);
                candidate.distance_to_anchor_m =
                    Self::haversine_distance(anchor_lon, anchor_lat, center[0], center[1]);

                Some(candidate)
            })
            .collect();

        // 排序：contains_anchor(降) → name_match(降) → distance(升)
        candidates.sort_by(|a, b| {
            // 1. 含锚点的排在前面
            if a.contains_anchor != b.contains_anchor {
                return b.contains_anchor.cmp(&a.contains_anchor);
            }

            // 2. 名称匹配高的在前
            if (a.name_match_score - b.name_match_score).abs() > 1e-6 {
                return b.name_match_score.total_cmp(&a.name_match_score);
            }

            // 3. 距离近的在前
            a.distance_to_anchor_m
                .total_cmp(&b.distance_to_anchor_m)
                .then_with(|| a.element.id.cmp(&b.element.id)) // 稳定排序：id tie-breaker
        });

        // 计算综合排名分 (可用于调试/日志)
        for c in candidates.iter_mut() {
            c.rank_score = Self::compute_rank_score(c);
        }

        candidates
    }

    /// 获取最佳匹配 (None = 无数据)
    pub fn best_match(
        elements: Vec<OsmElement>,
        anchor_lon: f64,
        anchor_lat: f64,
        requested_campus_name: Option<&str>,
    ) -> Option<Vec<[f64; 2]>> {
        let sorted = Self::sort_candidates(elements, anchor_lon, anchor_lat, requested_campus_name);
        sorted.into_iter().next().and_then(|c| c.element.geometry)
    }

    /// 多边形 bbox 是否包含点 (粗略过滤，足够用于第一级筛选)
    fn polygon_contains_bbox(coords: &[[f64; 2]], point_lng: f64, point_lat: f64) -> bool {
        let mut min_lng = f64::MAX;
        let mut max_lng = f64::MIN;
        let mut min_lat = f64::MAX;
        let mut max_lat = f64::MIN;

        for c in coords {
            if c[0] < min_lng {
                min_lng = c[0];
            }
            if c[0] > max_lng {
                max_lng = c[0];
            }
            if c[1] < min_lat {
                min_lat = c[1];
            }
            if c[1] > max_lat {
                max_lat = c[1];
            }
        }

        point_lng >= min_lng && point_lng <= max_lng && point_lat >= min_lat && point_lat <= max_lat
    }

    /// 计算中心点
    fn compute_center(coords: &[[f64; 2]]) -> [f64; 2] {
        let sum_lng: f64 = coords.iter().map(|c| c[0]).sum();
        let sum_lat: f64 = coords.iter().map(|c| c[1]).sum();
        let n = coords.len() as f64;
        [sum_lng / n, sum_lat / n]
    }

    /// Haversine 距离 (米)
    fn haversine_distance(lng1: f64, lat1: f64, lng2: f64, lat2: f64) -> f64 {
        const EARTH_RADIUS_M: f64 = 6378137.0;

        let d_lat = (lat2 - lat1) * std::f64::consts::PI / 180.0;
        let d_lng = (lng2 - lng1) * std::f64::consts::PI / 180.0;
        let a = (d_lat / 2.0).sin().powi(2)
            + (lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lng / 2.0).sin().powi(2));
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

        EARTH_RADIUS_M * c
    }

    /// 名称匹配度 (0.0~1.0)
    fn calculate_name_match(requested: &str, source: &str) -> f64 {
        let req_lower = requested.to_lowercase();
        let src_lower = source.to_lowercase();

        if req_lower == src_lower {
            1.0 // 完全匹配
        } else if src_lower.contains(&req_lower) || req_lower.contains(&src_lower) {
            0.85 // 子串包含
        } else {
            0.35 // 无关联
        }
    }

    /// 计算综合排名分 (仅用于日志；排序已用多重键)
    fn compute_rank_score(c: &BoundaryCandidateScore) -> f64 {
        let anchor_bonus = if c.contains_anchor { 1000.0 } else { 0.0 };
        let name_score = c.name_match_score * 100.0;
        let dist_penalty = -c.distance_to_anchor_m.min(10000.0) / 100.0;
        anchor_bonus + name_score + dist_penalty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_element(id: i64, coords: &[[f64; 2]; 4], name: Option<&str>) -> OsmElement {
        let mut tags = HashMap::new();
        if let Some(n) = name {
            tags.insert("name".to_string(), n.to_string());
            tags.insert("amenity".to_string(), "university".to_string());
        }
        OsmElement {
            r#type: "way".to_string(),
            id,
            geometry: Some(coords.to_vec()),
            members: vec![],
            tags,
        }
    }

    #[test]
    fn anchor_containment_wins() {
        // 候选 A: 含锚点但名称不匹配
        // 候选 B: 不含锚点但名称完全匹配
        // 预期：A 排第一

        let anchor_lng = 116.4074;
        let anchor_lat = 39.9042;

        // A: bbox 包含锚点 (116.4~116.5, 39.9~40.0)
        let coords_a = [[116.4, 39.9], [116.5, 39.9], [116.5, 40.0], [116.4, 40.0]];
        let elem_a = make_element(1, &coords_a, None);

        // B: bbox 不包含锚点 (121.0~121.1, 31.0~31.1)
        let coords_b = [[121.0, 31.0], [121.1, 31.0], [121.1, 31.1], [121.0, 31.1]];
        let mut tags_b = HashMap::new();
        tags_b.insert("name".to_string(), "北京大学".to_string());
        tags_b.insert("amenity".to_string(), "university".to_string());
        let elem_b = OsmElement {
            r#type: "way".to_string(),
            id: 2,
            geometry: Some(coords_b.to_vec()),
            members: vec![],
            tags: tags_b,
        };

        let sorted = BoundarySorter::sort_candidates(
            vec![elem_a, elem_b],
            anchor_lng,
            anchor_lat,
            Some("北京"),
        );

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].element.id, 1); // A 第一
        assert_eq!(sorted[1].element.id, 2); // B 第二
    }

    #[test]
    fn name_match_breaks_tie_when_no_anchor() {
        // 两者都不含锚点 → 名称匹配高的在前
        let anchor_lng = 116.4074;
        let anchor_lat = 39.9042;

        let coords = [[121.0, 31.0], [121.1, 31.0], [121.1, 31.1], [121.0, 31.1]];

        let mut tags_match = HashMap::new();
        tags_match.insert("name".to_string(), "华东师范大学".to_string());
        tags_match.insert("amenity".to_string(), "university".to_string());
        let elem_match = OsmElement {
            r#type: "way".to_string(),
            id: 1,
            geometry: Some(coords.to_vec()),
            members: vec![],
            tags: tags_match,
        };

        let mut tags_nomatch = HashMap::new();
        tags_nomatch.insert("name".to_string(), "复旦大学".to_string());
        tags_nomatch.insert("amenity".to_string(), "university".to_string());
        let elem_nomatch = OsmElement {
            r#type: "way".to_string(),
            id: 2,
            geometry: Some(coords.to_vec()),
            members: vec![],
            tags: tags_nomatch,
        };

        let sorted = BoundarySorter::sort_candidates(
            vec![elem_match, elem_nomatch],
            anchor_lng,
            anchor_lat,
            Some("华东师大"),
        );

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].element.id, 1); // 名称匹配的在前
        assert_eq!(sorted[1].element.id, 2);
    }

    #[test]
    fn distance_breaks_tie_for_same_anchor_and_name() {
        // 两者都含锚点且名称相同 → 距离近的在前
        let anchor_lng = 116.4074;
        let anchor_lat = 39.9042;

        // A: 中心很近 (~0km)
        let coords_a = [[116.4, 39.9], [116.5, 39.9], [116.5, 40.0], [116.4, 40.0]];
        let mut tags_a = HashMap::new();
        tags_a.insert("name".to_string(), "测试大学".to_string());
        tags_a.insert("amenity".to_string(), "university".to_string());
        let elem_a = OsmElement {
            r#type: "way".to_string(),
            id: 1,
            geometry: Some(coords_a.to_vec()),
            members: vec![],
            tags: tags_a,
        };

        // B: 中心很远 (~1000km)
        let coords_b = [[113.0, 23.0], [113.1, 23.0], [113.1, 23.1], [113.0, 23.1]];
        let mut tags_b = HashMap::new();
        tags_b.insert("name".to_string(), "测试大学".to_string());
        tags_b.insert("amenity".to_string(), "university".to_string());
        let elem_b = OsmElement {
            r#type: "way".to_string(),
            id: 2,
            geometry: Some(coords_b.to_vec()),
            members: vec![],
            tags: tags_b,
        };

        let sorted = BoundarySorter::sort_candidates(
            vec![elem_a, elem_b],
            anchor_lng,
            anchor_lat,
            Some("测试大学"),
        );

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].element.id, 1); // A 更近
        assert_eq!(sorted[1].element.id, 2);
    }

    #[test]
    fn empty_list_returns_none() {
        let result = BoundarySorter::best_match(vec![], 116.4, 39.9, Some("test"));
        assert!(result.is_none());
    }

    #[test]
    fn haversine_distance_knows_beijing_to_shanghai() {
        // 北京 ~116.4, 39.9; 上海 ~121.5, 31.2; 实际距离约 1068km
        let dist = BoundarySorter::haversine_distance(116.4, 39.9, 121.5, 31.2);
        assert!(
            dist > 1000000.0 && dist < 1100000.0,
            "距离应为~1068km, 实际{}km",
            dist / 1000.0
        );
    }
}
