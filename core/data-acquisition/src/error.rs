//! 带类型错误（窗口契约章：错误是带类型的值一路向上传递）。
//!
//! 错误消息为开发者诊断文本；用户可见文案由 UI 层按 B6 文本键另行处理
//! （ADR-0005）。底层 B2/B3/B13 错误经 `#[from]` 保留类型一路上传，
//! 最终由壳层按弹窗铁律三级分派（采集失败属"卡住流程"级，模态弹窗）。

/// F4 统一结果类型
pub type Result<T> = std::result::Result<T, AcquisitionError>;

/// F4 数据采集错误
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AcquisitionError {
    /// 边界为空：ADR-0012 必经第一步未完成，无从圈定查询范围
    #[error("边界为空：请先在地图上圈画边界再采集")]
    EmptyBoundary,

    /// 已确认方案边界无法安全解析为 Polygon/MultiPolygon。
    #[error("已确认方案边界无法解析，候选采集已停止")]
    InvalidBoundary,

    /// 数据源传输层不可达（网络、桥接失败等）
    #[error("数据源 {source_tag} 不可达：{message}")]
    SourceUnreachable {
        /// 数据源标识（如 "gaode"）
        source_tag: String,
        /// 传输层失败原因
        message: String,
    },

    /// B3 高德客户端解析失败（响应畸形、服务拒绝）
    #[error("数据源响应解析失败：{0}")]
    Source(#[from] gaode_client::Error),

    /// B13 归类引擎错误（映射表校验不过等）
    #[error("归类引擎错误：{0}")]
    Transform(#[from] data_transformers::TransformError),

    /// B2 原始观测落库失败
    #[error("原始观测落库失败：{0}")]
    Persistence(#[from] data_persistence::Error),
}
