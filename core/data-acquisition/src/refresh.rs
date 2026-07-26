//! 增量刷新检测框架：对比上次采集的内容指纹，展示 新增/更新/未变。
//!
//! 数据粮仓铁律下没有"删除"一说：上次有、这次没拉到的对象仍留在库里；
//! 差异只报三种——**新增**（库里没有）、**更新**（digest 变了）、
//! **未变**（digest 相同，B2 UPSERT 会原样保留该行）。

use std::collections::BTreeMap;

use shared_domain_types::CandidateCategory;

use crate::progress::text_keys;

/// 单个对象相对上次采集的差异种类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// 新增：库中不存在该实体
    Added,
    /// 更新：内容指纹与库中不同（原始数据变了）
    Updated,
    /// 未变：内容指纹相同（落库时原样保留）
    Unchanged,
}

impl DiffKind {
    /// 对应的 B6 文本键（UI 层解析显示）
    pub fn text_key(&self) -> &'static str {
        match self {
            Self::Added => text_keys::DIFF_ADDED,
            Self::Updated => text_keys::DIFF_UPDATED,
            Self::Unchanged => text_keys::DIFF_UNCHANGED,
        }
    }
}

/// 一条差异记录（对象粒度）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    /// 归入的类别
    pub category: CandidateCategory,
    /// 真实世界对象 ID
    pub entity_id: String,
    /// 差异种类
    pub kind: DiffKind,
}

/// 一次采集相对上次的完整差异（供 UI 展示）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshDiff {
    entries: Vec<DiffEntry>,
}

impl RefreshDiff {
    /// 从逐条差异构建
    pub fn new(entries: Vec<DiffEntry>) -> Self {
        Self { entries }
    }

    /// 全部差异条目（只读）
    pub fn entries(&self) -> &[DiffEntry] {
        &self.entries
    }

    /// 新增条数
    pub fn added_count(&self) -> usize {
        self.count(DiffKind::Added)
    }

    /// 更新条数
    pub fn updated_count(&self) -> usize {
        self.count(DiffKind::Updated)
    }

    /// 未变条数
    pub fn unchanged_count(&self) -> usize {
        self.count(DiffKind::Unchanged)
    }

    /// 是否有实际变化（新增或更新 ≥1）
    pub fn has_changes(&self) -> bool {
        self.added_count() + self.updated_count() > 0
    }

    /// 汇总文案的文本键（占位符 {added}/{updated}/{unchanged}）
    pub fn summary_key(&self) -> &'static str {
        text_keys::DIFF_SUMMARY
    }

    /// 按类别统计差异（方便 UI 绑定 "类别 → 差异统计"）
    ///
    /// 返回 (新增，更新，未变) 三元组。
    pub fn by_category(&self) -> BTreeMap<CandidateCategory, (usize, usize, usize)> {
        let mut map: BTreeMap<CandidateCategory, (usize, usize, usize)> = BTreeMap::new();
        for e in &self.entries {
            let entry = map.entry(e.category).or_insert((0, 0, 0));
            match e.kind {
                DiffKind::Added => entry.0 += 1,
                DiffKind::Updated => entry.1 += 1,
                DiffKind::Unchanged => entry.2 += 1,
            }
        }
        map
    }

    fn count(&self, kind: DiffKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, kind: DiffKind) -> DiffEntry {
        DiffEntry {
            category: CandidateCategory::Building,
            entity_id: id.to_owned(),
            kind,
        }
    }

    #[test]
    fn counts_by_kind() {
        let diff = RefreshDiff::new(vec![
            entry("a", DiffKind::Added),
            entry("b", DiffKind::Added),
            entry("c", DiffKind::Updated),
            entry("d", DiffKind::Unchanged),
        ]);
        assert_eq!(diff.added_count(), 2);
        assert_eq!(diff.updated_count(), 1);
        assert_eq!(diff.unchanged_count(), 1);
        assert!(diff.has_changes());
        assert_eq!(diff.entries().len(), 4);

        // 按类别统计（全部同一类）
        let by_cat = diff.by_category();
        assert_eq!(by_cat[&CandidateCategory::Building], (2, 1, 1));
    }

    #[test]
    fn all_unchanged_means_no_changes() {
        let diff = RefreshDiff::new(vec![entry("a", DiffKind::Unchanged)]);
        assert!(!diff.has_changes());
    }

    #[test]
    fn kinds_expose_text_keys() {
        assert_eq!(DiffKind::Added.text_key(), "collection.diff_added");
        assert_eq!(DiffKind::Updated.text_key(), "collection.diff_updated");
        assert_eq!(DiffKind::Unchanged.text_key(), "collection.diff_unchanged");
    }
}
