//! B2 校区/方案 CRUD 与"上次使用的校区"读写
//!
//! 缝 2 契约：F3 方案管理向 B2 要校区/方案的增删改查、回收站进出、
//! "上次使用的校区"读写（app_settings 表）。
//!
//! 删除方案不砸表：方案行保留，仅在回收站登记一条可恢复条目；
//! `list_plans` 自动隐藏在站（未恢复、未永久删除）的方案。
//! 数据粮仓铁律不受影响——本模块不触碰 raw_observations。

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::entities::{timestamp_to_db, TrashItem, TRASH_RETENTION_DAYS};
use crate::error::{Error, Result};
use crate::trash::TrashApi;

/// 校区实体（campuses 表的一行）
#[derive(Debug, Clone, PartialEq)]
pub struct CampusEntity {
    /// 校区 ID（UUID 文本）
    pub id: String,
    /// 校区名称
    pub name: String,
    /// 锚点经度（GCJ-02 坐标系，T05 新增）
    pub anchor_lng: f64,
    /// 锚点纬度（GCJ-02 坐标系，T05 新增）
    pub anchor_lat: f64,
    /// 创建时间（RFC3339 文本）
    pub created_at: String,
    /// 更新时间（RFC3339 文本）
    pub updated_at: String,
}

/// 方案实体（plans 表的一行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntity {
    /// 方案 ID（UUID 文本）
    pub id: String,
    /// 所属校区 ID
    pub campus_id: String,
    /// 方案名称
    pub name: String,
    /// 创建时间（RFC3339 文本）
    pub created_at: String,
    /// 更新时间（RFC3339 文本）
    pub updated_at: String,
}

/// 应用设置键（app_settings 表，ADR-0004/0006/0022）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSettingKey {
    /// 上次使用的校区 ID（ADR-0006 着陆流程）
    LastUsedCampus,
    /// 界面语言
    Language,
    /// Minecraft 版本
    MinecraftVersion,
    /// 首次运行完成标志
    FirstRunCompleted,
    /// 覆盖体检疑点裁决记忆（F7，按方案分组的 JSON，ADR-0019）
    CoverageAuditDecisions,
    /// 新手引导进度（F2，已见提示点 + 是否全跳的 JSON，ADR-0020）
    OnboardingProgress,
    /// 新手引导完成时刻（F2，RFC3339；空字符串表示未完成）
    OnboardingCompletedAt,
    /// 高德地图 API key（T22，经 F1 存储于本机明文）
    GaodeApiKey,
    /// 高德地图安全密钥（T22，经 F1 存储于本机明文）
    GaodeSecurityKey,
}

impl AppSettingKey {
    /// 数据库中的键名
    fn as_db_key(self) -> &'static str {
        match self {
            AppSettingKey::LastUsedCampus => "last_used_campus",
            AppSettingKey::Language => "language",
            AppSettingKey::MinecraftVersion => "minecraft_version",
            AppSettingKey::FirstRunCompleted => "first_run_completed",
            AppSettingKey::CoverageAuditDecisions => "coverage_audit_decisions",
            AppSettingKey::OnboardingProgress => "onboarding_progress",
            AppSettingKey::OnboardingCompletedAt => "onboarding_completed_at",
            AppSettingKey::GaodeApiKey => "gaode_api_key",
            AppSettingKey::GaodeSecurityKey => "gaode_security_key",
        }
    }
}

/// 校区 CRUD 接口（缝 2，F3 调用）
pub trait CampusCrudApi {
    /// 列出全部校区（按更新时间倒序）
    fn list_campuses(&self) -> Result<Vec<CampusEntity>>;

    /// 创建校区
    fn create_campus(&mut self, name: &str) -> Result<CampusEntity>;

    /// T05：创建校区并指定锚点坐标（高德 POI 中心）
    fn create_campus_with_anchor(
        &mut self,
        name: &str,
        poi_id: &str,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<CampusEntity>;

    /// 按 ID 查校区（不存在返回 None）
    fn find_campus_by_id(&self, campus_id: &str) -> Result<Option<CampusEntity>>;

    /// T05：更新校区的锚点坐标
    fn update_campus_anchor(&self, campus_id: &str, anchor_lng: f64, anchor_lat: f64)
        -> Result<()>;
}

/// 应用设置读写接口（缝 2，"上次使用的校区"等）
pub trait AppSettingsApi {
    /// 读取设置值（键不存在返回 None）
    fn get_setting(&self, key: AppSettingKey) -> Result<Option<String>>;

