//! F9 带类型错误（窗口契约章：错误是带类型的值一路向上传递）
//!
//! 分派结论按弹窗铁律（ADR-0021）落在 F9：导出失败/文件写入失败属
//! "卡住流程"级——B7 `error()` 模态弹窗并回滚封账；本文件的错误消息
//! 仅供开发者诊断，用户可见文案由 UI 层按文本键解析。

use std::path::PathBuf;

use thiserror::Error;

/// 边界直出资格错误；F9 不把缺边界与其他失败混成普通 IO 错误。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BoundaryError {
    /// 没有提供方案边界。
    #[error("没有方案边界，不能导出")]
    Missing,
    /// 边界存在但尚未由用户确认。
    #[error("方案边界尚未确认，不能导出")]
    NotConfirmed,
    /// 边界存在但几何无效。
    #[error("方案边界无效：{0}")]
    Invalid(String),
}

/// 生产导出版本契约失败；F9 不把请求版本静默改成其他版本。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VersionError {
    /// 请求的全局版本不是产品当前支持的版本。
    #[error("不支持的 Minecraft 目标版本：{requested}；当前只支持 26.1.2")]
    Unsupported { requested: String },
    /// F9 内部注入的用料表与目标版本不一致。
    #[error("Minecraft 目标版本 {requested} 与用料表版本 {material_table} 不一致")]
    MaterialTableMismatch {
        requested: String,
        material_table: String,
    },
    /// Sponge profile 与目标版本不一致。
    #[error("Minecraft 目标版本 {requested} 与 Sponge DataVersion {data_version} 不一致")]
    SchematicProfileMismatch {
        requested: String,
        data_version: i32,
    },
    /// 注入的用料配置本身未通过方块 ID 校验。
    #[error("{version} 用料配置校验失败：{detail}")]
    InvalidMaterialTable { version: String, detail: String },
}

/// 双文件发布中的文件类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// Sponge `.schem` 文件。
    Schematic,
    /// B17 manifest 文件。
    Manifest,
}

impl std::fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schematic => f.write_str(".schem"),
            Self::Manifest => f.write_str("manifest"),
        }
    }
}

/// 最终双文件发布失败；失败尚未升级为恢复失败时仍使用此类型。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArtifactWriteError {
    /// 输出目录/目标路径违反受控产物集合约束。
    #[error("导出产物目标无效：{detail}")]
    InvalidTarget { detail: String },
    /// 某一个 staging 文件向最终路径发布失败。
    #[error("发布 {artifact} 失败（{path}）：{detail}")]
    Publish {
        artifact: ArtifactKind,
        path: PathBuf,
        detail: String,
    },
    /// 成功发布后的备份清理失败，必须显式反馈而不是吞掉。
    #[error("清理发布备份失败（{path}）：{detail}")]
    Cleanup { path: PathBuf, detail: String },
}

/// 回滚/恢复动作本身失败；不能伪装成普通 ArtifactWrite。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArtifactRecoveryError {
    /// 原始发布失败后，至少一个恢复动作也失败。
    #[error("导出失败后的恢复未完成：{primary}; 恢复诊断：{recovery}")]
    Failed {
        primary: String,
        recovery: String,
        paths: Vec<PathBuf>,
    },
}
/// F9 导出控制台错误
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// 边界直出资格失败（ADR-0041）。
    #[error("边界导出资格失败：{0}")]
    Boundary(#[from] BoundaryError),

    /// 全局版本、用料表与 Sponge profile 的兼容性失败。
    #[error("导出版本契约失败：{0}")]
    Version(#[from] VersionError),

    /// 状态机被违规驱动（如未加载请求就点确认、未在导出中就报进度）
    #[error("导出控制台状态不允许该操作：{0}")]
    InvalidState(&'static str),

    /// 方案 ID 无法解析（缝 5 递交的请求损坏）
    #[error("方案 ID 无法解析：{0}")]
    BadPlanId(String),

    /// 封账失败（F5 批量写回失败等；封账不生效，评审保持可改）
    #[error("封账失败：{0}")]
    SealFailed(String),
    /// F1 设置读取失败；与缺失设置键触发的合法默认值区分。
    #[error("导出设置读取失败：{0}")]
    SettingsRead(String),

    /// 生成引擎错误（B18；用料表版本不匹配也从这里向上传递）
    #[error("生成引擎错误：{0}")]
    Generation(#[from] generation_engine::GenerationError),

    /// .schem 落盘失败（B4；弹窗并保留重试，缝 6 契约）
    #[error(".schem 落盘失败：{0}")]
    SchematicWrite(String),

    /// manifest 生成或写入失败（B17）。
    #[error("manifest 生成或落盘失败：{0}")]
    ManifestWrite(String),

    /// 双文件最终发布失败；不得把 staging 文件当成成功产物。
    #[error("导出文件发布失败：{0}")]
    ArtifactWrite(#[from] ArtifactWriteError),

    /// 双文件回滚失败；向调用方暴露恢复诊断，禁止声称已安全回滚。
    #[error("导出文件恢复失败：{0}")]
    ArtifactRecovery(#[from] ArtifactRecoveryError),

    /// 后台工作线程意外断开，不能显示成功。
    #[error("导出后台任务异常中止")]
    BackgroundTask,
}

/// F9 结果别名
pub type Result<T> = std::result::Result<T, Error>;
