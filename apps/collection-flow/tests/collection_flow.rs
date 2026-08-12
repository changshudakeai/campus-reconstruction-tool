//! A1 候选采集完整用例的定向验收测试（ADR-0039/0040）。
//!
//! 测试通过 [`CollectionFlow`] 的外部接口观察行为：开始采集、查看采集报告、
//! 取消/进度；不越过接口检查 F4/B2/B14/F7 的内部调用顺序。数据源一律用
//! 测试桩（真实高德在线链路留发布前验证）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use collection_flow::{
    CollectionError, CollectionFlow, CollectionOperation, CollectionOutcome, CollectionRunLimits,
    CollectionStatus,
};
use data_acquisition::{
    overpass::{boundary_bbox, campus_objects_query, OverpassClient},
    DataSource, OverpassDataSource, RawEntity, RegeoNamer, SourceGeometry,
};
use data_persistence::{CandidateProjectionsApi, Database, RawObservationsApi};
use data_transformers::TagMap;
use export_flow::{BoundaryExportFlow, StdExportFileSystem};
use global_settings::SettingsManager;
use localization::{Language, Localization};
use project_management::PlanContextView;
use shared_domain_types::{Boundary, PlanId};

/// 罐头数据源：固定返回预置对象（离线测试替身，ADR-0013 可插拔缝）。
#[derive(Debug, Clone)]
struct FakeSource {
    entities: Vec<RawEntity>,
}

fn putuo_boundary_gcj02() -> Boundary {
    let convert = |lon: f64, lat: f64| {
        let (lon, lat) = gaode_client::wgs84_to_gcj02(lon, lat);
        serde_json::json!([lon, lat])
    };
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            convert(121.3990, 31.2270),
            convert(121.4050, 31.2270),
            convert(121.4050, 31.2330),
            convert(121.3990, 31.2330),
            convert(121.3990, 31.2270)
        ]]),
    }
}

#[test]
fn putuo_collection_persists_all_sources_but_only_inside_six_categories_are_reviewable() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let payload = include_str!(
        "../../../core/data-acquisition/tests/fixtures/putuo-boundary-eligibility.json"
    )
    .to_owned();
    let source: Arc<dyn DataSource + Send + Sync> =
        Arc::new(OverpassDataSource::new(Box::new(move |_| {
            Ok(payload.clone())
        })));
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    flow.set_plan(&plan_id);
    flow.confirm_boundary(putuo_boundary_gcj02());

    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5)).expect("采集终态");
    let CollectionOutcome::Succeeded(summary) = outcome else {
        panic!("单项隔离不得拖垮整批");
    };
    let database = db.lock().expect("database lock");
    assert_eq!(
        database
            .list_raw_observations(&plan_id.to_string())
            .expect("原始观测 API")
            .len(),
        9,
        "边界外、相交和无效 relation 仍保留来源证据"
    );
    let reviewable = database
        .list_reviewable_candidate_projections(&plan_id.to_string())
        .expect("Reviewable API");
    assert_eq!(reviewable.len(), 6, "只有完整位于边界内的六类对象可评审");
    assert_eq!(
        reviewable
            .iter()
            .map(|candidate| candidate.category)
            .collect::<std::collections::BTreeSet<_>>(),
        [
            shared_domain_types::CandidateCategory::Building,
            shared_domain_types::CandidateCategory::Road,
            shared_domain_types::CandidateCategory::Water,
            shared_domain_types::CandidateCategory::Vegetation,
            shared_domain_types::CandidateCategory::Sports,
            shared_domain_types::CandidateCategory::Other,
        ]
        .into_iter()
        .collect()
    );
    let report = summary.page.report.expect("采集报告");
    for expected in [
        "来源对象 9 项",
        "边界内 6 项",
        "与边界相交 1 项",
        "边界外 1 项",
        "无效几何 1 项",
        "最终可评审 6 项",
    ] {
        assert!(
            report.candidate_lines.iter().any(|line| line == expected),
            "报告缺少 {expected}: {:?}",
            report.candidate_lines
        );
    }
}

