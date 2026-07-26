//! T19B-1 —— VM 注入器：壳与 F 模块之间唯一的连接桥（缝 1 基础设施）。
//!
//! 薄壳原则（ADR-0017）：壳零业务逻辑，本模块只做三件事——
//! 1. [`ViewModelInjector::new`] 构造并持有全部 F 模块实例；
//! 2. [`ViewModelInjector::inject`] 把各 VM 的视图状态绑定到 Slint
//!    窗口的 in property（只设属性，不改生成代码的行为逻辑）；
//! 3. 为后续 T19B 工单预留接线钥匙（各模块访问器 + 评审进台会话）。
//!
//! 主窗口导航骨架由 ADR-0027 决策，页面级绑定归 T19B-2..8；
//! 本模块不发明任何页面或导航 UI。回调错误的统一出口见
//! [`crate::report_callback_error`]（弹窗铁律 ADR-0021，经 B7 分派）。

use std::path::Path;

use anyhow::Result;
use coverage_audit::QuietSentinel;
use data_acquisition::AcquisitionPipeline;
use data_persistence::Database;
use export_console::{ExportConsole, MockSealGate};
use global_settings::SettingsManager;
use localization::{Language, Localization};
use onboarding_tutorial::OnboardingTutorial;
use project_management::ProjectManager;
use review_workbench::ReviewWorkbench;
use shared_domain_types::PlanId;

use crate::runtime::{decide, status_text, LandingDecision};
use crate::AppWindow;

/// 壳持有的 B2 连接组。
///
/// B2 `Database` 有意不可 `Clone`（一个持有者一条连接）；F1 与 F3 都
/// 按值持有句柄，因此壳对同一数据库文件开两条连接。F2/F5/F7 的落库
/// 操作统一借道 F3 [`ProjectManager::database_mut`]，不再额外开连接。
pub struct ShellDatabases {
    /// F1 全局设置的专属连接
    settings: Database,
    /// F3 方案管理的专属连接（`database_mut` 供 F2/F5/F7 借用）
    projects: Database,
}

impl ShellDatabases {
    /// 对同一数据库文件打开两条连接（开发版入口用）
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            settings: Database::open(path.as_ref())?,
            projects: Database::open(path.as_ref())?,
        })
    }

    /// 内存库连接组（测试用；注意两条连接是相互独立的内存库）
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            settings: Database::open_in_memory()?,
            projects: Database::open_in_memory()?,
        })
    }
}

/// VM 注入器：构造并持有全部 F 模块实例，把视图状态注入 Slint 窗口。
///
/// 借鉴依赖注入思想，但由 Rust 运行时直接构造而非 IoC 容器；
/// 壳内其余代码一律经本类型访问 F 模块，不各自另起炉灶。
pub struct ViewModelInjector {
    /// B6 文案解析（文本外置铁律 ADR-0005）
    l10n: Localization,
    /// F1 全局设置（自持一条 B2 连接）
    settings: SettingsManager,
    /// F2 新手教程（引导进度经 B2 app_settings 持久化）
    tutorial: OnboardingTutorial,
    /// F3 方案管理（自持一条 B2 连接）
    projects: ProjectManager,
    /// F4 数据采集流水线（内含 B13 归类引擎）
    acquisition: AcquisitionPipeline,
    /// F5 评审工作台：按方案进台的会话（[`Self::enter_review`] 装载）
    review: Option<ReviewWorkbench>,
    /// F7 覆盖率审计安静哨兵
    sentinel: QuietSentinel,
    /// F9 导出控制台。门控暂用 `MockSealGate` 占位——真门控（壳实现
    /// `SealGate`、内部调 F5 `seal`）随 T19B-8 导出接线落地。
    export: ExportConsole<MockSealGate>,
}

impl ViewModelInjector {
    /// 构造并持有全部 F 模块实例（F1-F9，B8/F6/F8 未立户不在册）。
    pub fn new(db: ShellDatabases) -> Result<Self> {
        let l10n = Localization::new(Language::ZhCn).map_err(anyhow::Error::msg)?;
        // F2 引导进度与 F3 同库装载（生产环境两连接指向同一文件）
        let tutorial = OnboardingTutorial::load(&db.projects)?;
        Ok(Self {
            l10n,
            settings: SettingsManager::new(db.settings),
            tutorial,
            projects: ProjectManager::new(db.projects),
            acquisition: AcquisitionPipeline::new()?,
            review: None,
            sentinel: QuietSentinel::new(),
            export: ExportConsole::new(MockSealGate::new()),
        })
    }