    /// 写入设置值（存在则覆盖）
    fn set_setting(&mut self, key: AppSettingKey, value: &str) -> Result<()>;
}

/// 方案 CRUD 接口（缝 2，F3 调用）
pub trait PlanCrudApi {
    /// 列出校区内未删除的方案（按最后修改时间倒序，ADR-0018）
    fn list_plans(&self, campus_id: &str) -> Result<Vec<PlanEntity>>;

    /// 创建方案；同一校区内方案名不得重复（ADR-0010）
    fn create_plan(&mut self, campus_id: &str, name: &str) -> Result<PlanEntity>;

    /// 改名；同一校区内方案名不得重复
    fn rename_plan(&mut self, plan_id: &str, new_name: &str) -> Result<()>;

    /// 按 ID 查方案（不存在返回 None）
    fn find_plan_by_id(&self, plan_id: &str) -> Result<Option<PlanEntity>>;

    /// 刷新方案的最后修改时间（进入方案干活后由功能层调用）
    fn touch_plan(&mut self, plan_id: &str) -> Result<()>;
}

/// 当前时刻的 RFC3339 毫秒文本
fn now_text() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// 同一校区内是否已有同名方案（可排除某个 ID，用于改名）。
/// 已在回收站（未恢复、未永久删除）的方案不占名额；
/// 恢复时的同名冲突由 F3 在恢复入口处理（ADR-0018 后果条）。
fn plan_name_taken(
    conn: &Connection,
    campus_id: &str,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<bool> {
    let taken: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM plans p
            WHERE p.campus_id = ?1 AND p.name = ?2 AND p.id != COALESCE(?3, '')
              AND NOT EXISTS(
                  SELECT 1 FROM trash t
                  WHERE t.entity_type = 'plan' AND t.entity_id = p.id
                    AND t.restored_at IS NULL AND t.permanently_deleted_at IS NULL
              )
        )",
        params![campus_id, name, exclude_id],
        |row| row.get(0),
    )?;
    Ok(taken)
}

