//! F9 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在本测试与 snapshots/public-api.txt 中，
//! PR diff 可见。附带 B6 国际化验收：本 crate 产出的全部文本键必须在
//! zh-CN.json 中逐条可解析（ADR-0005，文案外置）。

use export_console::{
    adapt_to_voxel_model, text_keys, Error, ExportConsole, ExportProgressView, ExportRequest,
    ExportStage, MockExportConsole, MockSealGate, NavigationTarget, ProgressTracker, SealGate,
};
use generation_engine::{BlockModel, BlockPosition};
use shared_domain_types::{CandidateCategory, PlanId};

#[test]
fn public_api_types_exist() {
    // ExportRequest：缝 5 输入（保留项集合 + 类别汇总 + 待定计数）
    let plan_id = PlanId::generate();
    let request = ExportRequest::new(
        plan_id.to_string(),
        vec![(CandidateCategory::Building, 5)],
        5,
        3,
        2,
        vec!["Building/way/1".to_owned()],
    );
    assert_eq!(request.keep_total, 5);
    assert_eq!(request.pending_count, 3);
    assert_eq!(request.keep_candidates.len(), 1);

    // 保留项为零也合法（最小路径，缝 5 不拦截）
    let empty = ExportRequest::new(plan_id.to_string(), vec![], 0, 0, 0, vec![]);
    assert_eq!(empty.keep_total, 0);

    // SealGate trait + MockSealGate：封账/解封
    let gate = MockSealGate::new();
    assert!(gate.seal(&plan_id).is_ok());
    assert!(gate.is_sealed());
    assert!(gate.release(&plan_id).is_ok());
    assert!(!gate.is_sealed());

    // ExportConsole 状态机：加载 → 弹窗 → 取消 → 重新加载 → 确认封账
    let gate = MockSealGate::new();
    let probe = gate.clone();
    let mut console: MockExportConsole = ExportConsole::new(gate);
    console.load_request(request.clone()).unwrap();
    let dialog = console.confirm_dialog_view().expect("待确认必有弹窗视图");
    assert_eq!(dialog.title_key, "export.confirm_title");
    assert_eq!(dialog.summary_rows.len(), 1);
    assert_eq!(dialog.summary_rows[0].keep_count, 5);
    assert_eq!(dialog.pending_count, 3);

    console.cancel().unwrap();
    assert!(!probe.is_sealed());

    console.load_request(request).unwrap();
    console.confirm_export().unwrap();
    assert!(probe.is_sealed());
    assert!(console.is_exporting());

    // 缝 6 执行：空模型也能落出合法 .schem，成功产出跳转目标
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("campus.schem");
    let mut model = BlockModel::new();
    model.set_block(BlockPosition::new(0, 0, 0), "minecraft:stone");
    let target = console.execute_export(&model, &output).unwrap();
    assert!(matches!(target, NavigationTarget::ExportCompleted(_)));
    if let NavigationTarget::ExportCompleted(summary) = target {
        assert_eq!(summary.export_count, 5);
        assert!(summary.output_path.ends_with("campus.schem"));
    }

    // ProgressTracker + ExportProgressView：非阻塞进度条
    let tracker = ProgressTracker::new();
    tracker.set_stage(ExportStage::Generating);
    assert!(tracker.report_percent(47)); // 对齐到 45
    assert_eq!(tracker.percent(), 45);
    let view = ExportProgressView::from_tracker(&tracker);
    assert!(view.visible);
    assert_eq!(view.percent, 45);
    assert!(!view.is_done);
    tracker.finish();
    assert!(ExportProgressView::from_tracker(&tracker).is_done);

    // ExportStage：阶段 → 文本键（zh-CN.json 既有键）
    assert_eq!(ExportStage::Generating.label_key(), "export.in_progress");
    assert_eq!(ExportStage::Done.label_key(), "export.done");
    assert_eq!(ExportStage::Failed.label_key(), "error.export_failed");

    // 缝 6 适配器：B18 BlockModel → B4 VoxelModel
    let voxel = adapt_to_voxel_model(&model).unwrap();
    assert_eq!(voxel.palette[0], "minecraft:air");

    // Error #[non_exhaustive]：带类型错误可匹配
    let gate = MockSealGate::new();
    let mut idle: MockExportConsole = ExportConsole::new(gate);
    let err: Error = idle.confirm_export().unwrap_err();
    assert!(matches!(err, Error::InvalidState(_)));
    assert!(!err.to_string().is_empty());
}

/// B6 国际化验收：本 crate 产出的全部文本键在 zh-CN.json 中逐条可解析
/// （`t()` 查不到键时原样返回键名——据此断言解析结果不等于键名）。
#[test]
fn every_emitted_text_key_resolves_in_zh_cn() {
    use localization::{Language, Localization};
    let l10n = Localization::new(Language::ZhCn).expect("zh-CN.json 可加载");
    let keys = [
        text_keys::START_BUTTON,
        text_keys::CONFIRM_TITLE,
        text_keys::CONFIRM_SUMMARY,
        text_keys::SEAL_NOTICE,
        text_keys::PENDING_NOTICE,
        text_keys::IN_PROGRESS,
        text_keys::DONE,
        text_keys::EXPORT_FAILED,
        text_keys::CONFIRM_BUTTON,
        text_keys::CANCEL_BUTTON,
        // 类别显示名（collection 命名空间既有键，与 F5 同源）
        "collection.category_building",
        "collection.category_road",
        "collection.category_water",
        "collection.category_vegetation",
        "collection.category_sports",
        "collection.category_other",
    ];
    for key in keys {
        assert_ne!(l10n.t(key), key, "文本键 {key} 在 zh-CN.json 中缺失");
    }
}