#[test]
fn expanded_bbox_with_twelve_thousand_outside_buildings_does_not_expand_reviewable_scope() {
    let mut payload: serde_json::Value = serde_json::from_str(include_str!(
        "../../../core/data-acquisition/tests/fixtures/putuo-boundary-eligibility.json"
    ))
    .expect("Putuo fixture JSON");
    let elements = payload["elements"].as_array_mut().expect("elements");
    for index in 0..12_050_u64 {
        let lon = 121.4100 + (index % 100) as f64 * 0.00001;
        let lat = 31.2400 + (index / 100) as f64 * 0.00001;
        elements.push(serde_json::json!({
            "type": "way",
            "id": 1_000_000 + index,
            "tags": {"building": "yes"},
            "geometry": [
                {"lon": lon, "lat": lat},
                {"lon": lon + 0.000005, "lat": lat},
                {"lon": lon + 0.000005, "lat": lat + 0.000005},
                {"lon": lon, "lat": lat}
            ]
        }));
    }
    let payload = payload.to_string();
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let source: Arc<dyn DataSource + Send + Sync> =
        Arc::new(OverpassDataSource::new(Box::new(move |_| {
            Ok(payload.clone())
        })));
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    flow.set_plan(&plan_id);
    flow.confirm_boundary(putuo_boundary_gcj02());

    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(30)).expect("大批次终态");
    let CollectionOutcome::Succeeded(summary) = outcome else {
        panic!("大量边界外对象应被隔离而不是拖垮采集");
    };
    let database = db.lock().expect("database lock");
    assert_eq!(
        database
            .list_raw_observations(&plan_id.to_string())
            .expect("原始观测")
            .len(),
        12_059
    );
    assert_eq!(
        database
            .list_reviewable_candidate_projections(&plan_id.to_string())
            .expect("Reviewable API")
            .len(),
        6,
        "bbox 传输窗口中的 12,000+ 边界外建筑不得形成 Reviewable"
    );
    assert!(summary
        .page
        .report
        .expect("报告")
        .candidate_lines
        .iter()
        .any(|line| line == "边界外 12051 项"));
}

impl DataSource for FakeSource {
    fn source_tag(&self) -> &str {
        "fake"
    }

    fn fetch_raw_entities(&self, _boundary: &Boundary) -> data_acquisition::Result<Vec<RawEntity>> {
        Ok(self.entities.clone())
    }
}

/// 阻塞数据源：fetch 一直等到测试放行（取消/过期隔离测试用）。
struct BlockingSource {
    entities: Mutex<Vec<RawEntity>>,
    release: (Mutex<bool>, Condvar),
}

impl BlockingSource {
    fn new() -> Self {
        Self {
            entities: Mutex::new(Vec::new()),
            release: (Mutex::new(false), Condvar::new()),
        }
    }

    fn release(&self) {
        *self.release.0.lock().expect("release lock") = true;
        self.release.1.notify_all();
    }
}

impl DataSource for BlockingSource {
    fn source_tag(&self) -> &str {
        "blocking"
    }

    fn fetch_raw_entities(&self, _boundary: &Boundary) -> data_acquisition::Result<Vec<RawEntity>> {
        let mut released = self.release.0.lock().expect("release lock");
        while !*released {
            released = self.release.1.wait(released).expect("release wait");
        }
        Ok(self.entities.lock().expect("entities lock").clone())
    }
}

/// 失败数据源：数据源不可达（结构化失败链路测试用）。
#[derive(Debug, Clone, Copy)]
struct FailingSource;

impl DataSource for FailingSource {
    fn source_tag(&self) -> &str {
        "failing"
    }

    fn fetch_raw_entities(&self, _boundary: &Boundary) -> data_acquisition::Result<Vec<RawEntity>> {
        Err(data_acquisition::AcquisitionError::SourceUnreachable {
            source_tag: "failing".to_owned(),
            message: "测试用网络不可达".to_owned(),
        })
    }
}

