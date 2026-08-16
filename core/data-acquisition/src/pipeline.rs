//! 采集流水线（窗口契约缝 3 的完整走线）。
//!
//! 一次 [`AcquisitionPipeline::collect`] 调用走完：给定边界 → 数据源拉取
//! 原始对象 → B13 归类 → 增量刷新比对（digest）→ B2 原始观测落库。
//! 归类逻辑完全复用 B13 `ClassifyEngine`；落库完全走 B2
//! `RawObservationsApi`（数据粮仓铁律：只写不删，重复采集按指纹刷新）。

use std::collections::BTreeMap;
use std::time::Instant;

use data_persistence::{CandidateNameSource, Database, RawObservation, RawObservationsApi};
use data_transformers::ClassifyEngine;
use geo::{Contains, Intersects, Line, LineString, MultiPolygon, Point, Polygon};
use shared_domain_types::{Boundary, CandidateCategory, PlanId};

use crate::error::{AcquisitionError, Result};
use crate::progress::{CollectionStage, StageListener};
use crate::refresh::{DiffEntry, DiffKind, RefreshDiff};
use crate::source::{DataSource, SourceGeometry};

/// 一次采集的结果报告（进度视图与差异展示的数据来源）
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionReport {
    /// 采集所属方案 ID（文本形式，与 B2 主键一致）
    pub plan_id: String,
    /// 数据源标识（来源全程可见）
    pub source_tag: String,
    /// 本次拉回的对象总数
    pub total: usize,
    /// 坐标转换后完整位于已确认方案边界内的来源对象数。
    pub boundary_inside: usize,
    /// 与已确认方案边界相交但不完整位于其中的来源对象数。
    pub boundary_crossing: usize,
    /// 完整位于已确认方案边界外的来源对象数。
    pub boundary_outside: usize,
    /// 缺少或无法安全解析来源几何的对象数。
    pub invalid_geometry: usize,
    /// 实际写库行数（新增 + 更新；未变的被 B2 原样保留不计入）
    pub written: usize,
    /// 各类别对象数（"完成了 N 个对象"的分类明细）
    pub category_counts: BTreeMap<CandidateCategory, usize>,
    /// 兜底进"其他"的对象数（映射不上但绝不静默丢弃，评审可见）
    pub fallback_count: usize,
    /// 相对上次采集的增量差异（新增/更新/未变）
    pub diff: RefreshDiff,
    /// 本次是否“部分建筑未命名”（补名截止 / 上限 / 调用失败导致）
    pub naming_partial: bool,
}

/// F4 交给 A1 的完整、尚未发布候选投影的采集批次。
#[derive(Debug, Clone, PartialEq)]
pub struct AcquisitionBatch {
    pub plan_id: String,
    pub source_tag: String,
    pub raw_observations: Vec<RawObservation>,
    pub candidate_drafts: Vec<CandidateDraft>,
    pub category_counts: BTreeMap<CandidateCategory, usize>,
    pub fallback_count: usize,
    pub diff: RefreshDiff,
    pub total_source_object_count: usize,
    pub boundary_inside: usize,
    pub boundary_crossing: usize,
    pub boundary_outside: usize,
    pub invalid_geometry: usize,
    pub geometry_object_count: usize,
    pub missing_geometry_object_count: usize,
    pub naming_partial: bool,
}

/// 采集来源派生的候选草稿；资格与验证结论留给 A1/B14。
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateDraft {
    pub raw_observation_id: String,
    pub data_source_tag: String,
    pub source_entity_id: String,
    pub geometry_part_id: String,
    pub category: CandidateCategory,
    pub name: String,
    /// 当前名称来源（OSM 原始名称，或 A1 在 B14 验证后补名的来源）。
    pub name_source: CandidateNameSource,
    pub source_data: serde_json::Value,
    pub source_geometry: Option<SourceGeometry>,
    /// bbox 粗查询之后、命名前完成的精确方案边界资格结论。
    pub boundary_disposition: BoundaryDisposition,
}

/// 来源几何相对已确认方案边界的互斥结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryDisposition {
    Inside,
    Crosses,
    Outside,
    Invalid,
}

impl AcquisitionBatch {
    fn report(&self, written: usize) -> CollectionReport {
        CollectionReport {
            plan_id: self.plan_id.clone(),
            source_tag: self.source_tag.clone(),
            total: self.total_source_object_count,
            boundary_inside: self.boundary_inside,
            boundary_crossing: self.boundary_crossing,
            boundary_outside: self.boundary_outside,
            invalid_geometry: self.invalid_geometry,
            written,
            category_counts: self.category_counts.clone(),
            fallback_count: self.fallback_count,
            diff: self.diff.clone(),
            naming_partial: self.naming_partial,
        }
    }
}

