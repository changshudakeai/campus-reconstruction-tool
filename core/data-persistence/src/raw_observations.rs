//! 原始观测表 API（数据粮仓）
//!
//! 缝 3 契约：F4 采集流水线把带完整原始标签的观测数据**当场写入**本表。
//! 铁律：本表永不删除——本模块不提供、也永不提供删除 API；
//! 重复采集同一实体时按内容指纹（digest）增量刷新（UPSERT，保留 created_at）。

use rusqlite::params;

use crate::database::Database;
use crate::entities::{
    category_from_db, category_to_db, timestamp_from_db, timestamp_to_db, RawObservation,
};
use crate::error::Result;
use shared_domain_types::CandidateCategory;

/// 原始观测表读写接口（供 F4 采集、未来精细建筑模式调用）
pub trait RawObservationsApi {
    /// 批量写入原始观测（单事务原子提交），返回受影响行数。
    ///
    /// 同一 `(plan_id, entity_type, entity_id)` 已存在时按 digest 增量刷新：
    /// 指纹相同则原样保留，指纹不同则更新 source_data/digest/updated_at
    /// （created_at 不动）。任何路径都不会丢失既有行。
    fn write_raw_observations(&mut self, observations: &[RawObservation]) -> Result<usize>;

    /// 列出方案下的全部原始观测（按类别、实体 ID 排序）
    fn list_raw_observations(&self, plan_id: &str) -> Result<Vec<RawObservation>>;

    /// 按类别列出方案下的原始观测
    fn list_raw_observations_by_category(
        &self,
        plan_id: &str,
        category: CandidateCategory,
    ) -> Result<Vec<RawObservation>>;

    /// 查询单条观测（用于增量刷新前的指纹比对）
    fn find_raw_observation(
        &self,
        plan_id: &str,
        category: CandidateCategory,
        entity_id: &str,
    ) -> Result<Option<RawObservation>>;
}

impl RawObservationsApi for Database {
    fn write_raw_observations(&mut self, observations: &[RawObservation]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut written = 0usize;
        {
            // UPSERT：digest 相同时 WHERE 子句拦下无效更新，保住 updated_at 语义
            let mut stmt = tx.prepare(
                "INSERT INTO raw_observations
                    (id, plan_id, entity_type, entity_id, source_data,
                     data_source_tag, digest, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(plan_id, entity_type, entity_id) DO UPDATE SET
                    source_data = excluded.source_data,
                    data_source_tag = excluded.data_source_tag,
                    digest = excluded.digest,
                    updated_at = excluded.updated_at
                 WHERE raw_observations.digest <> excluded.digest",
            )?;
            for observation in observations {
                written += stmt.execute(params![
                    observation.id,
                    observation.plan_id,
                    category_to_db(observation.entity_type)?,
                    observation.entity_id,
                    observation.source_data.to_string(),
                    observation.data_source_tag,
                    observation.digest,
                    timestamp_to_db(observation.created_at),
                    timestamp_to_db(observation.updated_at),
                ])?;
            }
        }
        tx.commit()?;
        Ok(written)
    }

    fn list_raw_observations(&self, plan_id: &str) -> Result<Vec<RawObservation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, plan_id, entity_type, entity_id, source_data,
                    data_source_tag, digest, created_at, updated_at
             FROM raw_observations WHERE plan_id = ?1
             ORDER BY entity_type, entity_id",
        )?;
        let observations = collect_observations(stmt.query(params![plan_id])?)?;
        Ok(observations)
    }

    fn list_raw_observations_by_category(
        &self,
        plan_id: &str,
        category: CandidateCategory,
    ) -> Result<Vec<RawObservation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, plan_id, entity_type, entity_id, source_data,
                    data_source_tag, digest, created_at, updated_at
             FROM raw_observations WHERE plan_id = ?1 AND entity_type = ?2
             ORDER BY entity_id",
        )?;
        let observations =
            collect_observations(stmt.query(params![plan_id, category_to_db(category)?])?)?;
        Ok(observations)
    }

    fn find_raw_observation(
        &self,
        plan_id: &str,
        category: CandidateCategory,
        entity_id: &str,
    ) -> Result<Option<RawObservation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, plan_id, entity_type, entity_id, source_data,
                    data_source_tag, digest, created_at, updated_at
             FROM raw_observations
             WHERE plan_id = ?1 AND entity_type = ?2 AND entity_id = ?3",
        )?;
        let mut rows = collect_observations(stmt.query(params![
            plan_id,
            category_to_db(category)?,
            entity_id
        ])?)?;
        Ok(rows.pop())
    }
}

/// 把查询结果集逐行转成实体（列顺序与上方 SELECT 一致）
fn collect_observations(mut rows: rusqlite::Rows<'_>) -> Result<Vec<RawObservation>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let source_data: String = row.get(4)?;
        let created_at: String = row.get(7)?;
        let updated_at: String = row.get(8)?;
        result.push(RawObservation {
            id: row.get(0)?,
            plan_id: row.get(1)?,
            entity_type: category_from_db(&row.get::<_, String>(2)?)?,
            entity_id: row.get(3)?,
            source_data: serde_json::from_str(&source_data)?,
            data_source_tag: row.get(5)?,
            digest: row.get(6)?,
            created_at: timestamp_from_db(&created_at)?,
            updated_at: timestamp_from_db(&updated_at)?,
        });
    }
    Ok(result)
}
