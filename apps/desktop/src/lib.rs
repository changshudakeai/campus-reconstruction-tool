// 主程序应用壳（S1）- Library crate for VM integration
//
// 薄壳原则：零业务逻辑，只负责集成各功能模块的 ViewModel 并分发给 Slint UI
//! # 架构
//!
//! - **ViewModel 集成层**: `lib.rs` 整合所有 F1-F9 功能模块的视图状态
//! - **事件分发**: 用户操作通过回调函数路由到对应功能模块
//! - **零业务逻辑**: 所有业务规则留在各自功能模块内

#![cfg_attr(not(test), warn(unreachable_pub))]

// ===== 外部依赖导入 =====

// B6 国际化（全局单例）
pub use localization::Localization;

// B1 共享领域类型（只读访问例外，ADR-0025）
pub use shared_domain_types::{CandidateCategory, ReviewState};

// ===== F1-F9 功能模块 ViewModel =====

// F1 全局设置
pub use global_settings::{FirstRunSetup, GlobalSettings, LandingCampus};

// F2 新手教程
pub use onboarding_tutorial::{OnboardingTutorial, TutorialBubble, TutorialStep};

// F3 方案管理
pub use project_management::{ProjectManager, CampusView, PlanCardView};

// F4 数据采集
pub use data_acquisition::{AcquisitionPipeline, CollectionProgressView};

// F5 评审台
pub use review_workbench::{WorkbenchView, CandidateCardView, MapObjectView, InfoPanelView};

// F7 覆盖率审计
pub use coverage_audit::{AuditReportView, AuditReportEntry};

// F9 导出控制台
pub use export_console::{
    ExportConfirmDialogView, ExportProgressView, NavigationTarget, ExportRequest,
};

// ===== 基础模块 =====

// B2 数据持久化
pub use data_persistence::{AppSettingKey, AppSettingsApi, Database};

// ===== 运行时模块 =====

mod runtime;
pub use runtime::{AppShell, CurrentView, run_dev, run_service};

// ===== UI 框架 =====

// Slint UI 代码生成后的模块
include!(concat!(env!("OUT_DIR"), "/main_slint.rs"));
