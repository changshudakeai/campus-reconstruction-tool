//! 带类型错误（窗口契约章：错误是带类型的值一路向上传递）

/// F7 覆盖率审计错误
///
/// 体检本身是纯内存数数、不会失败；错误只可能出在裁决记忆的读写上。
/// 分派级别由调用方按弹窗铁律决定（存储失败属"卡住流程"级 → 弹窗）。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// B2 存储读写失败
    #[error("裁决记忆读写失败: {0}")]
    Storage(#[from] data_persistence::Error),
    /// 裁决记忆 JSON 损坏（app_settings 中的值无法解析）
    #[error("裁决记忆数据损坏: {0}")]
    Corrupted(#[from] serde_json::Error),
}

/// F7 统一 Result 别名
pub type Result<T> = std::result::Result<T, Error>;
