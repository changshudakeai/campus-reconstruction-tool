//! A1 采集输入快照（Start 时冻结的不可变输入，export-flow 模板）。
//!
//! 输入由 S1 转交的用户动作喂入：打开方案（[`CollectionFlow::set_plan`]）、
//! 确认边界（[`CollectionFlow::confirm_boundary`]）。A1 在 [`CollectionFlow::start`]
//! 返回前冻结完整请求，后台 worker 只持有这份不可变值——Start 之后的重置
//! 或切换方案不得改变本次采集的输入（M1 验收已确认该模板）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use shared_domain_types::{Boundary, PlanId};

/// Start 时冻结的完整采集输入：方案 + 已确认的方案边界。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CollectionInput {
    /// 采集所属方案。
    pub plan_id: PlanId,
    /// 用户已确认的方案边界（候选采集的查询范围）。
    pub boundary: Boundary,
}

/// 单个方案的输入快照（切换方案后恢复该方案最新已确认输入）。
#[derive(Debug, Clone, Default)]
pub(crate) struct PlanInputSnapshot {
    pub(crate) boundary: Option<Boundary>,
    pub(crate) boundary_confirmed: bool,
}

/// A1 私有输入仓：只表达"方案 + 已确认边界"，不持有任何采集中间数据。
#[derive(Debug, Clone, Default)]
pub(crate) struct CollectionInputStore {
    inner: Arc<Mutex<InputStoreInner>>,
}

#[derive(Debug, Default)]
struct InputStoreInner {
    active_plan_id: Option<String>,
    plans: HashMap<String, PlanInputSnapshot>,
}

impl CollectionInputStore {
    /// 打开方案：切换活动方案并恢复该方案输入快照（export-flow 模板）。
    pub(crate) fn set_plan(&self, plan_id: &PlanId) {
        let mut inner = self.inner.lock().expect("collection input store lock");
        inner.active_plan_id = Some(plan_id.to_string());
        inner.plans.entry(plan_id.to_string()).or_default();
    }

    /// 确认边界：写入活动方案快照（与 F9 共用同一份用户确认事实）。
    pub(crate) fn confirm_boundary(&self, boundary: Boundary) {
        let mut inner = self.inner.lock().expect("collection input store lock");
        let Some(plan_id) = inner.active_plan_id.clone() else {
            return;
        };
        let snapshot = inner.plans.entry(plan_id).or_default();
        snapshot.boundary = Some(boundary);
        snapshot.boundary_confirmed = true;
    }

    /// 重置边界（用户重置圈画后采集输入同步失效）。
    pub(crate) fn reset_boundary(&self) {
        let mut inner = self.inner.lock().expect("collection input store lock");
        let Some(plan_id) = inner.active_plan_id.clone() else {
            return;
        };
        if let Some(snapshot) = inner.plans.get_mut(&plan_id) {
            snapshot.boundary = None;
            snapshot.boundary_confirmed = false;
        }
    }

    /// 冻结完整采集输入；活动方案缺失或边界未确认时返回 `None`。
    pub(crate) fn frozen_input(&self) -> Option<CollectionInput> {
        let inner = self.inner.lock().expect("collection input store lock");
        let plan_id = PlanId::parse(inner.active_plan_id.as_ref()?).ok()?;
        let snapshot = inner.plans.get(&plan_id.to_string())?;
        if !snapshot.boundary_confirmed {
            return None;
        }
        Some(CollectionInput {
            plan_id,
            boundary: snapshot.boundary.clone()?,
        })
    }

    /// 当前活动方案（呈现层取页面状态时使用）。
    pub(crate) fn active_plan_id(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("collection input store lock")
            .active_plan_id
            .clone()
    }
}
