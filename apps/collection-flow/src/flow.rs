//! A1 collection-flow 深模块：完整候选采集用例入口。
//!
//! 外部接口只表达完整用户操作（开始采集、查看采集报告、取消/进度）；
//! 接口后协调 F4 → B2 → B14 → F7，返回已决定的页面状态、进度、导航
//! 结果和通知事实。S1 不读取采集中间数据，也不选择下一步调用对象。

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use coverage_audit::{AuditOutcome, QuietSentinel, ALL_CATEGORIES};
use data_acquisition::{
    AcquisitionBatch, AcquisitionPipeline, BoundaryDisposition, CandidateDraft,
    CollectionProgressView, CollectionReport, CollectionStage, DataSource, RawEntity,
    SourceGeometry,
};
use data_persistence::{
    boundary_fingerprint, BoundaryRevalidationApi, CandidateBatchSummary, CandidateDisplay,
    CandidateEligibility, CandidateProjection, CandidateProjectionsApi, CandidateShape,
    CandidateValidation, Database, RawObservationsApi,
};
use geometry_validator::{
    CandidateGeometry, GeometryShape, GeometryValidation, GeometryValidator, ValidationDisposition,
};
use localization::Localization;
use notification_center::Notification;
use shared_domain_types::{CandidateCategory, PlanId};

use crate::error::{CollectionError, Result};
use crate::input::CollectionInputStore;
use crate::operation::CollectionOperation;
use crate::revalidate::{run_boundary_revalidation, BoundaryRevalidationReport};
use crate::view::{
    CollectionFailure, CollectionFailureView, CollectionOutcome, CollectionPageView,
    CollectionReportView, CollectionStatus, CollectionSummary, PlanCollectionState,
    PlanCollectionStateKind,
};

/// 单次采集运行的时间预算（T36：总体截止 + 本地收尾余量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectionRunLimits {
    /// 单次采集运行总体截止（默认 60s；超限立即结束并如实标注部分未命名）
    pub overall_deadline: Duration,
    /// 为本地写库/发布预留的尾部余量（默认 10s；补名必须在其前停止派发）
    pub local_tail_margin: Duration,
}

impl Default for CollectionRunLimits {
    fn default() -> Self {
        Self {
            overall_deadline: Duration::from_secs(60),
            local_tail_margin: Duration::from_secs(10),
        }
    }
}

/// A1 采集流程：候选采集完整用例入口（深模块，非 S1）。
#[derive(Clone)]
pub struct CollectionFlow {
    db: Arc<Mutex<Database>>,
    source: Arc<dyn DataSource + Send + Sync>,
    validator: GeometryValidator,
    sentinel: QuietSentinel,
    l10n: Arc<Localization>,
    input: CollectionInputStore,
    states: Arc<Mutex<HashMap<String, PlanCollectionState>>>,
    lifecycle: Arc<AtomicU64>,
    active: Arc<AtomicBool>,
    limits: CollectionRunLimits,
}

impl CollectionFlow {
    /// 构造完整采集用例入口。
    ///
    /// `db` 与壳内 F 模块共用同一 B2 连接（原始观测落库与候选投影发布
    /// 只经 B2 公开 trait）；`source` 是 F4 可插拔数据源（生产为壳注入的
    /// 适配器，测试为罐头桩）；`l10n` 用于把结构化结果转成 B6 文本键。
    pub fn new(
        db: Arc<Mutex<Database>>,
        source: Arc<dyn DataSource + Send + Sync>,
        l10n: Arc<Localization>,
    ) -> Self {
        Self::new_with_limits(db, source, l10n, CollectionRunLimits::default())
    }

