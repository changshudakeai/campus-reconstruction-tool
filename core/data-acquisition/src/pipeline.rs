//! 采集流水线（窗口契约缝 3 的完整走线）。
//!
//! 一次 [`AcquisitionPipeline::collect`] 调用走完：给定边界 → 数据源拉取
//! 原始对象 → B13 归类 → 增量刷新比对（digest）→ B2 原始观测落库。
//! 归类逻辑完全复用 B13 `ClassifyEngine`；落库完全走 B2
//! `RawObservationsApi`（数据粮仓铁律：只写不删，重复采集按指纹刷新）。

use std::collections::BTreeMap;

use data_persistence::{Database, RawObservation, RawObservationsApi};
use data_transformers::ClassifyEngine;
use shared_domain_types::{Boundary, CandidateCategory, PlanId};

use crate::error::{AcquisitionError, Result};
use crate::refresh::{DiffEntry, DiffKind, RefreshDiff};
use crate::source::DataSource;

/// 一次采集的结果报告（进度视图与差异展示的数据来源）
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionReport {
    /// 采集所属方案 ID（文本形式，与 B2 主键一致）
    pub plan_id: String,
    /// 数据源标识（来源全程可见）
    pub source_tag: String,
    /// 本次拉回的对象总数
    pub total: usize,
    /// 实际写库行数（新增 + 更新；未变的被 B2 原样保留不计入）
    pub written: usize,
    /// 各类别对象数（"完成了 N 个对象"的分类明细）
    pub category_counts: BTreeMap<CandidateCategory, usize>,
    /// 兜底进"其他"的对象数（映射不上但绝不静默丢弃，评审可见）
    pub fallback_count: usize,
    /// 相对上次采集的增量差异（新增/更新/未变）
    pub diff: RefreshDiff,
}

/// 采集流水线：持有 B13 归类引擎，对任意 [`DataSource`] 执行缝 3 走线
#[derive(Debug, Clone)]
pub struct AcquisitionPipeline {
    engine: ClassifyEngine,
}

impl AcquisitionPipeline {
    /// 用 B13 内嵌默认映射表创建流水线
    pub fn new() -> Result<Self> {
        Ok(Self {
            engine: ClassifyEngine::with_default_mapping()?,
        })
    }

    /// 用外部构建的归类引擎创建（映射表已由 B13 校验）
    pub fn with_engine(engine: ClassifyEngine) -> Self {
        Self { engine }
    }

    /// 当前使用的归类引擎（只读）
    pub fn engine(&self) -> &ClassifyEngine {
        &self.engine
    }

    /// 执行一次采集：边界 → 原始对象 → 归类 → 增量比对 → 落库。
    ///
    /// - 空边界拒绝（ADR-0012：圈画边界是必经第一步）；
    /// - 差异比对在写库前完成：与库中同 (方案, 类别, 实体) 的 digest 对照；
    /// - 类别变化的对象在新类别下记"新增"，旧行按粮仓铁律原地保留。
    pub fn collect(
        &self,
        db: &mut Database,
        plan_id: &PlanId,
        boundary: &Boundary,
        source: &dyn DataSource,
    ) -> Result<CollectionReport> {
        if boundary.is_empty() {
            return Err(AcquisitionError::EmptyBoundary);
        }

        let entities = source.fetch_raw_entities(boundary)?;
        let plan_key = plan_id.to_string();

        let mut observations = Vec::new();
        let mut diff_entries = Vec::new();
        let mut category_counts: BTreeMap<CandidateCategory, usize> = BTreeMap::new();
        let mut fallback_count = 0usize;

        for entity in &entities {
            let classification = self.engine.classify(&entity.tags);
            if classification.is_fallback {
                fallback_count += 1;
            }
            let source_data = entity.to_source_data();
            let digest = RawObservation::compute_digest(&source_data);

            // 增量刷新检测：写库前与上次采集的内容指纹对照
            let kind = match db.find_raw_observation(
                &plan_key,
                classification.category,
                &entity.entity_id,
            )? {
                None => DiffKind::Added,
                Some(existing) if existing.digest == digest => DiffKind::Unchanged,
                Some(_) => DiffKind::Updated,
            };
            diff_entries.push(DiffEntry {
                category: classification.category,
                entity_id: entity.entity_id.clone(),
                kind,
            });
            *category_counts.entry(classification.category).or_insert(0) += 1;

            observations.push(RawObservation::new(
                &plan_key,
                classification.category,
                &entity.entity_id,
                source_data,
                source.source_tag(),
            ));
        }

        // 数据粮仓落库：单事务原子提交，未变的行由 B2 原样保留
        let written = db.write_raw_observations(&observations)?;

        Ok(CollectionReport {
            plan_id: plan_key,
            source_tag: source.source_tag().to_owned(),
            total: entities.len(),
            written,
            category_counts,
            fallback_count,
            diff: RefreshDiff::new(diff_entries),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::RawEntity;
    use data_transformers::TagMap;

    /// 罐头数据源：固定返回预置对象（离线测试替身）
    struct FakeSource {
        entities: Vec<RawEntity>,
    }

    impl DataSource for FakeSource {
        fn source_tag(&self) -> &str {
            "fake"
        }

        fn fetch_raw_entities(&self, _boundary: &Boundary) -> Result<Vec<RawEntity>> {
            Ok(self.entities.clone())
        }
    }

    fn entity(id: &str, key: &str, value: &str) -> RawEntity {
        let mut tags = TagMap::new();
        tags.insert(key.to_owned(), value.to_owned());
        RawEntity::new(id, format!("对象{id}"), tags, serde_json::json!({"id": id}))
    }

    fn boundary() -> Boundary {
        Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [121.40, 31.20],
                [121.41, 31.20],
                [121.41, 31.21],
                [121.40, 31.20]
            ]]),
        }
    }

    #[test]
    fn empty_boundary_is_rejected() {
        let pipeline = AcquisitionPipeline::new().unwrap();
        let mut db = Database::open_in_memory().unwrap();
        let source = FakeSource {
            entities: Vec::new(),
        };
        let err = pipeline
            .collect(&mut db, &PlanId::generate(), &Boundary::empty(), &source)
            .unwrap_err();
        assert!(matches!(err, AcquisitionError::EmptyBoundary));
    }

    #[test]
    fn fallback_objects_are_counted_not_dropped() {
        let pipeline = AcquisitionPipeline::new().unwrap();
        let mut db = Database::open_in_memory().unwrap();
        let source = FakeSource {
            entities: vec![
                entity("known", "building", "school"),
                entity("mystery", "unknown_key", "unknown_value"),
            ],
        };
        let report = pipeline
            .collect(&mut db, &PlanId::generate(), &boundary(), &source)
            .unwrap();
        assert_eq!(report.total, 2);
        assert_eq!(report.fallback_count, 1);
        assert_eq!(
            report.category_counts.get(&CandidateCategory::Other),
            Some(&1),
            "映射不上的对象归'其他'，禁止静默丢弃"
        );
    }
}
