//! 校园级回收站 API
//!
//! 缝 2 契约：F3 方案管理向 B2 要回收站的进站/恢复/到期清理/确认后永久删除。
//! 决策记忆：保留 30 天；"立即永久删除"由 F3 弹确认窗后才调用本模块。
//! 恢复冲突（如同名方案已存在）属 F3 业务判断，B2 只负责状态写回。
//!
//! 关键决策：到期自动清理只提供**框架**（[`TrashApi::purge_expired`] 由调用方在
//! 启动/定时时机触发），B2 不内置定时任务——定时器属功能层职责。

use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::database::Database;
use crate::entities::{timestamp_from_db, timestamp_to_db, TrashItem, TRASH_RETENTION_DAYS};
use crate::error::{Error, Result};

/// 回收站接口（供 F3 方案管理调用）
pub trait TrashApi {
    /// 进站：登记一条删除记录
    fn insert_to_trash(&mut self, item: &TrashItem) -> Result<()>;

    /// 列出校区回收站中仍可恢复的条目（未恢复、未永久删除、未过 30 天）
    fn list_restorable_trash(&self, campus_id: &str) -> Result<Vec<TrashItem>>;

    /// 恢复：把条目标记为已恢复并返回它（已恢复/已永久删除/已过期则拒绝）
    fn restore_from_trash(&mut self, id: &str) -> Result<TrashItem>;

    /// 确认后永久删除：把条目标记为已永久删除（调用方必须先弹确认窗）
    fn permanently_delete(&mut self, id: &str) -> Result<()>;

    /// 到期清理框架：把所有超过保留期的未处理条目标记为已永久删除，
    /// 返回本次清理的条数。调用时机（启动时/定时）由功能层决定。
    fn purge_expired(&mut self) -> Result<usize>;
}

impl TrashApi for Database {
    fn insert_to_trash(&mut self, item: &TrashItem) -> Result<()> {
        self.conn.execute(
            "INSERT INTO trash
                (id, campus_id, plan_id, entity_type, entity_id,
                 deleted_at, deleted_by, restored_at, permanently_deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                item.id,
                item.campus_id,
                item.plan_id,
                item.entity_type,
                item.entity_id,
                timestamp_to_db(item.deleted_at),
                item.deleted_by,
                item.restored_at.map(timestamp_to_db),
                item.permanently_deleted_at.map(timestamp_to_db),
            ],
        )?;
        Ok(())
    }

    fn list_restorable_trash(&self, campus_id: &str) -> Result<Vec<TrashItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, campus_id, plan_id, entity_type, entity_id,
                    deleted_at, deleted_by, restored_at, permanently_deleted_at
             FROM trash
             WHERE campus_id = ?1 AND restored_at IS NULL
               AND permanently_deleted_at IS NULL
             ORDER BY deleted_at DESC",
        )?;
        let mut rows = stmt.query(params![campus_id])?;
        let now = Utc::now();
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let item = row_to_item(row)?;
            if item.is_restorable(now) {
                result.push(item);
            }
        }
        Ok(result)
    }

    fn restore_from_trash(&mut self, id: &str) -> Result<TrashItem> {
        let item = self.find_trash_item(id)?;
        if !item.is_restorable(Utc::now()) {
            return Err(Error::TrashOperationRejected(format!(
                "条目 {id} 已恢复、已永久删除或已过 {TRASH_RETENTION_DAYS} 天保留期"
            )));
        }
        let restored_at = Utc::now();
        self.conn.execute(
            "UPDATE trash SET restored_at = ?1 WHERE id = ?2",
            params![timestamp_to_db(restored_at), id],
        )?;
        Ok(TrashItem {
            restored_at: Some(restored_at),
            ..item
        })
    }

    fn permanently_delete(&mut self, id: &str) -> Result<()> {
        let item = self.find_trash_item(id)?;
        if item.restored_at.is_some() || item.permanently_deleted_at.is_some() {
            return Err(Error::TrashOperationRejected(format!(
                "条目 {id} 已恢复或已永久删除，不能再次永久删除"
            )));
        }
        self.conn.execute(
            "UPDATE trash SET permanently_deleted_at = ?1 WHERE id = ?2",
            params![timestamp_to_db(Utc::now()), id],
        )?;
        Ok(())
    }

    fn purge_expired(&mut self) -> Result<usize> {
        let cutoff: DateTime<Utc> = Utc::now() - chrono::Duration::days(TRASH_RETENTION_DAYS);
        let purged = self.conn.execute(
            "UPDATE trash SET permanently_deleted_at = ?1
             WHERE restored_at IS NULL AND permanently_deleted_at IS NULL
               AND deleted_at < ?2",
            params![timestamp_to_db(Utc::now()), timestamp_to_db(cutoff)],
        )?;
        Ok(purged)
    }
}

impl Database {
    /// 按 ID 取回收站条目（不存在则拒绝）
    fn find_trash_item(&self, id: &str) -> Result<TrashItem> {
        let mut stmt = self.conn.prepare(
            "SELECT id, campus_id, plan_id, entity_type, entity_id,
                    deleted_at, deleted_by, restored_at, permanently_deleted_at
             FROM trash WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => row_to_item(row),
            None => Err(Error::TrashOperationRejected(format!("条目 {id} 不存在"))),
        }
    }
}

/// 单行 → 回收站条目（列顺序与 SELECT 一致）
fn row_to_item(row: &rusqlite::Row<'_>) -> Result<TrashItem> {
    let deleted_at: String = row.get(5)?;
    let restored_at: Option<String> = row.get(7)?;
    let permanently_deleted_at: Option<String> = row.get(8)?;
    Ok(TrashItem {
        id: row.get(0)?,
        campus_id: row.get(1)?,
        plan_id: row.get(2)?,
        entity_type: row.get(3)?,
        entity_id: row.get(4)?,
        deleted_at: timestamp_from_db(&deleted_at)?,
        deleted_by: row.get(6)?,
        restored_at: restored_at.as_deref().map(timestamp_from_db).transpose()?,
        permanently_deleted_at: permanently_deleted_at
            .as_deref()
            .map(timestamp_from_db)
            .transpose()?,
    })
}
