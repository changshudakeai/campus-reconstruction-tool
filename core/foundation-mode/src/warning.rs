//! 朝向修改警告弹窗逻辑子模块
//!
//! **核心功能**：当用户修改朝向时，计算并告知哪些已生成数据会受影响重算
//! （ADR-0012：修改朝向会触发已生成数据的重算，需在界面明确告知影响范围）。
//!
//! 类别沿用 B1 的 [CandidateCategory]（六类别），朝向沿用 B1 的 [Orientation]，
//! 不另起炉灶。弹窗文案暂为中文硬编码，待 T03 文本外置后换文本键。

use shared_domain_types::{CandidateCategory, Orientation};
use std::collections::HashMap;

/// 影响项详情：某一类别下有多少已生成对象要重算
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactItem {
    /// 六类别之一（B1 共享类型）
    pub category: CandidateCategory,
    /// 受影响的项目数量
    pub count: usize,
    /// 是否需要用户二次确认（建筑/体育体量大，重算代价高）
    pub requires_confirmation: bool,
}

impl ImpactItem {
    fn new(category: CandidateCategory, count: usize) -> Self {
        let requires_confirmation = matches!(
            category,
            CandidateCategory::Building | CandidateCategory::Sports
        );
        Self {
            category,
            count,
            requires_confirmation,
        }
    }
}

/// 朝向修改的影响报告（供 UI 层弹窗展示）
#[derive(Debug, Clone, PartialEq)]
pub struct OrientationImpactReport {
    /// 受影响的类别列表（按类别数量列出）
    pub items: Vec<ImpactItem>,
    /// 是否全部可安全重算（不含需二次确认的项目）
    pub all_safe_to_skip: bool,
    /// 弹窗标题
    pub title: String,
    /// 弹窗正文
    pub details: String,
}

/// 计算朝向修改的影响范围
///
/// 参数：
/// - `existing_data`: 当前已生成的数据分布 `{类别 → 项目数}`
/// - `old_orientation`: 原有朝向（首次设定时为 None）
/// - `new_orientation`: 新朝向（B1 类型自带 0~360 校验）
pub fn check_orientation_change_impact(
    existing_data: &HashMap<CandidateCategory, usize>,
    old_orientation: Option<Orientation>,
    new_orientation: Orientation,
) -> OrientationImpactReport {
    let mut items: Vec<ImpactItem> = existing_data
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(category, count)| ImpactItem::new(*category, *count))
        .collect();
    // 按类别优先级排序（建筑 > 体育 > 水域 > 道路 > 植被 > 其他），弹窗列表稳定
    items.sort_by_key(|item| std::cmp::Reverse(item.category.priority()));

    let has_confirmation_required = items.iter().any(|item| item.requires_confirmation);

    let (title, details) = match old_orientation {
        Some(old) => {
            let raw_diff = (f64::from(new_orientation.degree()) - f64::from(old.degree())).abs();
            let diff_degrees = if raw_diff > 180.0 {
                360.0 - raw_diff
            } else {
                raw_diff
            };

            let impact_desc = if diff_degrees < 5.0 {
                "微小调整"
            } else if diff_degrees < 30.0 {
                "小幅旋转"
            } else if diff_degrees < 90.0 {
                "明显偏转"
            } else {
                "方向倒置"
            };

            (
                format!("修改朝向将触发重算（{impact_desc}）"),
                format!(
                    "当前朝向：{:.1}°\n新朝向：{:.1}°（偏差 {:.1}°，{}）",
                    old.degree(),
                    new_orientation.degree(),
                    diff_degrees,
                    impact_desc
                ),
            )
        }
        None => (
            "首次设定朝向".to_string(),
            format!("新朝向：{:.1}°", new_orientation.degree()),
        ),
    };

    let all_safe_to_skip = !has_confirmation_required;

    OrientationImpactReport {
        items,
        all_safe_to_skip,
        title,
        details,
    }
}

