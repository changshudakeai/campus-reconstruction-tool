//! A1 采集完整用例的带类型错误。
//!
//! F4/B2/F7 返回结构化错误，A1 在此汇总为 [`CollectionError`] 并决定本次
//! 操作的影响范围；用户可见文案按文本键交给 B6（`zh-CN.json`），B7 只接收
//! 解析后的通知事实，不依赖任何底层 crate 的内部错误类型（ADR-0039）。

use data_persistence::Error as PersistenceError;

/// A1 统一结果类型。
pub type Result<T> = std::result::Result<T, CollectionError>;

/// 候选采集完整用例的错误汇总。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CollectionError {
    /// 没有已确认的方案边界，无法开始候选采集（ADR-0041：边界是唯一前置）。
    #[error("缺少已确认的方案边界，无法开始候选采集")]
    MissingInput,
    /// 已有采集正在进行，不接受新的开始意图（export-flow 的 active 守卫）。
    #[error("已有采集正在进行，请等待完成或取消后再试")]
    Busy,
    /// 边界无法解析为合法多边形（重验证入口的防御性校验）。
    #[error("边界无效，无法重验证候选资格")]
    InvalidBoundary,
    /// F4 采集失败（数据源不可达、响应畸形、落库失败等）。
    #[error("数据采集失败：{0}")]
    Acquisition(#[from] data_acquisition::AcquisitionError),
    /// B2 正式数据保存/候选投影发布失败。
    #[error("正式数据保存失败：{0}")]
    Persistence(#[from] PersistenceError),
    /// F7 覆盖体检失败。
    #[error("覆盖体检失败：{0}")]
    Audit(#[from] coverage_audit::Error),
    /// 采集结果已过期（取消/切换方案后，旧结果不得交付）。
    #[error("采集结果已过期")]
    Expired,
    /// 后台采集任务异常中止。
    #[error("采集后台任务异常中止")]
    BackgroundTask,
}

impl CollectionError {
    /// 用户可见的失败类别文本键（B6；A1 决定影响范围，不吞错）。
    pub fn user_message_key(&self) -> &'static str {
        match self {
            Self::MissingInput => "collection.error_missing_input",
            Self::Busy => "collection.error_busy",
            Self::InvalidBoundary => "collection.error_invalid_boundary",
            Self::Acquisition(data_acquisition::AcquisitionError::SourceUnreachable { .. }) => {
                "error.data_source_unreachable"
            }
            Self::Acquisition(_) => "collection.error_failed",
            Self::Persistence(_) => "collection.error_persistence",
            Self::Audit(_) => "collection.error_audit",
            Self::Expired => "collection.error_expired",
            Self::BackgroundTask => "collection.error_background",
        }
    }
}
