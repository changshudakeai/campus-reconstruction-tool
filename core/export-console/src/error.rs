//! F9 带类型错误（窗口契约章：错误是带类型的值一路向上传递）
//!
//! 分派结论按弹窗铁律（ADR-0021）落在 F9：导出失败/文件写入失败属
//! "卡住流程"级——B7 `error()` 模态弹窗并回滚封账；本文件的错误消息
//! 仅供开发者诊断，用户可见文案由 UI 层按文本键解析。

use thiserror::Error;

/// F9 导出控制台错误
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
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
}

/// F9 结果别名
pub type Result<T> = std::result::Result<T, Error>;