fn boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.4000, 39.9000],
            [116.4010, 39.9000],
            [116.4010, 39.9010],
            [116.4000, 39.9010],
            [116.4000, 39.9000]
        ]]),
    }
}

fn tags(pairs: &[(&str, &str)]) -> TagMap {
    let mut tags = TagMap::new();
    for (key, value) in pairs {
        tags.insert((*key).to_owned(), (*value).to_owned());
    }
    tags
}

fn building_point(id: &str, lon: f64, lat: f64) -> RawEntity {
    RawEntity::with_geometry(
        id,
        format!("教学楼{id}"),
        tags(&[("building", "school")]),
        serde_json::json!({"id": id}),
        Some(SourceGeometry::Point((lon, lat))),
        "point",
    )
}

fn road_line(id: &str) -> RawEntity {
    RawEntity::with_geometry(
        id,
        format!("道路{id}"),
        tags(&[("highway", "residential")]),
        serde_json::json!({"id": id}),
        Some(SourceGeometry::LineString(vec![
            (116.4001, 39.9001),
            (116.4008, 39.9002),
        ])),
        "line",
    )
}

fn water_polygon(id: &str) -> RawEntity {
    RawEntity::with_geometry(
        id,
        format!("水域{id}"),
        tags(&[("natural", "water")]),
        serde_json::json!({"id": id}),
        Some(SourceGeometry::Polygon(vec![
            (116.4002, 39.9003),
            (116.4005, 39.9003),
            (116.4005, 39.9006),
            (116.4002, 39.9003),
        ])),
        "polygon",
    )
}

fn flow(db: Arc<Mutex<Database>>, source: Arc<dyn DataSource + Send + Sync>) -> CollectionFlow {
    let l10n = Arc::new(Localization::new(Language::ZhCn).expect("zh-CN 资源"));
    CollectionFlow::new(db, source, l10n)
}

fn flow_with_limits(
    db: Arc<Mutex<Database>>,
    source: Arc<dyn DataSource + Send + Sync>,
    limits: CollectionRunLimits,
) -> CollectionFlow {
    let l10n = Arc::new(Localization::new(Language::ZhCn).expect("zh-CN 资源"));
    CollectionFlow::new_with_limits(db, source, l10n, limits)
}

fn seed_plan(flow: &CollectionFlow, plan_id: &PlanId) {
    flow.set_plan(plan_id);
    flow.confirm_boundary(boundary());
}

fn wait_for_terminal(
    operation: &mut CollectionOperation,
    deadline: Duration,
) -> Result<CollectionOutcome, CollectionError> {
    let deadline = Instant::now() + deadline;
    loop {
        if let Some(result) = operation.try_complete() {
            return result;
        }
        assert!(Instant::now() < deadline, "采集操作未在期限内达到终态");
        std::thread::yield_now();
    }
}

#[test]
fn start_success_persists_raw_and_publishes_candidates_and_unlocks_review() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let source = Arc::new(FakeSource {
        entities: vec![
            building_point("B01", 116.4003, 39.9003),
            road_line("R01"),
            water_polygon("W01"),
        ],
    });
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start 应立即可用");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5))
        .expect("后台采集不得报告后台任务失败");
    let CollectionOutcome::Succeeded(summary) = outcome else {
        panic!("合法对象采集必须成功");
    };

    // 原始观测已保存（数据粮仓，只写不删）。
    let raw = db
        .lock()
        .expect("db lock")
        .list_raw_observations(&plan_id.to_string())
        .expect("读取原始观测");
    assert_eq!(raw.len(), 3, "三条原始观测必须全部落库");

    // 候选投影完整发布：current_candidate_batches 只读已发布批次。
    let reviewable = db
        .lock()
        .expect("db lock")
        .list_reviewable_candidate_projections(&plan_id.to_string())
        .expect("读取可评审候选");
    assert_eq!(reviewable.len(), 3, "点/线/面全部通过验证进入候选池");
    assert!(
        reviewable.iter().all(|projection| projection.eligibility
            == data_persistence::CandidateEligibility::Reviewable),
        "全部候选应具有 Reviewable 资格"
    );

    // 报告完成 + 评审解锁。
    assert_eq!(summary.page.status, CollectionStatus::Completed);
    assert_eq!(summary.page.progress.percent, 100);
    assert_eq!(summary.page.progress.collected_total, 3);
    assert!(summary.page.report.is_some(), "完成采集后必须提供常驻报告");
    assert!(
        summary.page.review_unlocked,
        "原始+投影+报告齐全后必须解锁评审"
    );
    assert!(flow.is_review_unlocked(&plan_id.to_string()));
    assert!(flow.report_view().is_some(), "查看采集报告必须可读");
}