    /// 构造采集入口并显式指定运行时间预算（生产用默认 60s；验收测试可收紧）。
    pub fn new_with_limits(
        db: Arc<Mutex<Database>>,
        source: Arc<dyn DataSource + Send + Sync>,
        l10n: Arc<Localization>,
        limits: CollectionRunLimits,
    ) -> Self {
        Self {
            db,
            source,
            validator: GeometryValidator::new(),
            sentinel: QuietSentinel::new(),
            l10n,
            input: CollectionInputStore::default(),
            states: Arc::new(Mutex::new(HashMap::new())),
            lifecycle: Arc::new(AtomicU64::new(0)),
            active: Arc::new(AtomicBool::new(false)),
            limits,
        }
    }

    /// 打开方案：过期上一次采集结果并恢复该方案输入快照。
    pub fn set_plan(&self, plan_id: &PlanId) {
        self.expire_fetching();
        self.input.set_plan(plan_id);
    }

    /// 确认边界：记录该方案已确认的方案边界（采集输入之一）。
    pub fn confirm_boundary(&self, boundary: shared_domain_types::Boundary) {
        self.input.confirm_boundary(boundary);
    }

    /// 重置边界：用户重置圈画后采集输入同步失效。
    pub fn reset_boundary(&self) {
        self.input.reset_boundary();
    }

    /// 边界确认后触发本地资格重验证（D 工单）：确认边界与"上次采集时
    /// 使用的边界"指纹不同时，用已存原始观测几何本地重算候选资格并
    /// 单事务落库；相同（或无候选）时不触发任何计算。全程不联网。
    pub fn revalidate_boundary_if_changed(
        &self,
        plan_id: &PlanId,
        boundary: &shared_domain_types::Boundary,
    ) -> Result<BoundaryRevalidationReport> {
        let mut db = self.db.lock().expect("collection database lock");
        run_boundary_revalidation(&mut db, &self.validator, plan_id, boundary)
    }

