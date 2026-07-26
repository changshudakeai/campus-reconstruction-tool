//! F5 带类型错误（窗口契约章：错误是带类型的值一路向上传递）
//!
//! 用户可见文案由 UI 层按文本键解析（如批量写回失败属"卡住流程"级，
//! 对应弹窗文案由通知中心按 ADR-0021 处理），本文件的错误消息仅供开发者诊断。

use thiserror::Error;

/// F5 评审工作台错误
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// B2 持久化层错误（封账批量写回失败时向上传递；封账不生效，评审可继续改）
    #[error("数据持久化错误：{0}")]
    Persistence(#[from] data_persistence::Error),

    /// 已封账：评审决定不可再改（想调整只能重新采集、重新评审）
    #[error("评审已封账，状态不可再改")]
    AlreadySealed,

    /// 状态变更操作指向了不在候选集里的对象（数据不一致信号）
    #[error("找不到候选对象：{0}")]
    CandidateNotFound(String),

    /// 没有等待确认的批量操作（confirm/cancel 被凭空调用）
    #[error("当前没有等待二次确认的批量操作")]
    NoPendingConfirmation,

    /// 会话临时文件读写失败（暂停/恢复）
    #[error("评审会话文件读写失败：{0}")]
    SessionIo(String),

    /// 会话临时文件内容损坏或格式不符
    #[error("评审会话文件损坏：{0}")]
    SessionCorrupt(String),

    /// 会话临时文件属于另一个方案（防串档）
    #[error("评审会话文件属于方案 {found}，当前方案是 {expected}")]
    SessionPlanMismatch {
        /// 当前评审台的方案 ID
        expected: String,
        /// 会话文件里记录的方案 ID
        found: String,
    },
}

/// F5 结果别名
pub type Result<T> = std::result::Result<T, Error>;
