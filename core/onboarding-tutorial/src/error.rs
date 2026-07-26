//! 带类型错误（窗口契约章：错误是带类型的值一路向上传递）

/// F2 新手教程错误
///
/// 引导编排本身是纯内存判定、不会失败；错误只可能出在引导进度的
/// 持久化读写上。引导不阻挡操作：存储失败时最坏情况是气泡状态未
/// 记住，不影响任何业务功能（ADR-0020 后果条）。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// B2 存储读写失败
    #[error("引导进度读写失败: {0}")]
    Storage(#[from] data_persistence::Error),
    /// 引导进度 JSON 损坏（app_settings 中的值无法解析）
    #[error("引导进度数据损坏: {0}")]
    Corrupted(#[from] serde_json::Error),
}

/// F2 统一 Result 别名
pub type Result<T> = std::result::Result<T, Error>;