    /// 提交一次完整"开始采集"意图；输入在 Start 返回前冻结。
    pub fn start(&self) -> Result<CollectionOperation> {
        let request = self
            .input
            .frozen_input()
            .ok_or(CollectionError::MissingInput)?;
        if self.active.swap(true, Ordering::SeqCst) {
            return Err(CollectionError::Busy);
        }
        let generation = self.lifecycle.load(Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        let worker = CollectionWorker {
            db: Arc::clone(&self.db),
            source: Arc::clone(&self.source),
            validator: self.validator,
            sentinel: self.sentinel.clone(),
            l10n: Arc::clone(&self.l10n),
            request,
            states: Arc::clone(&self.states),
            lifecycle: Arc::clone(&self.lifecycle),
            generation,
            limits: self.limits,
        };
        let active = Arc::clone(&self.active);
        let spawn_result = std::thread::Builder::new().spawn(move || {
            let outcome = worker.run();
            let _ = sender.send(outcome);
            active.store(false, Ordering::SeqCst);
        });
        if spawn_result.is_err() {
            self.active.store(false, Ordering::SeqCst);
            return Err(CollectionError::BackgroundTask);
        }
        self.mark_fetching();
        Ok(CollectionOperation::new(
            receiver,
            Arc::clone(&self.lifecycle),
            generation,
        ))
    }

    /// 取消当前采集：旧结果过期，后续轮询不得拉回成功。
    pub fn cancel(&self) {
        self.expire_fetching();
    }

    /// 离开当前页面/方案：与切换方案同语义的过期。
    pub fn leave(&self) {
        self.expire_fetching();
    }

    /// 当前方案已决定的采集页面状态（S1 只绘制）。
    pub fn page_view(&self) -> CollectionPageView {
        let Some(plan_id) = self.input.active_plan_id() else {
            return self.pending_page();
        };
        let states = self.states.lock().expect("collection states lock");
        match states.get(&plan_id).map(|state| &state.state) {
            Some(PlanCollectionStateKind::Fetching {
                stage,
                started_at_millis,
            }) => self.fetching_page(*stage, *started_at_millis),
            Some(PlanCollectionStateKind::Outcome(outcome)) => match outcome.as_ref() {
                CollectionOutcome::Succeeded(summary) => summary.page.clone(),
                CollectionOutcome::Failed(failure) => failure.page.clone(),
            },
            None => self.pending_page(),
        }
    }

    /// 完整"查看采集报告"操作：返回最近一次已完成采集的报告视图。
    pub fn report_view(&self) -> Option<CollectionReportView> {
        let plan_id = self.input.active_plan_id()?;
        self.states
            .lock()
            .expect("collection states lock")
            .get(&plan_id)
            .and_then(PlanCollectionState::report)
    }

    /// 评审入口是否解锁（原始观测已保存 + 候选投影完整发布 + 报告完成）。
    pub fn is_review_unlocked(&self, plan_id: &str) -> bool {
        self.states
            .lock()
            .expect("collection states lock")
            .get(plan_id)
            .is_some_and(PlanCollectionState::review_unlocked)
    }

    /// 把结构化错误汇总为页面状态 + B7 通知事实（A1 决定影响范围，不吞错）。
    ///
    /// Start 同步错误（无后台操作）与后台失败共用同一汇总，S1 只绘制并
    /// 把通知事实交给 B7。
    pub fn failure_view(&self, error: &CollectionError) -> CollectionFailure {
        failure_view(&self.l10n, error)
    }

    fn mark_fetching(&self) {
        let Some(plan_id) = self.input.active_plan_id() else {
            return;
        };
        self.states.lock().expect("collection states lock").insert(
            plan_id,
            PlanCollectionState {
                state: PlanCollectionStateKind::Fetching {
                    stage: CollectionStage::FetchingData,
                    started_at_millis: Self::now_millis(),
                },
            },
        );
    }

    /// 过期当前活动方案的进行中标记（取消/切换/离开后呈现回待定）。
    fn expire_fetching(&self) {
        self.lifecycle.fetch_add(1, Ordering::SeqCst);
        let Some(plan_id) = self.input.active_plan_id() else {
            return;
        };
        self.states
            .lock()
            .expect("collection states lock")
            .remove(&plan_id);
    }

    fn pending_page(&self) -> CollectionPageView {
        CollectionPageView {
            status: CollectionStatus::Pending,
            progress: CollectionProgressView::fetching(),
            diff_summary: None,
            report: None,
            review_unlocked: false,
            failure: None,
        }
    }

    fn fetching_page(&self, stage: CollectionStage, started_at_millis: u64) -> CollectionPageView {
        let elapsed_secs = Self::now_millis().saturating_sub(started_at_millis) / 1000;
        CollectionPageView {
            status: CollectionStatus::Fetching,
            progress: CollectionProgressView::fetching_at(stage, elapsed_secs),
            diff_summary: None,
            report: None,
            review_unlocked: false,
            failure: None,
        }
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// 后台采集 worker：完整执行 F4 → B2 → B14 → F7，只持有 Start 冻结的输入。
struct CollectionWorker {
    db: Arc<Mutex<Database>>,
    source: Arc<dyn DataSource + Send + Sync>,
    validator: GeometryValidator,
    sentinel: QuietSentinel,
    l10n: Arc<Localization>,
    request: crate::input::CollectionInput,
    states: Arc<Mutex<HashMap<String, PlanCollectionState>>>,
    lifecycle: Arc<AtomicU64>,
    generation: u64,
    limits: CollectionRunLimits,
}

/// 一次命名阶段的汇总事实（缺 Key 未执行 + 跳过数量）。
struct NamingSummary {
    key_missing: bool,
    skipped_count: usize,
}

impl CollectionWorker {
    fn run(self) -> CollectionOutcome {
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| self.run_inner()))
            .unwrap_or_else(|_| Err(CollectionError::BackgroundTask));
        let expired = self.lifecycle.load(Ordering::SeqCst) != self.generation;
        let plan_id = self.request.plan_id.to_string();
        match (outcome, expired) {
            (Ok(summary), false) => {
                self.states.lock().expect("collection states lock").insert(
                    plan_id,
                    PlanCollectionState {
                        state: PlanCollectionStateKind::Outcome(Box::new(
                            CollectionOutcome::Succeeded(summary.clone()),
                        )),
                    },
                );
                CollectionOutcome::Succeeded(summary)
            }
            (Ok(_), true) => CollectionOutcome::Failed(CollectionFailure {
                page: CollectionPageView {
                    status: CollectionStatus::Pending,
                    progress: CollectionProgressView::fetching(),
                    diff_summary: None,
                    report: None,
                    review_unlocked: false,
                    failure: None,
                },
                notification: None,
                diagnostic: CollectionError::Expired.to_string(),
            }),
            (Err(error), false) => {
                let failure = self.failure(&error);
                self.states.lock().expect("collection states lock").insert(
                    plan_id,
                    PlanCollectionState {
                        state: PlanCollectionStateKind::Outcome(Box::new(
                            CollectionOutcome::Failed(failure.clone()),
                        )),
                    },
                );
                CollectionOutcome::Failed(failure)
            }
            (Err(_), true) => CollectionOutcome::Failed(CollectionFailure {
                page: CollectionPageView {
                    status: CollectionStatus::Pending,
                    progress: CollectionProgressView::fetching(),
                    diff_summary: None,
                    report: None,
                    review_unlocked: false,
                    failure: None,
                },
                notification: None,
                diagnostic: CollectionError::Expired.to_string(),
            }),
        }
    }

    /// 采集错误 → 页面状态 + B7 通知事实（A1 汇总影响，不吞错）。
    fn failure(&self, error: &CollectionError) -> CollectionFailure {
        failure_view(&self.l10n, error)
    }

    /// 只在 B14 判定为 Reviewable 之后，对“无名建筑面”调用数据源补名。
    ///
    /// 资格门为：建筑类别、完全位于边界内、通过几何验证（Retained/Repaired）、
    /// 无可用 OSM 名称、面几何。
    ///
    /// Point/LineString、边界外/相交/隔离对象不会进入这里，因此不会发出高德请求。
    fn enrich_reviewable_building_names(
        &self,
        batch: &mut AcquisitionBatch,
        validation: &GeometryValidation,
        deadline: Instant,
    ) -> Result<NamingSummary> {
        let mut targets: Vec<(usize, RawEntity)> = Vec::new();
        for (index, draft) in batch.candidate_drafts.iter().enumerate() {
            if draft.boundary_disposition != BoundaryDisposition::Inside {
                continue;
            }
            if draft.category != CandidateCategory::Building {
                continue;
            }
            if draft.name != draft.source_entity_id {
                continue;
            }
            let Some(SourceGeometry::Polygon(_)) = draft.source_geometry.as_ref() else {
                continue;
            };
            let reviewable = validation.outcomes.iter().any(|outcome| {
                outcome.candidate_id == draft.raw_observation_id
                    && matches!(
                        outcome.disposition,
                        ValidationDisposition::Retained | ValidationDisposition::Repaired
                    )
            });
            if !reviewable {
                continue;
            }
            targets.push((
                index,
                RawEntity::for_naming(
                    draft.source_entity_id.clone(),
                    draft.source_geometry.clone(),
                    draft.geometry_part_id.clone(),
                ),
            ));
        }

        if targets.is_empty() {
            batch.naming_partial = false;
            return Ok(NamingSummary {
                key_missing: false,
                skipped_count: 0,
            });
        }

        let entities = targets.iter().map(|(_, entity)| entity.clone()).collect();
        let enriched = self
            .source
            .enrich(entities, deadline)
            .map_err(CollectionError::Acquisition)?;
        for (position, (draft_index, _)) in targets.iter().enumerate() {
            if let Some(entity) = enriched.entities.get(position) {
                batch.candidate_drafts[*draft_index].name = entity.name.clone();
            }
            if let Some(source) = enriched.name_sources.get(position) {
                batch.candidate_drafts[*draft_index].name_source = *source;
            }
        }
        batch.naming_partial = enriched.partial;
        Ok(NamingSummary {
            key_missing: enriched.key_missing,
            skipped_count: enriched.skipped_count,
        })
    }

    /// 完整采集链：F4 采集批次 → B2 原始观测落库 → B14 点线面验证 →
    /// B2 候选投影原子发布 → F7 覆盖体检 → 组装报告并解锁评审。
    fn run_inner(&self) -> Result<CollectionSummary> {
        // T36：总体截止与阶段上报（拉取数据 / 补名 / 写库）。
        let overall_deadline = Instant::now() + self.limits.overall_deadline;
        let enrich_deadline = overall_deadline
            .checked_sub(self.limits.local_tail_margin)
            .unwrap_or(overall_deadline);
        let states = Arc::clone(&self.states);
        let plan_id = self.request.plan_id.to_string();
        let notify_stage: Arc<dyn Fn(CollectionStage) + Send + Sync> =
            Arc::new(move |stage: CollectionStage| {
                if let Ok(mut states) = states.lock() {
                    if let Some(state) = states.get_mut(&plan_id) {
                        if let PlanCollectionStateKind::Fetching { stage: current, .. } =
                            &mut state.state
                        {
                            *current = stage;
                        }
                    }
                }
            });
        let pipeline_notify = Arc::clone(&notify_stage);
        let pipeline = AcquisitionPipeline::new()
            .map_err(CollectionError::Acquisition)?
            .with_stage_listener(Some(Box::new(move |stage: CollectionStage| {
                pipeline_notify(stage)
            })));
        let mut db = self.db.lock().expect("collection database lock");

        // 1. F4：采集批次（拉取 + 归类 + 增量比对，不发布投影）。
        let mut batch = pipeline
            .acquire_batch(
                &mut db,
                &self.request.plan_id,
                &self.request.boundary,
                self.source.as_ref(),
                enrich_deadline,
            )
            .map_err(CollectionError::Acquisition)?;

        // 2. B2：原始观测落库（数据粮仓，只写不删）。
        notify_stage(CollectionStage::Writing);
        let written = db
            .write_raw_observations(&batch.raw_observations)
            .map_err(CollectionError::Persistence)?;

        // 3. B14：点/线/面逐对象验证（隔离不阻断同批其它对象）。
        let validation = self.validator.validate_batch(
            batch
                .candidate_drafts
                .iter()
                .filter_map(candidate_geometry)
                .collect(),
        );

        // 3.5 命名资格门后移：只有最终 Reviewable 的无名建筑面才调用补名。
        notify_stage(CollectionStage::Naming);
        let naming =
            self.enrich_reviewable_building_names(&mut batch, &validation, enrich_deadline)?;

        // 4. B2：候选投影批次原子发布（构建 → 写入 → 继承缺失 → 发布）。
        let candidate_batch = db
            .prepare_candidate_batch(&batch.plan_id)
            .map_err(CollectionError::Persistence)?;
        let projections = build_projections(&batch, &validation);
        db.write_candidate_projections(&candidate_batch.id, &projections)
            .map_err(CollectionError::Persistence)?;
        db.carry_forward_missing_candidate_projections(&candidate_batch.id)
            .map_err(CollectionError::Persistence)?;
        db.publish_candidate_batch(&candidate_batch.id)
            .map_err(CollectionError::Persistence)?;
        // D 工单：把本次采集使用的边界指纹绑定到方案（重验证触发依据）。
        db.save_plan_collection_boundary(
            &self.request.plan_id.to_string(),
            &boundary_fingerprint(&self.request.boundary),
        )
        .map_err(CollectionError::Persistence)?;
        let batch_summary = db
            .candidate_batch_summary(&candidate_batch.id)
            .map_err(CollectionError::Persistence)?;

        // 5. F7：覆盖体检（安静哨兵；事实变体返回合并疑点窗，弹窗事实由
        //    A1 汇总后交给 S1/B7 在 UI 线程呈现）。
        let counts = category_counts(&batch);
        let database: &mut Database = &mut db;
        let audit = self
            .sentinel
            .after_collection_facts(
                database,
                &self.request.plan_id,
                &counts,
                Vec::new(),
                self.l10n.as_ref(),
            )
            .map_err(CollectionError::Audit)?;

        // 6. 组装页面/报告/评审解锁（只有全部完成后才返回成功）。
        let report = CollectionReport {
            plan_id: batch.plan_id.clone(),
            source_tag: batch.source_tag.clone(),
            total: batch.total_source_object_count,
            boundary_inside: batch.boundary_inside,
            boundary_crossing: batch.boundary_crossing,
            boundary_outside: batch.boundary_outside,
            invalid_geometry: batch.invalid_geometry,
            written,
            category_counts: batch.category_counts.clone(),
            fallback_count: batch.fallback_count,
            diff: batch.diff.clone(),
            naming_partial: batch.naming_partial,
        };
        let progress = CollectionProgressView::completed(&report);
        let diff_summary = self.l10n.t_with_args(
            batch.diff.summary_key(),
            serde_json::json!({
                "added": batch.diff.added_count(),
                "updated": batch.diff.updated_count(),
                "unchanged": batch.diff.unchanged_count(),
            }),
        );
        let report_view = self.build_report_view(&audit, &batch, &batch_summary, &naming);
        let notification = audit.popup.as_ref().map(|popup| {
            Notification::error(
                self.l10n.t("audit.source_tag"),
                popup.title.clone(),
                popup.body.clone(),
            )
        });
        Ok(CollectionSummary {
            page: CollectionPageView {
                status: CollectionStatus::Completed,
                progress,
                diff_summary: Some(diff_summary),
                report: Some(report_view),
                review_unlocked: true,
                failure: None,
            },
            notification,
        })
    }

    fn build_report_view(
        &self,
        audit: &AuditOutcome,
        batch: &AcquisitionBatch,
        batch_summary: &CandidateBatchSummary,
        naming: &NamingSummary,
    ) -> CollectionReportView {
        let audit_view = self.sentinel.report_view(&audit.result, self.l10n.as_ref());
        let mut candidate_lines = vec![
            self.l10n.t_with_args(
                "collection.report_source_total",
                serde_json::json!({ "count": batch.total_source_object_count }),
            ),
            self.l10n.t_with_args(
                "collection.report_boundary_inside",
                serde_json::json!({ "count": batch.boundary_inside }),
            ),
            self.l10n.t_with_args(
                "collection.report_boundary_crossing",
                serde_json::json!({ "count": batch.boundary_crossing }),
            ),
            self.l10n.t_with_args(
                "collection.report_boundary_outside",
                serde_json::json!({ "count": batch.boundary_outside }),
            ),
            self.l10n.t_with_args(
                "collection.report_invalid_geometry",
                serde_json::json!({ "count": batch.invalid_geometry }),
            ),
            self.l10n.t_with_args(
                "collection.report_reviewable_final",
                serde_json::json!({ "count": batch_summary.reviewable_count }),
            ),
            self.l10n.t_with_args(
                "collection.report_isolated",
                serde_json::json!({ "count": batch_summary.isolated_count }),
            ),
            self.l10n.t_with_args(
                "collection.report_repaired",
                serde_json::json!({ "count": batch_summary.repaired_count }),
            ),
        ];
        if naming.key_missing {
            candidate_lines.push(self.l10n.t_with_args(
                "collection.report_naming_skipped",
                serde_json::json!({ "count": naming.skipped_count }),
            ));
        }
        CollectionReportView {
            title: audit_view.title,
            entry_label: audit_view.entry_label,
            category_lines: audit_view.category_lines,
            candidate_lines,
            issue_lines: audit_view.issue_lines,
            no_issues_line: audit_view.no_issues_line,
        }
    }
}

/// 采集错误 → 页面状态 + B7 通知事实（A1 汇总影响，不吞错）。
fn failure_view(l10n: &Localization, error: &CollectionError) -> CollectionFailure {
    let message = l10n.t(error.user_message_key());
    let notification = match error {
        CollectionError::Expired => None,
        _ => Some(Notification::error(
            l10n.t("app.source_tag"),
            l10n.t("dialog.error_title"),
            message.clone(),
        )),
    };
    CollectionFailure {
        page: CollectionPageView {
            status: CollectionStatus::Failed,
            progress: CollectionProgressView::fetching(),
            diff_summary: None,
            report: None,
            review_unlocked: false,
            failure: Some(CollectionFailureView {
                message: message.clone(),
                diagnostic: error.to_string(),
            }),
        },
        notification,
        diagnostic: error.to_string(),
    }
}

/// 候选草稿 → B14 待验证几何（无来源几何的对象不伪造形状，交隔离）。
fn candidate_geometry(draft: &CandidateDraft) -> Option<CandidateGeometry> {
    if draft.boundary_disposition != BoundaryDisposition::Inside {
        return None;
    }
    let geometry = draft.source_geometry.as_ref()?;
    match geometry {
        SourceGeometry::Point(point) => Some(CandidateGeometry::with_shape(
            draft.raw_observation_id.clone(),
            GeometryShape::Point(*point),
        )),
        SourceGeometry::LineString(points) => Some(CandidateGeometry::with_shape(
            draft.raw_observation_id.clone(),
            GeometryShape::LineString(points.clone()),
        )),
        SourceGeometry::Polygon(points) => {
            let mut candidate = CandidateGeometry::with_shape(
                draft.raw_observation_id.clone(),
                GeometryShape::Polygon,
            );
            candidate.coordinates = points.clone();
            Some(candidate)
        }
        _ => None,
    }
}

/// 把 B14 验证结果转成 B2 候选投影（Reviewable/Isolated 资格，ADR-0040）。
fn build_projections(
    batch: &AcquisitionBatch,
    validation: &GeometryValidation,
) -> Vec<CandidateProjection> {
    batch
        .candidate_drafts
        .iter()
        .map(|draft| projection_for(draft, batch, validation))
        .collect()
}

fn projection_for(
    draft: &CandidateDraft,
    batch: &AcquisitionBatch,
    validation: &GeometryValidation,
) -> CandidateProjection {
    let outcome = validation
        .outcomes
        .iter()
        .find(|item| item.candidate_id == draft.raw_observation_id);
    let display = CandidateDisplay::new(draft.name.clone(), display_tags(&draft.source_data));
    let common = |shape: CandidateShape,
                  validation_flag: CandidateValidation,
                  eligibility: CandidateEligibility| {
        CandidateProjection::new(
            uuid::Uuid::new_v4().to_string(),
            batch.plan_id.clone(),
            draft.raw_observation_id.clone(),
            draft.data_source_tag.clone(),
            draft.source_entity_id.clone(),
            draft.geometry_part_id.clone(),
            draft.category,
            display.clone(),
            shape,
            validation_flag,
            eligibility,
        )
        .with_name_source(draft.name_source)
    };
    match draft.boundary_disposition {
        BoundaryDisposition::Outside => {
            return common(
                shape_from_source(draft.source_geometry.as_ref()),
                CandidateValidation::Rejected,
                CandidateEligibility::Isolated,
            )
            .isolated_reason("outside_confirmed_plan_boundary");
        }
        BoundaryDisposition::Crosses => {
            return common(
                shape_from_source(draft.source_geometry.as_ref()),
                CandidateValidation::Rejected,
                CandidateEligibility::Isolated,
            )
            .isolated_reason("crosses_confirmed_plan_boundary");
        }
        BoundaryDisposition::Invalid => {
            return common(
                no_shape(),
                CandidateValidation::Rejected,
                CandidateEligibility::Isolated,
            )
            .isolated_reason("invalid_source_geometry");
        }
        BoundaryDisposition::Inside => {}
    }
    match outcome.map(|item| &item.disposition) {
        Some(ValidationDisposition::Retained) | Some(ValidationDisposition::Repaired) => {
            let geometry = outcome
                .and_then(|item| item.geometry.as_ref())
                .expect("B14 保留/修复必有规范化几何");
            let validation_flag = if matches!(
                outcome.map(|item| &item.disposition),
                Some(ValidationDisposition::Repaired)
            ) {
                CandidateValidation::Repaired
            } else {
                CandidateValidation::Retained
            };
            common(
                shape_from_candidate(geometry),
                validation_flag,
                CandidateEligibility::Reviewable,
            )
        }
        Some(ValidationDisposition::Rejected(reason)) => common(
            shape_from_source(draft.source_geometry.as_ref()),
            CandidateValidation::Rejected,
            CandidateEligibility::Isolated,
        )
        .isolated_reason(reason.to_string()),
        None | Some(_) => common(
            no_shape(),
            CandidateValidation::Rejected,
            CandidateEligibility::Isolated,
        )
        .isolated_reason("missing_source_geometry"),
    }
}

/// 展示标签：来源 source_data 中的 tags 原样列出（ADR-0040 展示属性）。
fn display_tags(source_data: &serde_json::Value) -> Vec<(String, String)> {
    source_data
        .get("tags")
        .and_then(serde_json::Value::as_object)
        .map(|tags| {
            tags.iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// B14 规范化几何 → B2 候选形状。
fn shape_from_candidate(geometry: &CandidateGeometry) -> CandidateShape {
    match &geometry.shape {
        GeometryShape::Point(_) => CandidateShape::point(serde_json::json!(geometry.coordinates)),
        GeometryShape::LineString(_) => {
            CandidateShape::line_string(serde_json::json!(geometry.coordinates))
        }
        GeometryShape::Polygon => CandidateShape::polygon(serde_json::json!(geometry.coordinates)),
        _ => no_shape(),
    }
}

/// 来源几何原样 → B2 候选形状（隔离对象保留来源证据，不伪造）。
fn shape_from_source(geometry: Option<&SourceGeometry>) -> CandidateShape {
    match geometry {
        Some(SourceGeometry::Point(point)) => {
            CandidateShape::point(serde_json::json!([point.0, point.1]))
        }
        Some(SourceGeometry::LineString(points)) => {
            CandidateShape::line_string(serde_json::json!(points))
        }
        Some(SourceGeometry::Polygon(points)) => CandidateShape::polygon(serde_json::json!(points)),
        None | Some(_) => no_shape(),
    }
}

/// 无形状占位（数据源未提供几何时不得伪造候选形状，ADR-0040）。
///
/// B2 schema 的 geometry_kind 只接受 point/line_string/polygon；隔离投影
/// 以空坐标 point 占位并写明 `missing_source_geometry` 原因，评审只读
/// Reviewable，隔离占位不会被误用。
fn no_shape() -> CandidateShape {
    CandidateShape {
        kind: "point".to_owned(),
        coordinates: serde_json::json!([]),
    }
}

/// 各类别数量 → F7 六类别顺序计数。
fn category_counts(batch: &AcquisitionBatch) -> [u32; 6] {
    std::array::from_fn(|index| {
        let category = ALL_CATEGORIES[index];
        u32::try_from(batch.category_counts.get(&category).copied().unwrap_or(0))
            .unwrap_or(u32::MAX)
    })
}
