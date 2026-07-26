//! B3 带类型错误
//!
//! 文案暂用中文硬编码，待接入 T03 文本键。

/// B3 统一结果类型
pub type Result<T> = std::result::Result<T, Error>;

/// B3 高德地图客户端错误
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// 高德响应不是合法 JSON
    #[error("高德响应解析失败：{0}")]
    MalformedResponse(String),

    /// 高德服务返回业务失败（status != "1"）
    #[error("高德服务返回失败：{info}")]
    ServiceRejected {
        /// 高德返回的错误说明（info 字段原文）
        info: String,
    },

    /// 确认流程状态不允许该操作（如未看详情就确认）
    #[error("当前步骤不允许该操作：{0}")]
    InvalidFlowStep(String),

    /// 候选下标越界
    #[error("无效的候选序号：{0}")]
    CandidateOutOfRange(usize),
}
