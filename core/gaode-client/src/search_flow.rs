//! 校区搜索确认流程状态机
//!
//! ADR-0008 第 3-4 条 + T05 业务规则："校区必须显式确认而非自动进入"。
//!
//! ## 状态流转
//!
//! `Idle` →（输入关键词）`Searching` →（结果到达）`Candidates`
//! →（点选一项）`Detail`（展示详情等待确认）→（点"确认添加"）`Confirmed`
//!
//! 详情页可返回候选列表重选；**任何路径都不会跳过 Detail 直达 Confirmed**。

use shared_domain_types::CampusId;

use crate::error::{Error, Result};
use crate::poi::SchoolPoi;
use crate::record::CampusPoiRecord;

/// 确认后的校区：新分配的校区 ID + 选定的 POI 持久化载荷
///
/// T05 增强：record 中的 longitude/latitude 即为锚点坐标（GCJ-02），可直接用于落地 campuses 表。
/// 调用方（SettingsManager::select_campus_with_anchor）持此载荷创建校区并存档 POI 记录。
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmedCampus {
    /// 新分配的校区 ID（B1 共享类型，T02 复用）
    pub campus_id: CampusId,
    /// 校区 POI 持久化载荷（identity + coordinate lineage，ADR-0008/T05）
    pub record: CampusPoiRecord,
}

/// 搜索确认流程的当前状态（供 UI 渲染判定）
#[derive(Debug, Clone, PartialEq)]
pub enum SearchFlowState {
    /// 空闲：等待用户输入学校名称关键词
    Idle,
    /// 搜索中：已发出搜索，等待结果回传
    Searching {
        /// 用户输入的关键词
        keyword: String,
    },
    /// 候选列表：展示筛选后的学校类候选（名称 + 地址）
    Candidates,
    /// 详情确认：展示选中候选的完整信息，等待用户显式确认
    Detail {
        /// 被选中候选在列表中的下标
        selected: usize,
    },
    /// 已确认：校区建立，流程结束
    Confirmed,
}

/// 校区搜索确认流程 —— 驱动"搜索 → 候选 → 详情 → 显式确认"
#[derive(Debug)]
pub struct CampusSearchFlow {
    state: SearchFlowState,
    candidates: Vec<SchoolPoi>,
}

impl Default for CampusSearchFlow {
    fn default() -> Self {
        Self::new()
    }
}

impl CampusSearchFlow {
    /// 新建一个空闲流程
    pub fn new() -> Self {
        Self {
            state: SearchFlowState::Idle,
            candidates: Vec::new(),
        }
    }

    /// 当前状态（UI 据此决定渲染哪个页面）
    pub fn state(&self) -> &SearchFlowState {
        &self.state
    }

    /// 当前候选列表（只读；候选页与详情页共用）
    pub fn candidates(&self) -> &[SchoolPoi] {
        &self.candidates
    }

