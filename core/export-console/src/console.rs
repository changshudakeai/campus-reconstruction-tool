//! F9 核心状态机：确认弹窗 → 封账 → 导出 → 跳转/回滚
//!
//! 状态推进（显式状态机，非法驱动一律带类型错误拒绝）：
//!
//! ```text
//! Idle ─load_request→ RequestReady ─confirm_export→ Exporting
//!   ↑                     │cancel                       │execute_export
//!   └─────────────────────┘            成功→ Completed / 失败→ Failed(已回滚)
//! ```
//!
//! 弹窗铁律（ADR-0021）在此落地：导出失败走 B7 [`notification_center::error`]
//! （模态弹窗 + 留底），导出完成走 [`notification_center::warn`]（toast + 留底）。
//! B7 收成品文字：文本键在本层经 B6 解析后递入。

use std::path::Path;
use std::sync::Arc;

use generation_engine::BlockModel;
use manifest_generator::MaterialTable;
use shared_domain_types::PlanId;

use crate::boundary_export::{
    BoundaryExportInput, BoundaryExportOperation, BoundaryExportPort, BoundaryExportRequest,
    BoundaryExportResult, BoundaryExportUseCase, ExportFileSystem, StdExportFileSystem,
};
use crate::data::{ExportRequest, ExportStage, ExportSummary};
use crate::error::{Error, Result};
use crate::pipeline;
use crate::progress::ProgressTracker;
use crate::seal_gate::SealGate;
use crate::views::{text_keys, ExportConfirmDialogView, ExportProgressView, NavigationTarget};

/// B7 留底消息的来源标签前缀取自方案 ID（跨方案不混淆，ADR-0021）
fn source_tag(plan_id: &str) -> String {
    plan_id.to_owned()
}

/// 文本键 → 成品文字（B6 全局翻译器未初始化时原样返回键名，不 panic——
/// 无界面环境下 B7 仍能留底，消息不静默丢弃）
fn resolve_text(key: &str) -> String {
    localization::GLOBAL_LOCALIZATION
        .lock()
        .ok()
        .and_then(|global| global.as_ref().map(|l10n| l10n.t(key)))
        .unwrap_or_else(|| key.to_owned())
}

/// 内部状态（显式状态机）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// 空闲：未收到导出请求
    Idle,
    /// 已收到缝 5 请求，确认弹窗待用户裁决
    RequestReady,
    /// 已封账，导出进行中（评审入口禁用，界面不冻结）
    Exporting,
    /// 导出成功（已通知 + 已产出跳转目标）
    Completed,
    /// 导出失败（封账已回滚，评审恢复可改）
    Failed,
}

/// F9 导出控制台
pub struct ExportConsole<G: SealGate> {
    request: Option<ExportRequest>,
    gate: G,
    progress: ProgressTracker,
    state: State,
    boundary_export: Arc<BoundaryExportUseCase>,
}

impl<G: SealGate> ExportConsole<G> {
    /// 用封账门控创建（门控由壳接线到 F5，测试用 Mock）
    pub fn new(gate: G) -> Self {
        Self::new_with_material_table(gate, MaterialTable::v26_1_2_school())
    }

    /// 用指定 B17 用料表创建；完整边界直出用例仍由 F9 统一协调。
    pub fn new_with_material_table(gate: G, material_table: MaterialTable) -> Self {
        Self::new_with_material_table_and_file_system(
            gate,
            material_table,
            Arc::new(StdExportFileSystem),
        )
    }

    /// 用指定用料表与文件端口创建 F9 控制台。
    pub fn new_with_material_table_and_file_system(
        gate: G,
        material_table: MaterialTable,
        file_system: Arc<dyn ExportFileSystem>,
    ) -> Self {
        Self {
            request: None,
            gate,
            progress: ProgressTracker::new(),
            state: State::Idle,
            boundary_export: Arc::new(BoundaryExportUseCase::new(material_table, file_system)),
        }
    }

    /// 构造一个由 F9 拥有完整输入/执行链的异步能力端口。
    pub fn boundary_export_port(&self, input: Arc<dyn BoundaryExportInput>) -> BoundaryExportPort {
        BoundaryExportPort::new(Arc::clone(&self.boundary_export), input)
    }

    /// 直接启动异步边界导出；生产 S1 使用 [`Self::boundary_export_port`]。
    pub fn start_boundary_export(
        &self,
        input: Arc<dyn BoundaryExportInput>,
    ) -> Result<BoundaryExportOperation> {
        self.boundary_export_port(input).start()
    }