#[test]
fn invalid_geometry_is_isolated_but_raw_observation_is_kept() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    // 精确位于边界内的退化线由 B14 隔离；有效点与线 → 可评审。
    let invalid_line = RawEntity::with_geometry(
        "L_BAD",
        "退化建筑线",
        tags(&[("building", "school")]),
        serde_json::json!({"id": "L_BAD"}),
        Some(SourceGeometry::LineString(vec![
            (116.4005, 39.9005),
            (116.4005, 39.9005),
        ])),
        "line",
    );
    let source = Arc::new(FakeSource {
        entities: vec![
            invalid_line,
            building_point("B01", 116.4003, 39.9003),
            road_line("R01"),
        ],
    });
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5)).expect("后台完成");
    let CollectionOutcome::Succeeded(summary) = outcome else {
        let CollectionOutcome::Failed(failure) = outcome else {
            unreachable!()
        };
        panic!(
            "隔离不阻断同批其它对象，采集仍应成功：{}",
            failure.diagnostic
        );
    };

    let raw = db
        .lock()
        .expect("db lock")
        .list_raw_observations(&plan_id.to_string())
        .expect("读取原始观测");
    assert_eq!(raw.len(), 3, "无效几何的原始观测必须保留（数据粮仓铁律）");

    let reviewable = db
        .lock()
        .expect("db lock")
        .list_reviewable_candidate_projections(&plan_id.to_string())
        .expect("读取可评审候选");
    assert_eq!(reviewable.len(), 2, "只有有效点/线进入候选池");

    let report = summary.page.report.expect("报告存在");
    for expected in ["边界内 3 项", "无效几何 0 项", "最终可评审 2 项"] {
        assert!(
            report.candidate_lines.iter().any(|line| line == expected),
            "边界阶段计数必须独立于 B14 隔离和最终可评审计数；缺少 {expected}: {:?}",
            report.candidate_lines
        );
    }
    assert!(
        report
            .candidate_lines
            .iter()
            .any(|line| line.contains("已隔离") && line.contains('1')),
        "报告必须如实呈现隔离数量：{report:?}"
    );
    assert!(
        report
            .candidate_lines
            .iter()
            .any(|line| line.contains("可评审") && line.contains('2')),
        "报告必须如实呈现可评审数量：{report:?}"
    );
}

#[test]
fn missing_source_geometry_is_isolated_without_fabricated_shape() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    // 无来源几何的对象：保留原始观测，隔离进候选池，不伪造形状。
    let no_geometry = RawEntity::new(
        "NG01",
        "无形状对象",
        tags(&[("building", "school")]),
        serde_json::json!({"id": "NG01"}),
    );
    let source = Arc::new(FakeSource {
        entities: vec![no_geometry],
    });
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5)).expect("后台完成");
    let CollectionOutcome::Succeeded(summary) = outcome else {
        let CollectionOutcome::Failed(failure) = outcome else {
            unreachable!()
        };
        panic!(
            "无几何对象隔离后采集仍应成功（不伪造候选形状）：{}",
            failure.diagnostic
        );
    };

    let raw = db
        .lock()
        .expect("db lock")
        .list_raw_observations(&plan_id.to_string())
        .expect("读取原始观测");
    assert_eq!(raw.len(), 1, "无几何对象的原始观测必须保留");

    let reviewable = db
        .lock()
        .expect("db lock")
        .list_reviewable_candidate_projections(&plan_id.to_string())
        .expect("读取可评审候选");
    assert!(
        reviewable.is_empty(),
        "无形状对象不得进入候选池（ADR-0040）"
    );
    assert!(summary.page.report.is_some());
    assert!(
        summary
            .page
            .report
            .as_ref()
            .unwrap()
            .candidate_lines
            .iter()
            .any(|line| line.contains("已隔离")),
        "报告必须呈现已隔离对象"
    );
}

