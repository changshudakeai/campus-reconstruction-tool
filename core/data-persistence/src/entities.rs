//! 持久化领域实体
//!
//! 三张表的行模型：原始观测（数据粮仓）、评审终态、回收站条目。
//! 类别与三态复用 B1 共享领域类型，本文件只补数据库编解码辅助。

use chrono::{DateTime, SecondsFormat, Utc};
use shared_domain_types::{CandidateCategory, ReviewState};
use uuid::Uuid;

use crate::error::{Error, Result};

/// 回收站保留天数（决策记忆：校园级回收站保留 30 天）
pub const TRASH_RETENTION_DAYS: i64 = 30;

/// 原始观测记录（数据粮仓的一行，永不删除）
#[derive(Debug, Clone, PartialEq)]
pub struct RawObservation {
    /// 行 ID（UUID v4 文本）
    pub id: String,
    /// 所属方案 ID
    pub plan_id: String,
    /// 实体类别（六类别之一）
    pub entity_type: CandidateCategory,
    /// 真实世界对象 ID（来自数据源）
    pub entity_id: String,
    /// 原始标签 + 属性（JSON）
    pub source_data: serde_json::Value,
    /// 数据来源标识（如 "gaode" / "overpass"）
    pub data_source_tag: String,
    /// source_data 的 SHA256 内容指纹（增量刷新检测）
    pub digest: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后更新时间
    pub updated_at: DateTime<Utc>,
}

impl RawObservation {
    /// 构建一条新观测：自动生成行 ID、计算 digest、打当前时间戳
    pub fn new(
        plan_id: impl Into<String>,
        entity_type: CandidateCategory,
        entity_id: impl Into<String>,
        source_data: serde_json::Value,
        data_source_tag: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        let digest = Self::compute_digest(&source_data);
        Self {
            id: Uuid::new_v4().to_string(),
            plan_id: plan_id.into(),
            entity_type,
            entity_id: entity_id.into(),
            source_data,
            data_source_tag: data_source_tag.into(),
            digest,
            created_at: now,
            updated_at: now,
        }
    }

    /// 计算 source_data 的 SHA256 内容指纹（小写十六进制）
    pub fn compute_digest(source_data: &serde_json::Value) -> String {
        use sha2::{Digest, Sha256};
        let canonical = source_data.to_string();
        let hash = Sha256::digest(canonical.as_bytes());
        let mut hex = String::with_capacity(64);
        for byte in hash {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }
}

/// 评审终态记录（封账时批量写入的一行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDecision {
    /// 所属方案 ID
    pub plan_id: String,
    /// 实体类别（六类别之一）
    pub category: CandidateCategory,
    /// B2 候选投影的稳定 ID。
    pub candidate_id: String,
    /// 评审三态
    pub review_state: ReviewState,
    /// 评审人（当前版本填 "system" 或留空）
    pub reviewer_id: Option<String>,
    /// 最后修改时间
    pub updated_at: DateTime<Utc>,
}

impl ReviewDecision {
    /// 构建一条评审终态，时间戳取当前时刻
    pub fn new(
        plan_id: impl Into<String>,
        category: CandidateCategory,
        candidate_id: impl Into<String>,
        review_state: ReviewState,
    ) -> Self {
        Self {
            plan_id: plan_id.into(),
            category,
            candidate_id: candidate_id.into(),
            review_state,
            reviewer_id: None,
            updated_at: Utc::now(),
        }
    }
}

/// 回收站条目（当前仅承载方案删除，entity_type 预留扩展）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    /// 条目 ID（UUID v4 文本）
    pub id: String,
    /// 所属校区 ID（回收站是校园级的）
    pub campus_id: String,
    /// 所属方案 ID
    pub plan_id: String,
    /// 被删除实体类型（当前仅 "plan"）
    pub entity_type: String,
    /// 被删除实体 ID
    pub entity_id: String,
    /// 删除时间
    pub deleted_at: DateTime<Utc>,
    /// 删除人（可选）
    pub deleted_by: Option<String>,
    /// 恢复时间（None = 未恢复）
    pub restored_at: Option<DateTime<Utc>>,
    /// 永久删除时间（None = 未永久删除）
    pub permanently_deleted_at: Option<DateTime<Utc>>,
}

impl TrashItem {
    /// 构建一条方案删除的回收站条目
    pub fn new_plan(
        campus_id: impl Into<String>,
        plan_id: impl Into<String>,
        deleted_by: Option<String>,
    ) -> Self {
        let plan_id = plan_id.into();
        Self {
            id: Uuid::new_v4().to_string(),
            campus_id: campus_id.into(),
            entity_type: "plan".to_owned(),
            entity_id: plan_id.clone(),
            plan_id,
            deleted_at: Utc::now(),
            deleted_by,
            restored_at: None,
            permanently_deleted_at: None,
        }
    }

    /// 是否仍在回收站中可恢复（未恢复、未永久删除、未过保留期）
    pub fn is_restorable(&self, now: DateTime<Utc>) -> bool {
        self.restored_at.is_none()
            && self.permanently_deleted_at.is_none()
            && (now - self.deleted_at).num_days() < TRASH_RETENTION_DAYS
    }
}

/// 类别 → 数据库文本（与迁移脚本 CHECK 约束一致）
pub(crate) fn category_to_db(category: CandidateCategory) -> Result<&'static str> {
    match category {
        CandidateCategory::Building => Ok("Building"),
        CandidateCategory::Road => Ok("Road"),
        CandidateCategory::Water => Ok("Water"),
        CandidateCategory::Vegetation => Ok("Vegetation"),
        CandidateCategory::Sports => Ok("Sports"),
        CandidateCategory::Other => Ok("Other"),
        // B1 枚举带 #[non_exhaustive]：新增类别必须同步扩迁移脚本 CHECK 约束
        _ => Err(Error::InvalidCategory(format!("{category:?}"))),
    }
}

/// 数据库文本 → 类别
pub(crate) fn category_from_db(text: &str) -> Result<CandidateCategory> {
    match text {
        "Building" => Ok(CandidateCategory::Building),
        "Road" => Ok(CandidateCategory::Road),
        "Water" => Ok(CandidateCategory::Water),
        "Vegetation" => Ok(CandidateCategory::Vegetation),
        "Sports" => Ok(CandidateCategory::Sports),
        "Other" => Ok(CandidateCategory::Other),
        other => Err(Error::InvalidCategory(other.to_owned())),
    }
}

/// 三态 → 数据库文本（与 CHECK 约束一致，复用 B1 的标识符）
pub(crate) fn review_state_to_db(state: ReviewState) -> &'static str {
    state.to_identifier()
}

/// 数据库文本 → 三态
pub(crate) fn review_state_from_db(text: &str) -> Result<ReviewState> {
    ReviewState::parse(text).ok_or_else(|| Error::InvalidReviewState(text.to_owned()))
}

/// 时间戳 → 数据库 RFC3339 文本
pub(crate) fn timestamp_to_db(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// 数据库 RFC3339 文本 → 时间戳
pub(crate) fn timestamp_from_db(text: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|err| Error::InvalidTimestamp(format!("{text}: {err}")))
}