    /// 完整边界直出入口（ADR-0041）。
    ///
    /// 调用方只提交一次已确认边界与可选朝向；边界资格、默认正北、空候选
    /// 最小场地、manifest 与 `.schem` 的生成/发布都在 F9 内完成。
    pub fn export_confirmed_boundary(
        &mut self,
        request: BoundaryExportRequest,
    ) -> Result<BoundaryExportResult> {
        if self.state == State::Exporting {
            return Err(Error::InvalidState("导出进行中，不接受新的导出请求"));
        }
        self.progress = ProgressTracker::new();
        self.state = State::Exporting;
        let plan_id = request.plan.plan_id.to_string();
        match self.boundary_export.export(&request, &self.progress) {
            Ok(result) => {
                self.state = State::Completed;
                notification_center::warn(
                    source_tag(&plan_id),
                    resolve_text(text_keys::DONE),
                    result.schematic_path.display().to_string(),
                );
                Ok(result)
            }
            Err(error) => {
                self.state = State::Failed;
                notification_center::error(
                    source_tag(&plan_id),
                    resolve_text(text_keys::EXPORT_FAILED),
                    error.to_string(),
                );
                Err(error)
            }
        }
    }

    // ── 缝 5：接收导出请求 + 确认弹窗 ───────────────────────

    /// 接收 F5 递交的导出请求（保留项为零也合法——最小路径，缝 5 不拦截）。
    ///
    /// 导出进行中不接受新请求（评审入口已禁用，正常流程到不了这里）。
    pub fn load_request(&mut self, request: ExportRequest) -> Result<()> {
        if self.state == State::Exporting {
            return Err(Error::InvalidState("导出进行中，不接受新的导出请求"));
        }
        self.request = Some(request);
        self.progress = ProgressTracker::new();
        self.state = State::RequestReady;
        Ok(())
    }

    /// 确认弹窗视图（汇总 + 封账后果 + 待定报数）；无请求时为 None
    pub fn confirm_dialog_view(&self) -> Option<ExportConfirmDialogView> {
        match self.state {
            State::RequestReady => self
                .request
                .as_ref()
                .map(ExportConfirmDialogView::from_request),
            _ => None,
        }
    }

    /// 用户点"取消"：丢弃请求，原样返回评审（缝 5 契约）
    pub fn cancel(&mut self) -> Result<()> {
        if self.state != State::RequestReady {
            return Err(Error::InvalidState("没有等待确认的导出请求"));
        }
        self.request = None;
        self.state = State::Idle;
        Ok(())
    }

    // ── 封账闸门 ─────────────────────────────────────────

    /// 用户点"确认"：封账（确认即不可逆，ADR-0022）。
    ///
    /// 封账失败时返回 Err 且状态回到"待确认"——封账不生效，
    /// 评审保持可改（缝 4 契约），由调用方按弹窗铁律呈现错误。
    pub fn confirm_export(&mut self) -> Result<()> {
        if self.state != State::RequestReady {
            return Err(Error::InvalidState("没有等待确认的导出请求"));
        }
        let request = self.request.as_ref().expect("RequestReady 状态必有请求");
        let plan_id =
            PlanId::parse(&request.plan_id).map_err(|err| Error::BadPlanId(err.to_string()))?;

        self.progress.set_stage(ExportStage::Sealing);
        if let Err(reason) = self.gate.seal(&plan_id) {
            // 封账不生效：退回待确认，评审保持可改
            self.progress.set_stage(ExportStage::Waiting);
            return Err(Error::SealFailed(reason));
        }

        self.state = State::Exporting;
        self.progress.set_stage(ExportStage::Generating);
        Ok(())
    }

    // ── 缝 6：导出执行（成功跳转 / 失败回滚）────────────────

    /// 执行导出：B18 方块模型 → B4 .schem 落盘，返回跳转目标。
    ///
    /// - 成功：B7 toast"导出完成"（普通提示级），跳转导出完成页；
    /// - 失败：**封账回滚**（评审恢复可改）+ B7 模态弹窗（弹窗铁律），
    ///   跳转回评审台，并把带类型错误一路向上传递。
    pub fn execute_export(
        &mut self,
        model: &BlockModel,
        output_path: &Path,
    ) -> Result<NavigationTarget> {
        if self.state != State::Exporting {
            return Err(Error::InvalidState("尚未确认导出，不能执行导出"));
        }
        let request = self.request.as_ref().expect("Exporting 状态必有请求");
        let schematic_name = request.plan_id.clone();

        match pipeline::export_schematic(model, output_path, &schematic_name, &self.progress) {
            Ok(()) => Ok(self.complete_export(output_path)),
            Err(err) => {
                self.rollback_seal(&err);
                Err(err)
            }
        }
    }

    /// 导出成功收尾：toast 通知 + 产出跳转目标（导出完成页）
    fn complete_export(&mut self, output_path: &Path) -> NavigationTarget {
        let request = self.request.as_ref().expect("Exporting 状态必有请求");
        let summary = ExportSummary {
            plan_id: request.plan_id.clone(),
            export_count: request.keep_total,
            by_category: request.keep_by_category.clone(),
            output_path: output_path.display().to_string(),
        };
        self.state = State::Completed;

        // 普通提示级：toast + 留底（ADR-0021 三级表"导出完成"行）
        notification_center::warn(
            source_tag(&summary.plan_id),
            resolve_text(text_keys::DONE),
            summary.output_path.clone(),
        );
        NavigationTarget::ExportCompleted(summary)
    }