#[test]
fn empty_collection_is_a_legal_complete_run() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let source = Arc::new(FakeSource {
        entities: Vec::new(),
    });
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5)).expect("后台完成");
    let CollectionOutcome::Succeeded(summary) = outcome else {
        panic!("空候选集合是合法采集结果（ADR-0041），不得伪装失败");
    };
    assert_eq!(summary.page.progress.collected_total, 0);
    assert!(summary.page.report.is_some());
    assert!(summary.page.review_unlocked);
}

#[test]
fn collection_failure_is_structured_and_presented_without_fake_success() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let flow = flow(Arc::clone(&db), Arc::new(FailingSource));
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5)).expect("后台完成");
    let CollectionOutcome::Failed(failure) = outcome else {
        panic!("数据源不可达必须结构化失败，不得出现伪成功产物");
    };

    assert_eq!(failure.page.status, CollectionStatus::Failed);
    assert!(!failure.page.review_unlocked, "失败不得解锁评审");
    let notification = failure.notification.expect("失败必须携带 B7 通知事实");
    assert!(notification.level.is_error(), "采集失败必须走 error 级弹窗");
    assert!(
        !notification.body.contains("collection.") && !notification.body.contains("error."),
        "通知正文必须是解析后的中文，不能是文本键：{}",
        notification.body
    );
    assert!(
        failure.diagnostic.contains("failing"),
        "A1 不吞错：诊断详情必须保留原错误：{}",
        failure.diagnostic
    );
    assert!(failure.page.failure.is_some());

    // 无伪成功产物：原始观测与候选投影都不能出现。
    let raw = db
        .lock()
        .expect("db lock")
        .list_raw_observations(&plan_id.to_string())
        .expect("读取原始观测");
    assert!(raw.is_empty(), "失败不得写入原始观测");
    let reviewable = db
        .lock()
        .expect("db lock")
        .list_reviewable_candidate_projections(&plan_id.to_string())
        .expect("读取可评审候选");
    assert!(reviewable.is_empty(), "失败不得发布候选投影");
    assert!(flow.report_view().is_none(), "失败后不得有伪成功报告");
}

#[test]
fn failed_collection_keeps_boundary_export_eligible() {
    let directory = tempfile::tempdir().expect("临时目录");
    let db = Arc::new(Mutex::new(
        Database::open(directory.path().join("flow.db")).expect("文件库"),
    ));
    let plan_id = PlanId::generate();
    let plan_context = PlanContextView {
        plan_id: plan_id.to_string(),
        plan_name: "导出资格方案".to_owned(),
        campus_id: "campus-export".to_owned(),
        campus_name: "导出资格校区".to_owned(),
        anchor_lng: 116.4,
        anchor_lat: 39.9,
    };

    // 采集失败：数据源不可达。
    let collection_flow = flow(Arc::clone(&db), Arc::new(FailingSource));
    collection_flow.set_plan(&plan_id);
    collection_flow.confirm_boundary(boundary());
    let mut operation = collection_flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5)).expect("后台完成");
    assert!(
        matches!(outcome, CollectionOutcome::Failed(_)),
        "采集必须失败"
    );

    // 采集失败不得取消已确认边界的基础导出资格：F9 边界直出仍成功。
    let export_flow = BoundaryExportFlow::new(Arc::new(StdExportFileSystem));
    let mut settings = SettingsManager::new(Database::open_in_memory().expect("测试设置库"));
    settings
        .set_default_export_location(directory.path().to_str().expect("临时路径"))
        .expect("设置导出目录");
    export_flow.sync_settings(&settings);
    export_flow.set_plan(&plan_context);
    export_flow.confirm_boundary(
        "Polygon",
        serde_json::json!([[
            [116.4000, 39.9000],
            [116.4010, 39.9000],
            [116.4010, 39.9010],
            [116.4000, 39.9010],
            [116.4000, 39.9000]
        ]]),
    );
    let mut export_operation = export_flow.start().expect("Start 导出");
    let export_result = wait_for_export(&mut export_operation);
    let export_result = export_result.expect("采集失败后基础导出仍必须成功");
    assert!(export_result.schematic_path.is_file());
    assert!(export_result.manifest_path.is_file());
}