    /// 用户提交搜索关键词：进入搜索中（任何状态都可重新搜索）
    ///
    /// 空白关键词被拒绝（避免全量拉取）。
    pub fn start_search(&mut self, keyword: &str) -> Result<()> {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            return Err(Error::InvalidFlowStep("搜索关键词不能为空".to_owned()));
        }
        self.candidates.clear();
        self.state = SearchFlowState::Searching {
            keyword: keyword.to_owned(),
        };
        Ok(())
    }

    /// 搜索结果到达（已由 [`crate::parse_place_search_response`] 筛选去重）
    ///
    /// 空结果也进入候选列表状态——"无结果"页面与失败策略由上层呈现
    ///（ADR-0008 后果条：失败与空结果的处理策略单独决策）。
    pub fn receive_results(&mut self, results: Vec<SchoolPoi>) -> Result<()> {
        if !matches!(self.state, SearchFlowState::Searching { .. }) {
            return Err(Error::InvalidFlowStep(
                "尚未发起搜索，不能接收结果".to_owned(),
            ));
        }
        self.candidates = results;
        self.state = SearchFlowState::Candidates;
        Ok(())
    }

    /// 用户点选候选列表某一项：进入详情确认页（不建立校区）
    pub fn view_detail(&mut self, index: usize) -> Result<&SchoolPoi> {
        if !matches!(
            self.state,
            SearchFlowState::Candidates | SearchFlowState::Detail { .. }
        ) {
            return Err(Error::InvalidFlowStep(
                "只有候选列表页可以查看详情".to_owned(),
            ));
        }
        let poi = self
            .candidates
            .get(index)
            .ok_or(Error::CandidateOutOfRange(index))?;
        self.state = SearchFlowState::Detail { selected: index };
        Ok(poi)
    }

    /// 详情页返回候选列表（重选）
    pub fn back_to_candidates(&mut self) -> Result<()> {
        if !matches!(self.state, SearchFlowState::Detail { .. }) {
            return Err(Error::InvalidFlowStep(
                "只有详情页可以返回候选列表".to_owned(),
            ));
        }
        self.state = SearchFlowState::Candidates;
        Ok(())
    }

    /// 用户在详情页点"确认添加"：建立校区（唯一出口，显式确认）
    ///
    /// 校区名称、坐标锚点均取自高德数据，用户不手动输入校区名（ADR-0008）。
    pub fn confirm(&mut self) -> Result<ConfirmedCampus> {
        let SearchFlowState::Detail { selected } = self.state else {
            return Err(Error::InvalidFlowStep(
                "必须先在详情页核对校区信息才能确认".to_owned(),
            ));
        };
        let poi = self
            .candidates
            .get(selected)
            .ok_or(Error::CandidateOutOfRange(selected))?;
        let confirmed = ConfirmedCampus {
            campus_id: CampusId::generate(),
            record: CampusPoiRecord::from_poi(poi),
        };
        self.state = SearchFlowState::Confirmed;
        Ok(confirmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pois() -> Vec<SchoolPoi> {
        vec![
            SchoolPoi {
                poi_id: "B01".to_owned(),
                name: "华东师范大学(普陀校区)".to_owned(),
                address: "中山北路3663号".to_owned(),
                longitude: 121.406,
                latitude: 31.228,
                typecode: "141201".to_owned(),
            },
            SchoolPoi {
                poi_id: "B02".to_owned(),
                name: "华东师范大学(闵行校区)".to_owned(),
                address: "东川路500号".to_owned(),
                longitude: 121.456,
                latitude: 31.033,
                typecode: "141201".to_owned(),
            },
        ]
    }

    fn flow_at_candidates() -> CampusSearchFlow {
        let mut flow = CampusSearchFlow::new();
        flow.start_search("华东师范大学").unwrap();
        flow.receive_results(sample_pois()).unwrap();
        flow
    }

    #[test]
    fn happy_path_requires_detail_before_confirm() {
        let mut flow = flow_at_candidates();
        let poi = flow.view_detail(1).unwrap();
        assert_eq!(poi.name, "华东师范大学(闵行校区)");

        let confirmed = flow.confirm().unwrap();
        assert_eq!(confirmed.record.name, "华东师范大学(闵行校区)");
        assert_eq!(confirmed.record.gaode_poi_id, "B02");
        assert_eq!(*flow.state(), SearchFlowState::Confirmed);
    }

    #[test]
    fn confirm_without_viewing_detail_is_rejected() {
        let mut flow = flow_at_candidates();
        let err = flow.confirm().unwrap_err();
        assert!(matches!(err, Error::InvalidFlowStep(_)), "不得自动进入校区");
    }

    #[test]
    fn detail_can_go_back_and_reselect() {
        let mut flow = flow_at_candidates();
        flow.view_detail(0).unwrap();
        flow.back_to_candidates().unwrap();
        assert_eq!(*flow.state(), SearchFlowState::Candidates);
        flow.view_detail(1).unwrap();
        assert_eq!(flow.confirm().unwrap().record.gaode_poi_id, "B02");
    }

    #[test]
    fn blank_keyword_is_rejected() {
        let mut flow = CampusSearchFlow::new();
        assert!(matches!(
            flow.start_search("   ").unwrap_err(),
            Error::InvalidFlowStep(_)
        ));
    }

    #[test]
    fn results_before_search_are_rejected() {
        let mut flow = CampusSearchFlow::new();
        assert!(matches!(
            flow.receive_results(sample_pois()).unwrap_err(),
            Error::InvalidFlowStep(_)
        ));
    }

    #[test]
    fn out_of_range_candidate_is_rejected() {
        let mut flow = flow_at_candidates();
        assert!(matches!(
            flow.view_detail(99).unwrap_err(),
            Error::CandidateOutOfRange(99)
        ));
    }

    #[test]
    fn empty_results_still_reach_candidates_state() {
        let mut flow = CampusSearchFlow::new();
        flow.start_search("不存在的学校").unwrap();
        flow.receive_results(Vec::new()).unwrap();
        assert_eq!(*flow.state(), SearchFlowState::Candidates);
        assert!(flow.candidates().is_empty());
    }

    #[test]
    fn restart_search_clears_previous_candidates() {
        let mut flow = flow_at_candidates();
        flow.start_search("另一所学校").unwrap();
        assert!(flow.candidates().is_empty());
        assert!(matches!(flow.state(), SearchFlowState::Searching { .. }));
    }
}