    /// 失败回滚：释放封账（评审恢复可改）+ 模态弹窗（弹窗铁律）
    fn rollback_seal(&mut self, err: &Error) {
        self.progress.fail();
        let plan_key = self
            .request
            .as_ref()
            .map(|request| request.plan_id.clone())
            .unwrap_or_default();
        if let Ok(plan_id) = PlanId::parse(&plan_key) {
            if let Err(reason) = self.gate.release(&plan_id) {
                // 解封失败也必须留痕（仍走弹窗——卡住流程级）
                notification_center::error(
                    source_tag(&plan_key),
                    resolve_text(text_keys::EXPORT_FAILED),
                    reason,
                );
            }
        }
        self.state = State::Failed;

        // 要紧错误级：模态弹窗 + 留底，禁止横幅（ADR-0021）
        notification_center::error(
            source_tag(&plan_key),
            resolve_text(text_keys::EXPORT_FAILED),
            err.to_string(),
        );
    }

    /// 失败后的跳转目标：回评审台继续评审（封账已回滚）
    pub fn failure_target(&self) -> Option<NavigationTarget> {
        (self.state == State::Failed).then_some(NavigationTarget::ContinueReview)
    }

    // ── 非阻塞进度条 ─────────────────────────────────────

    /// 进度追踪器（克隆给后台线程报进度）
    pub fn progress(&self) -> &ProgressTracker {
        &self.progress
    }

    /// 右上角浮动进度视图（UI 轮询产出当前一帧）
    pub fn progress_view(&self) -> ExportProgressView {
        ExportProgressView::from_tracker(&self.progress)
    }

    /// 是否导出进行中（评审入口禁用信号）
    pub fn is_exporting(&self) -> bool {
        self.state == State::Exporting
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seal_gate::MockSealGate;
    use shared_domain_types::CandidateCategory;

    fn request(plan_id: &str) -> ExportRequest {
        ExportRequest::new(
            plan_id.to_owned(),
            vec![(CandidateCategory::Building, 1)],
            1,
            2,
            0,
            vec!["Building/way/1".to_owned()],
        )
    }

    fn console_with_request() -> (ExportConsole<MockSealGate>, MockSealGate) {
        let gate = MockSealGate::new();
        let probe = gate.clone();
        let mut console = ExportConsole::new(gate);
        console
            .load_request(request(&PlanId::generate().to_string()))
            .unwrap();
        (console, probe)
    }

    #[test]
    fn confirm_dialog_appears_only_when_request_ready() {
        let gate = MockSealGate::new();
        let mut console = ExportConsole::new(gate);
        assert!(console.confirm_dialog_view().is_none());

        console
            .load_request(request(&PlanId::generate().to_string()))
            .unwrap();
        let dialog = console.confirm_dialog_view().unwrap();
        assert_eq!(dialog.pending_count, 2);
    }

    #[test]
    fn cancel_returns_to_review_without_sealing() {
        let (mut console, probe) = console_with_request();
        console.cancel().unwrap();
        assert!(!probe.is_sealed());
        assert!(console.confirm_dialog_view().is_none());
    }

    #[test]
    fn confirm_seals_and_disables_review_entry() {
        let (mut console, probe) = console_with_request();
        console.confirm_export().unwrap();
        assert!(probe.is_sealed());
        assert!(console.is_exporting());
        // 导出中确认弹窗不再出现
        assert!(console.confirm_dialog_view().is_none());
    }

    #[test]
    fn seal_failure_keeps_request_ready_state() {
        let gate = MockSealGate::new();
        let probe = gate.clone();
        // 预先封账：下一次 seal 必然失败（模拟写库失败）
        probe.seal(&PlanId::generate()).unwrap();

        let mut console = ExportConsole::new(gate);
        console
            .load_request(request(&PlanId::generate().to_string()))
            .unwrap();
        let err = console.confirm_export().unwrap_err();
        assert!(matches!(err, Error::SealFailed(_)));
        // 封账失败 → 仍在待确认状态，可再次确认（评审保持可改）
        assert!(console.confirm_dialog_view().is_some());
        assert!(!console.is_exporting());
    }

    #[test]
    fn bad_plan_id_is_rejected_before_sealing() {
        let gate = MockSealGate::new();
        let probe = gate.clone();
        let mut console = ExportConsole::new(gate);
        console.load_request(request("不是 UUID")).unwrap();
        let err = console.confirm_export().unwrap_err();
        assert!(matches!(err, Error::BadPlanId(_)));
        assert!(!probe.is_sealed());
    }

    #[test]
    fn invalid_state_transitions_are_rejected() {
        let gate = MockSealGate::new();
        let mut console = ExportConsole::new(gate);
        assert!(matches!(
            console.confirm_export(),
            Err(Error::InvalidState(_))
        ));
        assert!(matches!(console.cancel(), Err(Error::InvalidState(_))));
        assert!(matches!(
            console.execute_export(&BlockModel::new(), Path::new("x.schem")),
            Err(Error::InvalidState(_))
        ));
    }
}