/// 采集流水线：持有 B13 归类引擎，对任意 [`DataSource`] 执行缝 3 走线
pub struct AcquisitionPipeline {
    engine: ClassifyEngine,
    stage_listener: Option<StageListener>,
}

impl AcquisitionPipeline {
    /// 用 B13 内嵌默认映射表创建流水线
    pub fn new() -> Result<Self> {
        Ok(Self {
            engine: ClassifyEngine::with_default_mapping()?,
            stage_listener: None,
        })
    }

    /// 用外部构建的归类引擎创建（映射表已由 B13 校验）
    pub fn with_engine(engine: ClassifyEngine) -> Self {
        Self {
            engine,
            stage_listener: None,
        }
    }

    /// 注册阶段上报监听器（T36：拉取数据 / 补名 / 写库）
    pub fn with_stage_listener(mut self, listener: Option<StageListener>) -> Self {
        self.stage_listener = listener;
        self
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
        deadline: Instant,
    ) -> Result<CollectionReport> {
        let batch = self.acquire_batch(db, plan_id, boundary, source, deadline)?;
        self.emit_stage(CollectionStage::Writing);
        let written = db.write_raw_observations(&batch.raw_observations)?;
        Ok(batch.report(written))
    }

    /// 采集、分类与增量比对，但不发布任何候选投影或调用其它功能模块。
    pub fn acquire_batch(
        &self,
        db: &mut Database,
        plan_id: &PlanId,
        boundary: &Boundary,
        source: &dyn DataSource,
        deadline: Instant,
    ) -> Result<AcquisitionBatch> {
        let _ = deadline;
        if boundary.is_empty() {
            return Err(AcquisitionError::EmptyBoundary);
        }
        let confirmed_boundary =
            parse_boundary(boundary).ok_or(AcquisitionError::InvalidBoundary)?;
        self.emit_stage(CollectionStage::FetchingData);
        let entities = source.fetch_raw_entities(boundary)?;
        let dispositions = entities
            .iter()
            .map(|entity| {
                boundary_disposition(entity.source_geometry.as_ref(), &confirmed_boundary)
            })
            .collect::<Vec<_>>();
        let plan_key = plan_id.to_string();
        let mut raw_observations = Vec::new();
        let mut candidate_drafts = Vec::new();
        let mut diff_entries = Vec::new();
        let mut category_counts = BTreeMap::new();
        let mut fallback_count = 0;
        for (entity, disposition) in entities.iter().zip(&dispositions) {
            let source_entity = entity;
            let classification = self.engine.classify(&entity.tags);
            if *disposition == BoundaryDisposition::Inside {
                fallback_count += usize::from(classification.is_fallback);
                *category_counts.entry(classification.category).or_insert(0) += 1;
            }
            // RawObservation 只封存来源返回的证据。补名与当前方案边界资格都是
            // 派生事实，分别留在 CandidateDraft.name 和 typed disposition 中。
            let source_data = source_entity.to_source_data();
            let kind = match db.find_raw_observation(
                &plan_key,
                classification.category,
                &entity.entity_id,
            )? {
                None => DiffKind::Added,
                Some(existing)
                    if existing.digest == RawObservation::compute_digest(&source_data) =>
                {
                    DiffKind::Unchanged
                }
                Some(_) => DiffKind::Updated,
            };
            diff_entries.push(DiffEntry {
                category: classification.category,
                entity_id: entity.entity_id.clone(),
                kind,
            });
            let observation = RawObservation::new(
                &plan_key,
                classification.category,
                &entity.entity_id,
                source_data.clone(),
                source.source_tag(),
            );
            candidate_drafts.push(CandidateDraft {
                raw_observation_id: observation.id.clone(),
                data_source_tag: source.source_tag().to_owned(),
                source_entity_id: entity.entity_id.clone(),
                geometry_part_id: entity.geometry_part_id.clone(),
                category: classification.category,
                name: entity.name.clone(),
                name_source: if entity.name == entity.entity_id {
                    CandidateNameSource::Unnamed
                } else {
                    CandidateNameSource::Osm
                },
                source_data,
                source_geometry: source_entity.source_geometry.clone(),
                boundary_disposition: *disposition,
            });
            raw_observations.push(observation);
        }
        let geometry_object_count = candidate_drafts
            .iter()
            .filter(|draft| draft.source_geometry.is_some())
            .count();
        Ok(AcquisitionBatch {
            plan_id: plan_key,
            source_tag: source.source_tag().to_owned(),
            total_source_object_count: entities.len(),
            boundary_inside: dispositions
                .iter()
                .filter(|item| **item == BoundaryDisposition::Inside)
                .count(),
            boundary_crossing: dispositions
                .iter()
                .filter(|item| **item == BoundaryDisposition::Crosses)
                .count(),
            boundary_outside: dispositions
                .iter()
                .filter(|item| **item == BoundaryDisposition::Outside)
                .count(),
            invalid_geometry: dispositions
                .iter()
                .filter(|item| **item == BoundaryDisposition::Invalid)
                .count(),
            missing_geometry_object_count: entities.len() - geometry_object_count,
            geometry_object_count,
            naming_partial: false,
            raw_observations,
            candidate_drafts,
            category_counts,
            fallback_count,
            diff: RefreshDiff::new(diff_entries),
        })
    }

