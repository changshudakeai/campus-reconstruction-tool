//! B14 候选几何校验（ADR-0032）。
//!
//! 本模块只处理采集后、进入候选池前的对象几何；不会检查或修改 B5 的方案边界。
//! 每个对象独立验证：可安全恢复的环被规范化，不能可靠判断的对象被隔离，调用方
//! 仍可将原始观测保存在数据粮仓。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod validator;

pub use validator::{
    CandidateGeometry, GeometryOutcome, GeometryShape, GeometryValidation, GeometryValidator,
    RejectionReason, ValidationDisposition,
};
