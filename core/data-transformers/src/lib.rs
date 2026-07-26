//! B13 数据转换器：原始对象 → 已归类候选（标签映射表 + 六类别归类）。
//!
//! 归类完全由数据源标签驱动（ADR-0011）：集中定义的标签映射表决定归属；
//! 多标签冲突按 建筑 > 体育 > 水域 > 道路 > 植被 > 其他 取最高优先级
//! （优先级定义在 B1 `CandidateCategory::priority()`）；映射不上归"其他"
//! 并带明确兜底信号，禁止静默丢弃。
//!
//! ## 模块组织
//!
//! - [config](crate::config)：映射表数据结构与内嵌默认表（集中配置的唯一一处）
//! - [engine](crate::engine)：`ClassifyEngine` 归类引擎（纯内存查表）
//! - [validator](crate::validator)：`TagMappingValidator` 映射表校验
//! - [error](crate::error)：带类型错误
//!
//! ## 依赖边界（ADR-0017）
//!
//! 仅依赖 B1（shared-domain-types）；不依赖 B12 数据源适配器——
//! F4 负责把 B12 的原始对象标签递进来（窗口契约缝 3）。

#![cfg_attr(not(test), warn(unreachable_pub))]

pub mod config;
pub mod engine;
pub mod error;
pub mod validator;

// 重新导出公共类型，方便 crate 外使用
pub use config::{ClassifyConfig, TagPattern, TagRuleEntry};
pub use engine::{Classification, ClassifyEngine, TagMap};
pub use error::TransformError;
pub use validator::TagMappingValidator;