fn wait_for_export(
    operation: &mut export_flow::BoundaryExportOperation,
) -> export_flow::Result<export_flow::BoundaryExportResult> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = operation.try_complete() {
            return result;
        }
        assert!(Instant::now() < deadline, "导出操作未达到终态");
        std::thread::yield_now();
    }
}

#[test]
fn cancel_does_not_pull_back_old_result() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let blocking = Arc::new(BlockingSource::new());
    blocking
        .entities
        .lock()
        .expect("entities lock")
        .push(building_point("B01", 116.4003, 39.9003));
    let flow = flow(Arc::clone(&db), blocking.clone());
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    assert!(
        operation.try_complete().is_none(),
        "fetch 阻塞时不得提前终态"
    );

    flow.cancel();
    blocking.release();

    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5));
    assert!(
        matches!(outcome, Err(CollectionError::Expired)),
        "取消后旧结果不得被拉回：{outcome:?}"
    );
    assert_eq!(
        flow.page_view().status,
        CollectionStatus::Pending,
        "取消后页面不得呈现成功"
    );
    assert!(
        !flow.is_review_unlocked(&plan_id.to_string()),
        "取消不得解锁评审"
    );
}

#[test]
fn switching_plans_isolates_old_collection_results() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let blocking = Arc::new(BlockingSource::new());
    let flow = flow(Arc::clone(&db), blocking.clone());

    let plan_a = PlanId::generate();
    let plan_b = PlanId::generate();
    seed_plan(&flow, &plan_a);

    let mut old_operation = flow.start().expect("Start 方案 A");
    assert!(old_operation.try_complete().is_none());

    // 切换方案：旧采集结果过期，不得交付到新方案。
    blocking
        .entities
        .lock()
        .expect("entities lock")
        .push(building_point("B01", 116.4003, 39.9003));
    flow.set_plan(&plan_b);
    flow.confirm_boundary(boundary());
    blocking.release();

    let old_outcome = wait_for_terminal(&mut old_operation, Duration::from_secs(5));
    assert!(
        matches!(old_outcome, Err(CollectionError::Expired)),
        "切换方案后旧采集结果必须过期隔离：{old_outcome:?}"
    );

    // 新方案采集成功，结果按方案隔离。
    let mut new_operation = flow.start().expect("Start 方案 B");
    let new_outcome =
        wait_for_terminal(&mut new_operation, Duration::from_secs(5)).expect("后台完成");
    assert!(
        matches!(new_outcome, CollectionOutcome::Succeeded(_)),
        "新方案采集应成功"
    );
    assert!(flow.is_review_unlocked(&plan_b.to_string()));
    assert!(
        !flow.is_review_unlocked(&plan_a.to_string()),
        "被取消/过期的旧方案不得解锁评审"
    );
}

#[test]
fn start_without_confirmed_boundary_is_rejected() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let flow = flow(Arc::clone(&db), Arc::new(FailingSource));
    let plan_id = PlanId::generate();
    flow.set_plan(&plan_id);

    assert!(
        matches!(flow.start(), Err(CollectionError::MissingInput)),
        "无已确认边界不得开始候选采集"
    );

    flow.confirm_boundary(boundary());
    flow.reset_boundary();
    assert!(
        matches!(flow.start(), Err(CollectionError::MissingInput)),
        "重置边界后采集输入同步失效"
    );
}