    /// 把 VM 视图状态注入 Slint 窗口（只设 in property）。
    ///
    /// Phase1 最小窗口只有标题与着陆状态两个属性；页面级属性与回调
    /// 随 T19B-2..8 逐单接线（导航骨架法源 ADR-0027）。
    pub fn inject(&self, window: &AppWindow) {
        window.set_app_title(self.l10n.t("app.welcome_title").into());
        window.set_status_text(status_text(&self.l10n, &self.landing()).into());
    }

    /// 首开着陆判定（委托 F1，壳只消费结果）
    pub fn landing(&self) -> LandingDecision {
        decide(&self.settings)
    }

    /// 评审进台：从 B2 一次性读入该方案的候选集（缝 4）。
    ///
    /// 再次调用即重新装载——导出失败回滚后"丢弃已封账实例、重新
    /// 进台"的落点（见 F9 `SealGate` 文档）。
    pub fn enter_review(&mut self, plan_id: &PlanId) -> Result<()> {
        self.review = Some(ReviewWorkbench::load(
            self.projects.database_mut(),
            plan_id,
        )?);
        Ok(())
    }

    // ── 接线钥匙：后续 T19B 工单经这些访问器绑定回调 ─────────────

    /// B6 文案解析器
    pub fn l10n(&self) -> &Localization {
        &self.l10n
    }

    /// F1 全局设置
    pub fn settings(&self) -> &SettingsManager {
        &self.settings
    }

    /// F1 全局设置（可变：设置页写入）
    pub fn settings_mut(&mut self) -> &mut SettingsManager {
        &mut self.settings
    }

    /// F2 新手教程
    pub fn tutorial(&self) -> &OnboardingTutorial {
        &self.tutorial
    }

    /// F3 方案管理（只读）
    pub fn projects(&self) -> &ProjectManager {
        &self.projects
    }

    /// F3 方案管理（可变：CRUD 与借出 B2 句柄给 F2/F5/F7 落库）
    pub fn projects_mut(&mut self) -> &mut ProjectManager {
        &mut self.projects
    }

    /// F4 采集流水线
    pub fn acquisition(&self) -> &AcquisitionPipeline {
        &self.acquisition
    }

    /// F5 当前评审会话（未进台时为 `None`）
    pub fn review(&self) -> Option<&ReviewWorkbench> {
        self.review.as_ref()
    }

    /// F7 安静哨兵
    pub fn sentinel(&self) -> &QuietSentinel {
        &self.sentinel
    }

    /// F9 导出控制台
    pub fn export(&self) -> &ExportConsole<MockSealGate> {
        &self.export
    }
}

#[cfg(test)]
mod tests {
    use onboarding_tutorial::TutorialStatus;
    use shared_domain_types::CampusId;

    use super::*;

    fn injector() -> ViewModelInjector {
        let db = ShellDatabases::open_in_memory().expect("内存库连接组");
        ViewModelInjector::new(db).expect("构造注入器")
    }

    #[test]
    fn test_injector_holds_all_vms() {
        let mut injector = injector();

        // F1：全新库 → 首次运行
        assert!(injector.settings().is_first_run().expect("读首次运行标记"));
        // F2：引导尚未开始
        assert_eq!(injector.tutorial().status(), TutorialStatus::NotStarted);
        // F3：可建校区与方案
        let campus = injector
            .projects_mut()
            .create_campus("演示大学")
            .expect("建校区");
        let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "第一个方案")
            .expect("建方案");
        // F4：归类引擎就绪
        let _engine = injector.acquisition().engine();
        // F5：进台会话装载成功（零候选照样成台）
        injector.enter_review(&plan_id).expect("评审进台");
        let workbench = injector.review().expect("评审会话已持有");
        assert_eq!(workbench.candidate_count(), 0);
        // F7 / F9：实例可达
        let _sentinel = injector.sentinel();
        let _export = injector.export();
    }

    #[test]
    fn test_vm_state_injected_to_slint() {
        let injector = injector();
        let window = AppWindow::new().expect("创建 Slint 窗口");
        injector.inject(&window);

        let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
        let title = l10n.t("app.welcome_title");
        assert_eq!(window.get_app_title().as_str(), title.as_str());
        // 全新库 → 首次运行状态文案
        let status = l10n.t("app.shell_status_first_run");
        assert_eq!(window.get_status_text().as_str(), status.as_str());
    }
}
