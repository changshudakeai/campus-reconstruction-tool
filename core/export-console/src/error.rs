//! F9 带类型错误（窗口契约章：错误是带类型的值一路向上传递）
//!
//! 分派结论按弹窗铁律（ADR-0021）落在 F9：导出失败/文件写入失败属
//! "卡住流程"级——B7 `error()` 模态弹窗并回滚封账；本文件的错误消息
//! 仅供开发者诊断，用户可见文案由 UI 层按文本键解析。

use thiserror::Error;

/// 边界直出资格错误；F9 不把缺边界与其他失败混成普通 IO 错误。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BoundaryError {
    /// 没有提供方案边界。
    #[error("没有方案边界，不能导出")]
    Missing,
    /// 边界存在但尚未由用户确认。
    #[error("方案边界尚未确认，不能导出")]
    NotConfirmed,
    /// 边界存在但几何无效。
    #[error("方案边界无效：{0}")]
    Invalid(String),
}

/// F9 导出控制台错误
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// 边界直出资格失败（ADR-0041）。
    #[error("边界导出资格失败：{0}")]
    Boundary(#[from] BoundaryError),

    /// 状态机被违规驱动（如未加载请求就点确认、未在导出中就报进度）
    #[error("导出控制台状态不允许该操作：{0}")]
    InvalidState(&'static str),

    /// 方案 ID 无法解析（缝 5 递交的请求损坏）
    #[error("方案 ID 无法解析：{0}")]
    BadPlanId(String),

    /// 封账失败（F5 批量写回失败等；封账不生效，评审保持可改）
    #[error("封账失败：{0}")]
    SealFailed(String),

    /// 生成引擎错误（B18；用料表版本不匹配也从这里向上传递）
    #[error("生成引擎错误：{0}")]
    Generation(#[from] generation_engine::GenerationError),

    /// .schem 落盘失败（B4；弹窗并保留重试，缝 6 契约）
    #[error(".schem 落盘失败：{0}")]
    SchematicWrite(String),

    /// manifest 生成或写入失败（B17）。
    #[error("manifest 生成或落盘失败：{0}")]
    ManifestWrite(String),

    /// 双文件最终发布失败；不得把 staging 文件当成成功产物。
    #[error("导出文件发布失败：{0}")]
    ArtifactWrite(String),
}

/// F9 结果别名
pub type Result<T> = std::result::Result<T, Error>;