impl CampusCrudApi for Connection {
    fn list_campuses(&self) -> Result<Vec<CampusEntity>> {
        let mut stmt = self.prepare(
            "SELECT id, name, anchor_lng, anchor_lat, created_at, updated_at FROM campuses ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(CampusEntity {
                id: row.get(0)?,
                name: row.get(1)?,
                anchor_lng: row.get(2)?,
                anchor_lat: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    /// 创建校区（仅名称，用于旧兼容场景）
    fn create_campus(&mut self, name: &str) -> Result<CampusEntity> {
        self.create_campus_with_anchor(name, "", 116.397, 39.916)
    }

    /// T05：创建校区并指定锚点坐标（高德 POI 中心）
    fn create_campus_with_anchor(
        &mut self,
        name: &str,
        _poi_id: &str,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<CampusEntity> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_text();
        // 注意：T05 中 poi_id 列尚未加入 campuses 表，此处预留字段位置
        // 当前实现仅存储锚点，poi_id 存储在 campus_poi_records 或其他扩展表（待 TXX）
        self.execute(
            "INSERT INTO campuses (id, name, anchor_lng, anchor_lat, created_at, updated_at) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, anchor_lng, anchor_lat, now.clone(), now.clone()],
        )?;
        Ok(CampusEntity {
            id,
            name: name.to_owned(),
            anchor_lng,
            anchor_lat,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    fn find_campus_by_id(&self, campus_id: &str) -> Result<Option<CampusEntity>> {
        let entity = self
            .query_row(
                "SELECT id, name, anchor_lng, anchor_lat, created_at, updated_at FROM campuses WHERE id = ?1",
                [campus_id],
                |row| {
                    Ok(CampusEntity {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        anchor_lng: row.get(2)?,
                        anchor_lat: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(entity)
    }

    /// T05：更新校区的锚点坐标
    fn update_campus_anchor(
        &self,
        campus_id: &str,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<()> {
        let now = now_text();
        self.execute(
            "UPDATE campuses SET anchor_lng = ?1, anchor_lat = ?2, updated_at = ?3 WHERE id = ?4",
            params![anchor_lng, anchor_lat, now, campus_id],
        )?;
        Ok(())
    }
}

impl AppSettingsApi for Connection {
    fn get_setting(&self, key: AppSettingKey) -> Result<Option<String>> {
        let value = self
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                [key.as_db_key()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    fn set_setting(&mut self, key: AppSettingKey, value: &str) -> Result<()> {
        self.execute(
            "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                            updated_at = excluded.updated_at",
            params![key.as_db_key(), value, now_text()],
        )?;
        Ok(())
    }
}

impl PlanCrudApi for Connection {
    fn list_plans(&self, campus_id: &str) -> Result<Vec<PlanEntity>> {
        // 在回收站（未恢复、未永久删除）的方案不出现在列表里
        let mut stmt = self.prepare(
            "SELECT p.id, p.campus_id, p.name, p.created_at, p.updated_at
             FROM plans p
             WHERE p.campus_id = ?1
               AND NOT EXISTS(
                   SELECT 1 FROM trash t
                   WHERE t.entity_type = 'plan' AND t.entity_id = p.id
                     AND t.restored_at IS NULL AND t.permanently_deleted_at IS NULL
               )
             ORDER BY p.updated_at DESC",
        )?;
        let rows = stmt.query_map([campus_id], |row| {
            Ok(PlanEntity {
                id: row.get(0)?,
                campus_id: row.get(1)?,
                name: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    fn create_plan(&mut self, campus_id: &str, name: &str) -> Result<PlanEntity> {
        if self.find_campus_by_id(campus_id)?.is_none() {
            return Err(Error::CampusNotFound(campus_id.to_owned()));
        }
        if plan_name_taken(self, campus_id, name, None)? {
            return Err(Error::DuplicatePlanName(name.to_owned()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_text();
        self.execute(
            "INSERT INTO plans (id, campus_id, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, campus_id, name, now, now],
        )?;
        Ok(PlanEntity {
            id,
            campus_id: campus_id.to_owned(),
            name: name.to_owned(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    fn rename_plan(&mut self, plan_id: &str, new_name: &str) -> Result<()> {
        let plan = self
            .find_plan_by_id(plan_id)?
            .ok_or_else(|| Error::PlanNotFound(plan_id.to_owned()))?;
        if plan_name_taken(self, &plan.campus_id, new_name, Some(plan_id))? {
            return Err(Error::DuplicatePlanName(new_name.to_owned()));
        }
        self.execute(
            "UPDATE plans SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_name, now_text(), plan_id],
        )?;
        Ok(())
    }

    fn find_plan_by_id(&self, plan_id: &str) -> Result<Option<PlanEntity>> {
        let entity = self
            .query_row(
                "SELECT id, campus_id, name, created_at, updated_at FROM plans WHERE id = ?1",
                [plan_id],
                |row| {
                    Ok(PlanEntity {
                        id: row.get(0)?,
                        campus_id: row.get(1)?,
                        name: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(entity)
    }

    fn touch_plan(&mut self, plan_id: &str) -> Result<()> {
        let changed = self.execute(
            "UPDATE plans SET updated_at = ?1 WHERE id = ?2",
            params![now_text(), plan_id],
        )?;
        if changed == 0 {
            return Err(Error::PlanNotFound(plan_id.to_owned()));
        }
        Ok(())
    }
}

// Database 转发（F3 只拿得到 Database，拿不到内部 Connection）
impl CampusCrudApi for crate::Database {
    fn list_campuses(&self) -> Result<Vec<CampusEntity>> {
        self.conn.list_campuses()
    }

    fn create_campus(&mut self, name: &str) -> Result<CampusEntity> {
        self.conn.create_campus(name)
    }

    fn create_campus_with_anchor(
        &mut self,
        name: &str,
        poi_id: &str,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<CampusEntity> {
        self.conn
            .create_campus_with_anchor(name, poi_id, anchor_lng, anchor_lat)
    }

    fn find_campus_by_id(&self, campus_id: &str) -> Result<Option<CampusEntity>> {
        self.conn.find_campus_by_id(campus_id)
    }

    fn update_campus_anchor(
        &self,
        campus_id: &str,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<()> {
        self.conn
            .update_campus_anchor(campus_id, anchor_lng, anchor_lat)
    }
}

impl AppSettingsApi for crate::Database {
    fn get_setting(&self, key: AppSettingKey) -> Result<Option<String>> {
        self.conn.get_setting(key)
    }

    fn set_setting(&mut self, key: AppSettingKey, value: &str) -> Result<()> {
        self.conn.set_setting(key, value)
    }
}

impl PlanCrudApi for crate::Database {
    fn list_plans(&self, campus_id: &str) -> Result<Vec<PlanEntity>> {
        self.conn.list_plans(campus_id)
    }

    fn create_plan(&mut self, campus_id: &str, name: &str) -> Result<PlanEntity> {
        self.conn.create_plan(campus_id, name)
    }

    fn rename_plan(&mut self, plan_id: &str, new_name: &str) -> Result<()> {
        self.conn.rename_plan(plan_id, new_name)
    }

    fn find_plan_by_id(&self, plan_id: &str) -> Result<Option<PlanEntity>> {
        self.conn.find_plan_by_id(plan_id)
    }

    fn touch_plan(&mut self, plan_id: &str) -> Result<()> {
        self.conn.touch_plan(plan_id)
    }
}

impl crate::Database {
    /// 删除方案进回收站（ADR-0018：进校园级回收站保留 30 天）。
    ///
    /// 方案行保留（`list_plans` 自动隐藏），回收站登记一条可恢复条目并返回。
    /// 已在站的方案不能重复删除。
    pub fn delete_plan_to_trash(&mut self, campus_id: &str, plan_id: &str) -> Result<TrashItem> {
        let plan = self
            .find_plan_by_id(plan_id)?
            .ok_or_else(|| Error::PlanNotFound(plan_id.to_owned()))?;
        if plan.campus_id != campus_id {
            return Err(Error::PlanNotFound(plan_id.to_owned()));
        }
        let already_trashed = self
            .list_restorable_trash(campus_id)?
            .iter()
            .any(|item| item.entity_type == "plan" && item.entity_id == plan_id);
        if already_trashed {
            return Err(Error::TrashOperationRejected(format!(
                "方案 {plan_id} 已在回收站中"
            )));
        }
        let item = TrashItem::new_plan(campus_id, plan_id, None);
        self.insert_to_trash(&item)?;
        Ok(item)
    }

    /// 确认后永久删除方案（调用方必须先弹确认窗）。
    ///
    /// 回收站条目标记为已永久删除（审计痕迹保留），方案行本身删除；
    /// 两表写操作在同一事务内原子提交（ADR-0002）。
    /// 数据粮仓（raw_observations）铁律不受影响，不动。
    pub fn purge_plan_permanently(&mut self, trash_id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let row: Option<(String, Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT entity_id, restored_at, permanently_deleted_at
                 FROM trash WHERE id = ?1 AND entity_type = 'plan'",
                [trash_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((plan_id, restored_at, permanently_deleted_at)) = row else {
            return Err(Error::TrashOperationRejected(format!(
                "条目 {trash_id} 不存在"
            )));
        };
        if restored_at.is_some() || permanently_deleted_at.is_some() {
            return Err(Error::TrashOperationRejected(format!(
                "条目 {trash_id} 已恢复或已永久删除，不能再次永久删除"
            )));
        }
        tx.execute(
            "UPDATE trash SET permanently_deleted_at = ?1 WHERE id = ?2",
            params![timestamp_to_db(Utc::now()), trash_id],
        )?;
        tx.execute("DELETE FROM plans WHERE id = ?1", [plan_id])?;
        tx.commit()?;
        Ok(())
    }

    /// 到期清理框架：标记超过保留期的条目为已永久删除，并清理对应方案行
    ///（同一事务原子提交）；返回本次标记的条数。
    /// 调用时机（启动时/定时）由应用壳决定。
    pub fn purge_expired_plans(&mut self) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let cutoff = Utc::now() - chrono::Duration::days(TRASH_RETENTION_DAYS);
        let purged = tx.execute(
            "UPDATE trash SET permanently_deleted_at = ?1
             WHERE restored_at IS NULL AND permanently_deleted_at IS NULL
               AND deleted_at < ?2",
            params![timestamp_to_db(Utc::now()), timestamp_to_db(cutoff)],
        )?;
        // 幂等收尾：凡已标记永久删除的方案，方案行一律清掉
        tx.execute(
            "DELETE FROM plans WHERE id IN (
                SELECT entity_id FROM trash
                WHERE entity_type = 'plan' AND permanently_deleted_at IS NOT NULL
            )",
            [],
        )?;
        tx.commit()?;
        Ok(purged)
    }
}
