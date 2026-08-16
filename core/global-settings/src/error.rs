//! F1 带类型错误
//!
//! 窗口契约章：错误是带类型的值一路向上传递，由壳层决定呈现方式
//!（通知中心三级分派）。文案暂用中文硬编码，待接入 T03 文本键。

/// F1 统一结果类型
pub type Result<T> = std::result::Result<T, Error>;

/// F1 应用全局设置错误
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// 底层存储错误（B2 透传）
    #[error("存储错误：{0}")]
    Storage(#[from] data_persistence::Error),

    /// 不支持的语言选项（ADR-0004：当前仅中文）
    #[error("不支持的语言：{0}")]
    UnsupportedLanguage(String),

    /// 不支持的 Minecraft 版本（ADR-0004：当前仅 26.1.2）
    #[error("不支持的 Minecraft 版本：{0}")]
    UnsupportedMinecraftVersion(String),

    /// 首次设置未勾选知情告知（ADR-0004：设置页兼任知情告知）
    #[error("请先确认已阅读版本提示")]
    NoticeNotAcknowledged,

    /// 高德必填配置缺失（ADR-0004：JS API Key 与安全密钥必填，明确指出缺失项）
    #[error("缺少必填的高德配置：{0}")]
    MissingGaodeKeys(String),

    /// 高德 API key 格式无效（T22）
    #[error("高德 API key 格式无效，只能包含字母或数字")]
    InvalidGaodeApiKey,

    /// 高德安全密钥格式无效（T22）
    #[error("高德安全密钥格式无效，只能包含字母或数字")]
    InvalidGaodeSecurityKey,

    /// 高德地图连通性测试失败（T22）
    #[error("高德地图连通性测试失败：{0}")]
    GaodeConnectionFailed(String),

    /// 默认导出位置为空（ADR-0004：必须给出可用的文件夹路径）
    #[error("默认导出位置不能为空")]
    InvalidExportLocation,

    /// T05：校区 ID 解析失败
    #[error("校区 ID 解析失败：{0}")]
    InvalidCampusId(String),
    /// 最近使用校区列表的 JSON 编解码失败（ADR-0006 持久化格式损坏）
    #[error("最近使用校区列表数据损坏：{0}")]
    InvalidRecentCampuses(String),
}
