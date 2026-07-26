//! F3 带类型错误（窗口契约章：错误是带类型的值一路向上传递）
//!
//! 用户可见文案由 UI 层按文本键解析（如同名冲突对应 `plan.duplicate_name`），
//! 本文件的错误消息仅供开发者诊断。

use thiserror::Error;

/// F3 方案管理错误
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// B2 持久化层错误（含同名冲突 `DuplicatePlanName`，UI 层据此弹
    /// `plan.duplicate_name` 文案）
    #[error("数据持久化错误：{0}")]
    Persistence(#[from] data_persistence::Error),

    /// 找不到方案
    #[error("找不到方案：{0}")]
    PlanNotFound(String),

    /// 找不到回收站条目（或不在指定校区的回收站中）
    #[error("找不到回收站条目：{0}")]
    TrashItemNotFound(String),

    /// 恢复冲突：校区内已有同名方案，须先处理再恢复
    #[error("恢复冲突：校区内已存在同名方案 '{0}'")]
    RestoreNameConflict(String),

    /// 复制方案时"副本"序号用尽（同名副本过多）
    #[error("复制方案失败：'{0}' 的副本序号已用尽")]
    DuplicateNameExhausted(String),

    /// B2 返回的 ID 不是合法 UUID（数据损坏信号）
    #[error("非法 ID：{0}")]
    InvalidId(String),
}

/// F3 结果别名
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// 是否为同校区方案名冲突（UI 层据此显示 `plan.duplicate_name` 文案）
    pub fn is_duplicate_name(&self) -> bool {
        matches!(
            self,
            Error::Persistence(data_persistence::Error::DuplicatePlanName(_))
        )
    }
}