    fn emit_stage(&self, stage: CollectionStage) {
        if let Some(listener) = &self.stage_listener {
            listener(stage);
        }
    }
}

/// 来源几何相对已确认方案边界的互斥结论（D 工单复用：边界变化后本地重算）。
pub fn boundary_disposition(
    geometry: Option<&SourceGeometry>,
    boundary: &MultiPolygon<f64>,
) -> BoundaryDisposition {
    match geometry {
        Some(SourceGeometry::Point((lon, lat))) if finite_coordinate(*lon, *lat) => {
            let point = Point::new(*lon, *lat);
            if boundary.contains(&point) || boundary.intersects(&point) {
                BoundaryDisposition::Inside
            } else {
                BoundaryDisposition::Outside
            }
        }
        Some(SourceGeometry::LineString(points))
            if points.len() >= 2
                && points
                    .iter()
                    .all(|(lon, lat)| finite_coordinate(*lon, *lat)) =>
        {
            let line = LineString::from(points.clone());
            if boundary.contains(&line) {
                BoundaryDisposition::Inside
            } else if boundary.intersects(&line) {
                BoundaryDisposition::Crosses
            } else {
                BoundaryDisposition::Outside
            }
        }
        Some(SourceGeometry::Polygon(points))
            if points.len() >= 4
                && points.first() == points.last()
                && points
                    .iter()
                    .all(|(lon, lat)| finite_coordinate(*lon, *lat)) =>
        {
            let polygon = Polygon::new(LineString::from(points.clone()), Vec::new());
            if points
                .windows(2)
                .all(|pair| boundary.contains(&Line::new(pair[0], pair[1])))
            {
                BoundaryDisposition::Inside
            } else if boundary.intersects(&polygon) {
                BoundaryDisposition::Crosses
            } else {
                BoundaryDisposition::Outside
            }
        }
        _ => BoundaryDisposition::Invalid,
    }
}

/// 解析已确认边界为 geo MultiPolygon；无效/无法解析返回 `None`。
pub fn parse_boundary(boundary: &Boundary) -> Option<MultiPolygon<f64>> {
    let polygons = match boundary.r#type.as_str() {
        "Polygon" => vec![parse_polygon(&boundary.coordinates)?],
        "MultiPolygon" => boundary
            .coordinates
            .as_array()?
            .iter()
            .map(parse_polygon)
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    (!polygons.is_empty()).then(|| MultiPolygon::new(polygons))
}

fn parse_polygon(value: &serde_json::Value) -> Option<Polygon<f64>> {
    let mut rings = value.as_array()?.iter();
    let exterior = parse_ring(rings.next()?)?;
    let interiors = rings.map(parse_ring).collect::<Option<Vec<_>>>()?;
    Some(Polygon::new(exterior, interiors))
}

fn parse_ring(value: &serde_json::Value) -> Option<LineString<f64>> {
    let mut points = value
        .as_array()?
        .iter()
        .map(|point| {
            let pair = point.as_array()?;
            let lon = pair.first()?.as_f64()?;
            let lat = pair.get(1)?.as_f64()?;
            finite_coordinate(lon, lat).then_some((lon, lat))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut distinct = Vec::new();
    for point in &points {
        if !distinct.contains(point) {
            distinct.push(*point);
        }
    }
    if distinct.len() < 3 {
        return None;
    }
    if points.first() != points.last() {
        points.push(*points.first()?);
    }
    Some(LineString::from(points))
}

fn finite_coordinate(lon: f64, lat: f64) -> bool {
    lon.is_finite()
        && lat.is_finite()
        && (-180.0..=180.0).contains(&lon)
        && (-90.0..=90.0).contains(&lat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::RawEntity;
    use data_transformers::TagMap;
    use std::time::Duration;

    fn run_deadline() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

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
        RawEntity::with_geometry(
            id,
            format!("对象{id}"),
            tags,
            serde_json::json!({"id": id}),
            Some(SourceGeometry::Point((121.405, 31.205))),
            "point",
        )
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
            .collect(
                &mut db,
                &PlanId::generate(),
                &Boundary::empty(),
                &source,
                run_deadline(),
            )
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
            .collect(
                &mut db,
                &PlanId::generate(),
                &boundary(),
                &source,
                run_deadline(),
            )
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