/// 判断是否需要弹出二次确认窗口
pub fn should_show_confirmation_dialog(report: &OrientationImpactReport) -> bool {
    report.items.iter().any(|item| item.requires_confirmation)
}

/// 格式化影响详情（供 UI 弹窗正文显示）
pub fn format_impact_details(report: &OrientationImpactReport) -> String {
    let mut lines = vec![report.details.clone(), String::new()];

    if report.items.is_empty() {
        lines.push("当前尚无已生成数据，修改朝向不会触发任何重算。".to_string());
    } else {
        lines.push("以下已生成数据将被重新计算：".to_string());
        for item in &report.items {
            let marker = if item.requires_confirmation {
                "⚠"
            } else {
                "•"
            };
            lines.push(format!(
                "{marker} {}：{} 项",
                item.category.display_name(),
                item.count
            ));
        }
        if should_show_confirmation_dialog(report) {
            lines.push(String::new());
            lines.push("注意：建筑/体育场地重算代价较高，请确认后再继续。".to_string());
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orientation(degree: f32) -> Orientation {
        Orientation::new(degree).expect("测试角度在 0~360 内")
    }

    #[test]
    fn first_time_setup_has_no_impact_items() {
        let report = check_orientation_change_impact(&HashMap::new(), None, orientation(90.0));

        assert!(report.items.is_empty());
        assert_eq!(report.title, "首次设定朝向");
        assert!(format_impact_details(&report).contains("不会触发任何重算"));
    }

    #[test]
    fn minor_change_is_labelled_and_building_needs_confirmation() {
        let mut existing = HashMap::new();
        existing.insert(CandidateCategory::Building, 15);
        existing.insert(CandidateCategory::Vegetation, 42);

        let report =
            check_orientation_change_impact(&existing, Some(orientation(0.0)), orientation(3.0));

        assert_eq!(report.items.len(), 2);
        assert!(report.title.contains("微小调整"));
        assert!(should_show_confirmation_dialog(&report));
        assert!(!report.all_safe_to_skip);
    }

    #[test]
    fn opposite_direction_is_labelled_as_reversal() {
        let mut existing = HashMap::new();
        existing.insert(CandidateCategory::Road, 20);

        let report =
            check_orientation_change_impact(&existing, Some(orientation(0.0)), orientation(180.0));

        assert!(report.title.contains("方向倒置"));
        assert!(report.details.contains("180.0"));
    }

    #[test]
    fn wrap_around_diff_uses_shorter_arc() {
        let mut existing = HashMap::new();
        existing.insert(CandidateCategory::Water, 5);

        // 350° → 10° 的实际偏差是 20°，不是 340°
        let report =
            check_orientation_change_impact(&existing, Some(orientation(350.0)), orientation(10.0));

        assert!(report.details.contains("20.0"));
        assert!(report.title.contains("小幅旋转"));
    }

    #[test]
    fn items_are_sorted_by_category_priority() {
        let mut existing = HashMap::new();
        existing.insert(CandidateCategory::Vegetation, 50);
        existing.insert(CandidateCategory::Building, 10);
        existing.insert(CandidateCategory::Water, 7);

        let report =
            check_orientation_change_impact(&existing, Some(orientation(45.0)), orientation(90.0));

        assert_eq!(report.items[0].category, CandidateCategory::Building);
        assert_eq!(report.items[1].category, CandidateCategory::Water);
        assert_eq!(report.items[2].category, CandidateCategory::Vegetation);
    }

    #[test]
    fn vegetation_only_changes_are_safe() {
        let mut existing = HashMap::new();
        existing.insert(CandidateCategory::Vegetation, 50);
        existing.insert(CandidateCategory::Other, 7);

        let report =
            check_orientation_change_impact(&existing, Some(orientation(45.0)), orientation(90.0));

        assert!(report.all_safe_to_skip);
        assert!(!should_show_confirmation_dialog(&report));
        let formatted = format_impact_details(&report);
        assert!(formatted.contains("植被：50 项"));
        assert!(!formatted.contains("请确认后再继续"));
    }
}