#[test]
fn busy_start_is_rejected_until_finish() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let blocking = Arc::new(BlockingSource::new());
    let flow = flow(Arc::clone(&db), blocking.clone());
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut first = flow.start().expect("第一次 Start");
    assert!(
        matches!(flow.start(), Err(CollectionError::Busy)),
        "采集进行中不接受第二次开始意图"
    );

    blocking.release();
    let _ = wait_for_terminal(&mut first, Duration::from_secs(5));
    flow.set_plan(&plan_id);
    let _second_ok = flow.start().expect("完成后可再次开始");
    blocking.release();
}

#[test]
fn report_view_reflects_last_completed_collection() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let source = Arc::new(FakeSource {
        entities: vec![building_point("B01", 116.4003, 39.9003)],
    });
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5)).expect("后台完成");
    let CollectionOutcome::Succeeded(summary) = outcome else {
        panic!("应成功");
    };
    let report = flow.report_view().expect("报告可读");
    assert_eq!(report.title, summary.page.report.unwrap().title);
    assert!(!report.category_lines.is_empty(), "报告必须包含类别汇总");
    assert!(
        !report.candidate_lines.is_empty(),
        "报告必须包含候选投影汇总"
    );
}

#[test]
fn failed_run_does_not_leave_a_report() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let flow = flow(Arc::clone(&db), Arc::new(FailingSource));
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    let _ = wait_for_terminal(&mut operation, Duration::from_secs(5));
    assert!(flow.report_view().is_none(), "失败后不得有伪成功报告");
}

#[test]
fn page_view_reflects_fetching_while_running() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let blocking = Arc::new(BlockingSource::new());
    let flow = flow(Arc::clone(&db), blocking.clone());
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    assert_eq!(flow.page_view().status, CollectionStatus::Fetching);
    blocking.release();
    let _ = wait_for_terminal(&mut operation, Duration::from_secs(5));
    assert_eq!(flow.page_view().status, CollectionStatus::Completed);
}

#[test]
fn operation_is_send_usable_across_threads() {
    // CollectionOperation 必须能跨线程轮询（后台结果经通道回传）。
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let source = Arc::new(FakeSource {
        entities: vec![building_point("B01", 116.4003, 39.9003)],
    });
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let mut operation = flow.start().expect("Start");
    let done = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done);
    let handle = std::thread::spawn(move || {
        let outcome = wait_for_terminal(&mut operation, Duration::from_secs(5));
        assert!(outcome.is_ok(), "后台线程轮询应取得成功结果");
        done_clone.store(true, Ordering::SeqCst);
    });
    handle.join().expect("轮询线程");
    assert!(done.load(Ordering::SeqCst));
}

fn unnamed_polygons_payload(count: usize) -> String {
    let elements: Vec<serde_json::Value> = (0..count)
        .map(|index| {
            let base_lon = 121.40 + index as f64 * 0.001;
            serde_json::json!({
                "type": "way",
                "id": 10_000 + index as i64,
                "tags": {"building": "yes"},
                "geometry": [
                    {"lat": 31.200, "lon": base_lon},
                    {"lat": 31.201, "lon": base_lon},
                    {"lat": 31.201, "lon": base_lon + 0.001},
                    {"lat": 31.200, "lon": base_lon + 0.001},
                    {"lat": 31.200, "lon": base_lon}
                ]
            })
        })
        .collect();
    serde_json::json!({ "elements": elements }).to_string()
}

