//! 评审会话快照：暂停/恢复（内存状态持久化到临时文件）
//!
//! ADR-0016：可随时暂停、保存项目、退出再回来继续。评审期间零写库
//! （缝 4 契约），所以暂停进度不进 SQLite，而是序列化成 JSON 临时文件；
//! 恢复时按候选标识逐条对回内存，文件里多出来的条目（候选集已变化）
//! 安静跳过——已发布候选投影才是本次评审的事实来源。

use serde::{Deserialize, Serialize};
use shared_domain_types::{CandidateCategory, ReviewState};
use std::path::Path;

use crate::error::{Error, Result};

/// 当前写出版本；读取器同时兼容仍按稳定 candidate_id 对回的 v2。
const SNAPSHOT_FORMAT_VERSION: u32 = 3;
const COMPATIBLE_SNAPSHOT_FORMAT_VERSION: u32 = 2;

/// 快照里的一条候选状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionEntry {
    /// B2 候选投影的稳定 ID
    pub(crate) candidate_id: String,
    /// 三态标识符（B1 `ReviewState::to_identifier` 的取值）
    pub(crate) state: String,
    /// 复选框勾选状态
    pub(crate) selected: bool,
}

/// 评审会话快照（临时文件的 JSON 根）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionSnapshot {
    /// 文件格式版本
    pub(crate) version: u32,
    /// 所属方案 ID（恢复时防串档）
    pub(crate) plan_id: String,
    /// 当前激活的类别抽屉
    pub(crate) active_category: CandidateCategory,
    /// 全部候选的状态与勾选
    pub(crate) entries: Vec<SessionEntry>,
}

impl SessionSnapshot {
    /// 创建快照（版本号自动填当前格式版本）
    pub(crate) fn new(
        plan_id: String,
        active_category: CandidateCategory,
        entries: Vec<SessionEntry>,
    ) -> Self {
        Self {
            version: SNAPSHOT_FORMAT_VERSION,
            plan_id,
            active_category,
            entries,
        }
    }

    /// 写入临时文件（覆盖既有内容）
    pub(crate) fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|err| Error::SessionCorrupt(err.to_string()))?;
        std::fs::write(path, json).map_err(|err| Error::SessionIo(err.to_string()))
    }

    /// 从临时文件读回；v2 中多余的 category 身份字段由 Serde 忽略。
    pub(crate) fn load_from_file(path: &Path) -> Result<Self> {
        let json =
            std::fs::read_to_string(path).map_err(|err| Error::SessionIo(err.to_string()))?;
        let snapshot: Self =
            serde_json::from_str(&json).map_err(|err| Error::SessionCorrupt(err.to_string()))?;
        if !matches!(
            snapshot.version,
            COMPATIBLE_SNAPSHOT_FORMAT_VERSION | SNAPSHOT_FORMAT_VERSION
        ) {
            return Err(Error::SessionCorrupt(format!(
                "不支持的会话文件版本 {}（当前 {SNAPSHOT_FORMAT_VERSION}）",
                snapshot.version
            )));
        }
        Ok(snapshot)
    }

    /// 解析一条快照里的三态标识符
    pub(crate) fn parse_state(entry: &SessionEntry) -> Result<ReviewState> {
        ReviewState::parse(&entry.state)
            .ok_or_else(|| Error::SessionCorrupt(format!("未知三态标识符：{}", entry.state)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrips_through_json() {
        let snapshot = SessionSnapshot::new(
            "plan-1".to_owned(),
            CandidateCategory::Sports,
            vec![SessionEntry {
                candidate_id: "overpass:way/1:outer".to_owned(),
                state: ReviewState::Keep.to_identifier().to_owned(),
                selected: true,
            }],
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: SessionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.plan_id, "plan-1");
        assert_eq!(restored.active_category, CandidateCategory::Sports);
        assert_eq!(
            SessionSnapshot::parse_state(&restored.entries[0]).unwrap(),
            ReviewState::Keep
        );
        assert!(restored.entries[0].selected);
    }

    #[test]
    fn unknown_state_identifier_is_rejected() {
        let entry = SessionEntry {
            candidate_id: "overpass:way/2:outer".to_owned(),
            state: "definitely-not-a-state".to_owned(),
            selected: false,
        };
        assert!(matches!(
            SessionSnapshot::parse_state(&entry),
            Err(Error::SessionCorrupt(_))
        ));
    }
}
