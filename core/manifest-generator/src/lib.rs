//! MCRebuild V2 —— B17 Manifest 生成器与用料表配置
//!
//! 职责：导出时自动生成 `foundation_manifest.json`，如实记录本次包含/缺失哪些类别；
//! 用料表集中配置且与 MC 版本强绑定（只准用目标版本存在的方块）。
//!
//! 依据 ADR-0012（可选采集 + manifest）+ ADR-0024（用料表按版本核对）。
//!
//! # 模块结构
//!
//! - `manifest`: `foundation_manifest.json` 数据结构定义
//! - `materials`: 用料表配置结构（按 MC 版本区分）
//! - `generator`: manifest 生成逻辑
//! - `validator`: 用料表验证（方块是否存在检查）

pub mod generator;
pub mod manifest;
pub mod materials;
pub mod validator;

// 重新导出常用类型
pub use generator::{GeneratorError, ManifestGenerator, PlanInfo};
pub use manifest::{
    CandidateFacts, CategoryCount, CategoryStatus, ExportKind, FoundationManifest,
    ManifestOrientation, ManifestOrientationSource,
};
pub use materials::{
    BuildingBlocks, BuildingPresets, MaterialTable, MinecraftVersion, ValidationError,
};
pub use validator::MaterialValidator;
