//! F7 覆盖率审计 —— 安静哨兵（ADR-0019）
//!
//! 采集后默默体检（纯内存数数，不联网、不写盘），只在有疑点时经 B7
//! 弹一个合并窗口（禁横幅），报告措辞一律问句 —— 系统不冒充认识校园，
//! 裁判永远是用户。
//!
//! 疑点只有两条纯统计规则：
//! - ① 空类别：某类采集过但结果为 0 项（跳过的类别不算，标注"未采集（跳过）"）；
//! - ② "其他"过多："其他"占比超过阈值（默认 20%，实现层可调）。
//!
//! 用户点"知道了"关闭弹窗后记住该裁决，同一批疑点不再主动出现；
//! 仅当该方案重新采集（数据变化，体检重做）时重新评估。
//! 已关闭的疑点在常驻"采集报告"入口内仍可查看。
//!
//! ## 模块组织
//!
//! - [model](crate::model)：疑点数据结构、审计结果、稳定 ID
//! - [audit](crate::audit)：两条疑点规则的检测引擎
//! - [resolver](crate::resolver)：裁决记忆（按方案持久化到 B2 app_settings）
//! - [report](crate::report)：安静哨兵编排 + 弹窗/常驻报告视图
//! - [error](crate::error)：带类型错误
//!
//! ## 架构边界（ADR-0017）
//!
//! F7 是功能模块：不依赖任何其他 F* 模块（与 F4 数据采集的衔接由壳接线，
//! 体检输入是各类别计数的纯数据）；内部依赖 B1/B2/B6/B7，存储只经
//! B2 公开 trait，弹窗只经 B7 分派，本 crate 零 slint、零 SQL。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod audit;
mod error;
mod model;
mod report;
mod resolver;

pub use audit::{CoverageAudit, DEFAULT_OTHER_THRESHOLD};
pub use error::{Error, Result};
pub use model::{AuditIssue, AuditResult, IssueRule, ALL_CATEGORIES};
pub use report::{AuditOutcome, AuditPopupView, AuditReportView, QuietSentinel};
pub use resolver::DecisionResolver;
