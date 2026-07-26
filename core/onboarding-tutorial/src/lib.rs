//! F2 新手教程 —— 跟练式气泡引导（ADR-0020）
//!
//! 不设单独的教程页：用户直接开始建第一个真实方案，软件在真实操作里
//! 当场指路——气泡是任务链的旁白，不是独立课程。教程做完，用户手里
//! 已经有第一个真实作品。
//!
//! ## 四条规矩（ADR-0020 第二节）
//!
//! - ① **每泡可关**：气泡上有"知道了"，点击即消失（[`OnboardingTutorial::dismiss`]）；
//! - ② **一键全跳**：第一个气泡附"跳过全部引导"，选择后所有气泡永不再出现
//!   （[`OnboardingTutorial::skip_all`]，经 B7 info 级留底"设置里可重看"）；
//! - ③ **只教一次**：每个提示点只在第一次遇到时出现，建第二个方案时全程安静
//!   （引导进度为应用级持久化，不分方案）；
//! - ④ **可以重看**：设置中"重新查看教程"，跳过可逆（[`OnboardingTutorial::restart`]）。
//!
//! ## 实施顺序约束（ADR-0020 第三节）
//!
//! 气泡的具体位置与文案在界面成型后的开发版审核时敲定——本阶段
//! [`BubblePlacement`] 只是占位值，提示点清单只预留
//! 三个里程碑钩子（首进方案列表 / 采集完成 / 导出完成），扩容归 T19。
//!
//! ## 模块组织
//!
//! - [model](crate::model)：里程碑提示点与教程状态机
//! - [progress](crate::progress)：引导进度的应用级持久化（B2 app_settings）
//! - [tutorial](crate::tutorial)：气泡编排与四条规矩 + 设置页入口
//! - [error](crate::error)：带类型错误
//!
//! ## 架构边界（ADR-0017）
//!
//! F2 是功能模块：不依赖任何其他 F* 模块（气泡与各页面的接线由壳负责，
//! 本 crate 只按提示点产出纯数据 ViewModel）；内部依赖 B2/B6/B7，存储只经
//! B2 公开 trait，留底提示只经 B7 分派，本 crate 零 slint、零 SQL。
//! 引导不碰业务数据、不阻挡操作——最坏情况是气泡未出现，不影响任何功能。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod error;
mod model;
mod progress;
mod tutorial;

pub use error::{Error, Result};
pub use model::{TutorialStatus, TutorialStep, ALL_STEPS};
pub use progress::TutorialProgress;
pub use tutorial::{BubblePlacement, OnboardingTutorial, SettingsEntryView, TutorialBubble};
