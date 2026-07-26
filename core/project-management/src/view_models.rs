//! F3 ViewModel：调用 B2 的 CRUD/回收站接口，产出面向 UI 的纯数据
//!
//! 缝 2 契约的功能层一侧：本文件不写任何 SQL，全部经由 B2 公开 trait。
//! 业务规则落在这里：同名冲突向上抛带类型错误（ADR-0010）、复制方案加
//! "副本"后缀（ADR-0018）、复制边界（ADR-0007）、"上次使用的校区"（ADR-0006）。

use data_persistence::{
    AppSettingKey, AppSettingsApi, CampusCrudApi, Database, PlanCrudApi, TrashApi,
};
use shared_domain_types::{CampusId, PlanId};

use crate::entities::{CampusView, PlanCardView, PlanProgress, TrashItemView};
use crate::error::{Error, Result};

/// "副本"后缀的文本键（文案本身在 zh-CN.json 的 plan.duplicate_suffix，
/// 由 UI 层解析后传入 duplicate_plan；本 crate 不硬编码中文，ADR-0005）
pub const DUPLICATE_SUFFIX_KEY: &str = "plan.duplicate_suffix";

/// 方案管理 ViewModel（持有 B2 数据库句柄）
pub struct ProjectManager {
    db: Database,
}

impl std::fmt::Debug for ProjectManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectManager").finish_non_exhaustive()
    }
}

impl ProjectManager {
    /// 用已打开的 B2 数据库句柄创建
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// 借出内部句柄（供上层做 F3 范围之外的操作）
    pub fn database_mut(&mut self) -> &mut Database {
        &mut self.db
    }

    // ── 校区与着陆（ADR-0006）─────────────────────────────

    /// 列出全部校区
    pub fn list_campuses(&self) -> Result<Vec<CampusView>> {
        let campuses = self.db.list_campuses()?;
        Ok(campuses
            .into_iter()
            .map(|c| CampusView {
                id: c.id,
                name: c.name,
            })
            .collect())
    }

    /// 创建校区并返回视图
    pub fn create_campus(&mut self, name: &str) -> Result<CampusView> {
        let campus = self.db.create_campus(name)?;
        Ok(CampusView {
            id: campus.id,
            name: campus.name,
        })
    }

    /// 老用户着陆：读"上次使用的校区"，校区已被删除则返回 None
    ///（调用方退回校区选择页，ADR-0006）
    pub fn landing_campus(&self) -> Result<Option<CampusView>> {
        let Some(campus_id) = self.db.get_setting(AppSettingKey::LastUsedCampus)? else {
            return Ok(None);
        };
        let Some(campus) = self.db.find_campus_by_id(&campus_id)? else {
            return Ok(None);
        };
        Ok(Some(CampusView {
            id: campus.id,
            name: campus.name,
        }))
    }

    /// 记录"上次使用的校区"（切换校区/选定校区后调用）
    pub fn remember_campus(&mut self, campus_id: &CampusId) -> Result<()> {
        self.db
            .set_setting(AppSettingKey::LastUsedCampus, &campus_id.to_string())?;
        Ok(())
    }

    // ── 方案卡片与 CRUD（ADR-0010/0018）───────────────────

    /// 列出校区的方案卡片（最近修改倒序，由 B2 SQL 保证）
    pub fn list_plan_cards(&self, campus_id: &CampusId) -> Result<Vec<PlanCardView>> {
        let plans = self.db.list_plans(&campus_id.to_string())?;
        Ok(plans
            .into_iter()
            .map(|plan| PlanCardView {
                progress: self.plan_progress(&plan.id),
                plan_id: plan.id,
                name: plan.name,
                last_modified_at: plan.updated_at,
            })
            .collect())
    }

    /// 方案进度描述："已完成 A → 下一步 B"；边界等状态数据尚未接入
    /// （B5 边界存储属 T14 范围），当前一律如实显示"尚未确定范围"（ADR-0010）。
    fn plan_progress(&self, _plan_id: &str) -> PlanProgress {
        PlanProgress::BoundaryNotSet
    }

    /// 轻创建（ADR-0010）：输入方案名即建立方案；同名冲突返回带类型错误
    pub fn create_plan(&mut self, campus_id: &CampusId, name: &str) -> Result<PlanId> {
        let plan = self.db.create_plan(&campus_id.to_string(), name)?;
        PlanId::parse(&plan.id).map_err(|e| Error::InvalidId(e.to_string()))
    }

