//! T19 — S1 主程序应用壳。
//!
//! 薄壳原则（ADR-0017）：本 crate 零业务逻辑，只做四件事——
//! 1. 引入 `ui/main.slint` 生成的窗口绑定（`slint::include_modules!`）；
//! 2. 判定首开着陆去向（首次向导 / 老用户直达 / 校区选择），
//!    判定本身委托 F1 global-settings，壳只消费结果；
//! 3. 经 B6 l10n 注入全部文案（文本外置铁律 ADR-0005）；
//! 4. T19B-1 起：[`ViewModelInjector`] 构造并持有全部 F 模块实例，
//!    把视图状态注入 Slint 属性；回调错误统一经
//!    [`report_callback_error`] 分派 B7（弹窗铁律 ADR-0021）；
//! 5. T19B-2 起：[`ShellPresenter`] 实现 B7 Presenter（错误模态遮罩），
//!    首跑向导/设置页回调经 [`ViewModelInjector::bind`] 接线。
//!
//! 页面级接线与导航骨架（ADR-0027）归 T19B-2..8，见 `.scratch/v2-implementation/issues/`。

mod dispatch;
mod injector;
mod presenter;
mod runtime;

pub use dispatch::report_callback_error;
pub use injector::{ShellDatabases, ViewModelInjector};
pub use presenter::ShellPresenter;
pub use runtime::{landing_decision, run_dev, LandingDecision};

// ui/main.slint 生成的 AppWindow 绑定（build.rs 产出）。生成代码的可见性
// 与属性由 Slint 生成器决定，不受本库 lint 约束，故在模块内豁免。
mod generated {
    #![allow(
        unreachable_pub,
        clippy::allow_attributes_without_reason,
        clippy::todo,
        reason = "Slint 生成代码，可见性与豁免标记由生成器决定，非手写代码；\
                  std-widgets 组件的生成代码内含 todo! 占位分支"
    )]

    slint::include_modules!();
}

pub use generated::AppWindow;
