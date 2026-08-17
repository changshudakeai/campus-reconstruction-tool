//! 持久化错误类型
//!
//! 窗口契约章规矩：错误是带类型的值一路向上传递，最终由功能模块按
//! 弹窗铁律三级分派（底层 IO 失败属"卡住流程"级，由调用方弹窗）。

use thiserror::Error;

/// B2 持久化层错误
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// SQLite 层错误（连接、语句、事务）
    #[error("数据库错误：{0}")]
    Database(#[from] rusqlite::Error),

    /// 迁移执行失败
    #[error("迁移失败（版本 {version}）：{message}")]
    MigrationFailed {
        /// 出错的迁移版本号
        version: u32,
        /// 失败原因
        message: String,
    },

    /// source_data 等 JSON 字段编解码失败
    #[error("序列化错误：{0}")]
    Serialization(#[from] serde_json::Error),

    /// 数据库中的类别名不在六类别之内
    #[error("类别解析错误：期望 Building/Road/Water/Vegetation/Sports/Other，实际为 '{0}'")]
    InvalidCategory(String),

    /// 数据库中的评审状态不在三态之内
    #[error("状态解析错误：期望 pending/keep/remove，实际为 '{0}'")]
    InvalidReviewState(String),

    /// 时间戳不是合法 RFC3339 文本
    #[error("时间戳解析错误：{0}")]
    InvalidTimestamp(String),

    /// 回收站条目不存在或状态不允许该操作（如恢复已永久删除的条目）
    #[error("回收站操作被拒：{0}")]
    TrashOperationRejected(String),

    /// 校区不存在
    #[error("校区不存在：{0}")]
    CampusNotFound(String),

    /// 计划不存在
    #[error("计划不存在：{0}")]
    PlanNotFound(String),

    /// 同校区内计划名重复
    #[error("同名计划冲突：{0}")]
    DuplicatePlanName(String),

    /// 候选批次不在允许状态，不能写入或发布。
    #[error("候选批次操作被拒：{0}")]
    CandidateBatchRejected(String),

    /// 评审页打开后候选批次已经变化，旧页面不得把旧决定写回新生命周期。
    #[error("候选投影版本已变化：期望 {expected}，当前 {actual}")]
    StaleCandidateProjectionRevision { expected: String, actual: String },
}

/// B2 持久化层结果别名
pub type Result<T> = std::result::Result<T, Error>;