    /// 改名（ADR-0018 卡片菜单）；同名冲突返回带类型错误
    pub fn rename_plan(&mut self, plan_id: &PlanId, new_name: &str) -> Result<()> {
        self.db.rename_plan(&plan_id.to_string(), new_name)?;
        Ok(())
    }

    /// 复制方案（ADR-0018）：新名 = 原名 + 空格 + 后缀（后缀文案由 UI 层
    /// 解析 `plan.duplicate_suffix` 传入）；同名时追加序号直到不冲突。
    ///
    /// 当前全量复制的范围是方案行本身；边界/朝向/采集/评审数据表由后续
    /// 工单（T14/F4）接入后在各自表上按 plan_id 拷贝。
    pub fn duplicate_plan(&mut self, plan_id: &PlanId, suffix: &str) -> Result<PlanId> {
        let source = self
            .db
            .find_plan_by_id(&plan_id.to_string())?
            .ok_or_else(|| Error::PlanNotFound(plan_id.to_string()))?;

        // "方案 1 副本"、"方案 1 副本 2"、"方案 1 副本 3"…… 上限内找可用名
        const MAX_ATTEMPTS: u32 = 100;
        for attempt in 1..=MAX_ATTEMPTS {
            let candidate = if attempt == 1 {
                format!("{} {}", source.name, suffix)
            } else {
                format!("{} {} {}", source.name, suffix, attempt)
            };
            match self.db.create_plan(&source.campus_id, &candidate) {
                Ok(plan) => {
                    return PlanId::parse(&plan.id).map_err(|e| Error::InvalidId(e.to_string()))
                }
                Err(data_persistence::Error::DuplicatePlanName(_)) => continue,
                Err(other) => return Err(other.into()),
            }
        }
        Err(Error::DuplicateNameExhausted(source.name))
    }

    // ── 回收站（缝 2：进站/恢复查询/到期清理框架/确认后永久删除）──

    /// 删除方案进校园级回收站（保留 30 天，ADR-0018）
    pub fn delete_plan(&mut self, campus_id: &CampusId, plan_id: &PlanId) -> Result<TrashItemView> {
        let item = self
            .db
            .delete_plan_to_trash(&campus_id.to_string(), &plan_id.to_string())?;
        Ok(trash_view(&item))
    }

    /// 列出校区回收站中仍可恢复的方案条目
    pub fn list_trash(&self, campus_id: &CampusId) -> Result<Vec<TrashItemView>> {
        let items = self.db.list_restorable_trash(&campus_id.to_string())?;
        Ok(items
            .iter()
            .filter(|item| item.entity_type == "plan")
            .map(trash_view)
            .collect())
    }

    /// 从回收站恢复方案；若校区内已有同名方案则拒绝并要求先改名
    ///（恢复冲突处理，ADR-0018 后果条）
    pub fn restore_plan(&mut self, campus_id: &CampusId, trash_id: &str) -> Result<TrashItemView> {
        // 预检恢复冲突：同名方案已存在则不动回收站状态
        let campus_key = campus_id.to_string();
        let in_trash = self.db.list_restorable_trash(&campus_key)?;
        let target = in_trash
            .iter()
            .find(|item| item.id == trash_id)
            .ok_or_else(|| Error::TrashItemNotFound(trash_id.to_owned()))?;
        if let Some(plan) = self.db.find_plan_by_id(&target.plan_id)? {
            let live = self.db.list_plans(&campus_key)?;
            if live.iter().any(|p| p.name == plan.name) {
                return Err(Error::RestoreNameConflict(plan.name));
            }
        }
        let restored = self.db.restore_from_trash(trash_id)?;
        Ok(trash_view(&restored))
    }

    /// 确认后立即永久删除（调用方必须先弹确认窗——决策记忆：
    /// 回收站内立即永久删除需确认）
    pub fn purge_plan_confirmed(&mut self, trash_id: &str) -> Result<()> {
        self.db.purge_plan_permanently(trash_id)?;
        Ok(())
    }

    /// 到期清理框架：清掉超过 30 天保留期的条目，返回清理条数。
    /// 调用时机（启动时/定时）由应用壳决定。
    pub fn purge_expired_trash(&mut self) -> Result<usize> {
        Ok(self.db.purge_expired_plans()?)
    }
}

/// B2 回收站条目 → F3 视图
fn trash_view(item: &data_persistence::TrashItem) -> TrashItemView {
    TrashItemView {
        trash_id: item.id.clone(),
        plan_id: item.plan_id.clone(),
        deleted_at: item
            .deleted_at
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    }
}