#[test]
fn slow_regeo_enrichment_respects_overall_deadline_with_partial_feedback() {
    // T36 验收 1：注入“慢/挂起 regeo transport（每次 5s 超时）”，
    // 断言整次采集 ≤ 总体截止时间，且降级路径如实标注“部分建筑未命名”。
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let payload = unnamed_polygons_payload(20);
    let overpass_transport = Box::new(move |_boundary: &Boundary| Ok(payload.clone()));
    let regeo_transport = Box::new(|_: &str, timeout: Duration| {
        let (_tx, rx) = std::sync::mpsc::channel::<()>();
        let _ = rx.recv_timeout(timeout);
        Err("模拟 regeo 挂起".to_owned())
    });
    let namer = Arc::new(RegeoNamer::new(
        regeo_transport,
        Box::new(|| Some("web-key".to_owned())),
    ));
    let source: Arc<dyn DataSource + Send + Sync> =
        Arc::new(OverpassDataSource::new(overpass_transport).with_name_enricher(Some(namer)));
    let limits = CollectionRunLimits {
        overall_deadline: Duration::from_secs(10),
        local_tail_margin: Duration::from_secs(5),
    };
    let flow = flow_with_limits(Arc::clone(&db), source, limits);
    let plan_id = PlanId::generate();
    flow.set_plan(&plan_id);
    let convert = |lon: f64, lat: f64| {
        let (lon, lat) = gaode_client::wgs84_to_gcj02(lon, lat);
        serde_json::json!([lon, lat])
    };
    flow.confirm_boundary(Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            convert(121.399, 31.199),
            convert(121.425, 31.199),
            convert(121.425, 31.202),
            convert(121.399, 31.202),
            convert(121.399, 31.199)
        ]]),
    });

    let started = Instant::now();
    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(10))
        .expect("后台采集必须在截止时间内到达终态");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(10),
        "整次采集必须 ≤ 总体截止时间（10s），实际 {elapsed:?}"
    );
    let CollectionOutcome::Succeeded(summary) = outcome else {
        panic!("补名降级后采集仍应成功落库，不得静默假失败");
    };
    assert!(
        summary.page.progress.naming_partial,
        "补名截止/失败必须如实标注“部分建筑未命名”"
    );
    assert_eq!(summary.page.status, CollectionStatus::Completed);
}

#[test]
fn three_endpoint_hang_overpass_fails_within_overall_deadline() {
    // T36 验收 2：注入“三端点全挂 Overpass transport”（每端点 5s 超时），
    // 断言 ≤ 整体查询截止（15s，远小于运行总体截止 60s）并结构化失败。
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let client = OverpassClient::with_transport(Box::new(|_: &str, timeout: Duration| {
        let (_tx, rx) = std::sync::mpsc::channel::<()>();
        let _ = rx.recv_timeout(timeout);
        Err("模拟 Overpass 端点挂起".to_owned())
    }));
    let overpass_transport = Box::new(move |boundary: &Boundary| {
        let bbox = boundary_bbox(boundary, 0.01).ok_or_else(|| "边界包围盒失败".to_owned())?;
        let query = campus_objects_query(bbox)
            .map_err(|error| format!("集中标签规则无法生成采集查询：{error}"))?;
        client
            .query_with_fallback(&query)
            .map_err(|message| format!("Overpass 采集查询失败：{message}"))
    });
    let source: Arc<dyn DataSource + Send + Sync> =
        Arc::new(OverpassDataSource::new(overpass_transport));
    let flow = flow(Arc::clone(&db), source);
    let plan_id = PlanId::generate();
    seed_plan(&flow, &plan_id);

    let started = Instant::now();
    let mut operation = flow.start().expect("Start");
    let outcome = wait_for_terminal(&mut operation, Duration::from_secs(20))
        .expect("三端点全挂必须在运行截止前结构化失败");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(20),
        "整体查询不得超过整体截止（验收 ≤15s + 余量），实际 {elapsed:?}"
    );
    let CollectionOutcome::Failed(failure) = outcome else {
        panic!("三端点全挂必须结构化失败，不得出现假成功产物");
    };
    assert_eq!(failure.page.status, CollectionStatus::Failed);
    assert!(
        failure.notification.is_some(),
        "失败必须携带 B7 错误弹窗事实"
    );
    assert!(
        failure.diagnostic.contains("端点"),
        "诊断必须保留端点回退事实：{}",
        failure.diagnostic
    );
}
