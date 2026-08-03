//! ADR-0040 候选投影与批次发布：原始观测保持不变，只有完整发布的投影可被评审读取。

use chrono::{DateTime, Utc};
use rusqlite::params;
use shared_domain_types::CandidateCategory;
use uuid::Uuid;

use crate::database::Database;
use crate::entities::{category_from_db, category_to_db, timestamp_from_db, timestamp_to_db};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateEligibility {
    Reviewable,
    Isolated,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateValidation {
    Retained,
    Repaired,
    Rejected,
}
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateShape {
    pub kind: String,
    pub coordinates: serde_json::Value,
}
impl CandidateShape {
    pub fn point(coordinates: serde_json::Value) -> Self {
        Self {
            kind: "point".to_owned(),
            coordinates,
        }
    }
    pub fn line_string(coordinates: serde_json::Value) -> Self {
        Self {
            kind: "line_string".to_owned(),
            coordinates,
        }
    }
    pub fn polygon(coordinates: serde_json::Value) -> Self {
        Self {
            kind: "polygon".to_owned(),
            coordinates,
        }
    }
}

/// F5 展示候选所需的最小投影属性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDisplay {
    /// 候选卡片标题。
    pub title: String,
    /// 用于展示的来源标签与属性。
    pub tags: Vec<(String, String)>,
}

impl CandidateDisplay {
    pub fn new(title: impl Into<String>, tags: Vec<(String, String)>) -> Self {
        Self {
            title: title.into(),
            tags,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateProjection {
    pub candidate_id: String,
    pub plan_id: String,
    pub collection_batch_id: Option<String>,
    pub raw_observation_id: String,
    pub data_source_tag: String,
    pub source_entity_id: String,
    pub geometry_part_id: String,
    pub category: CandidateCategory,
    pub display: CandidateDisplay,
    pub shape: CandidateShape,
    pub validation: CandidateValidation,
    pub eligibility: CandidateEligibility,
    pub isolation_reason: Option<String>,
    pub automatically_repaired: bool,
    pub missing_in_latest_batch: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl CandidateProjection {
    #[allow(
        clippy::too_many_arguments,
        reason = "ADR-0040 候选投影是跨模块的完整来源与资格事实"
    )]
    pub fn new(
        candidate_id: impl Into<String>,
        plan_id: impl Into<String>,
        raw_observation_id: impl Into<String>,
        data_source_tag: impl Into<String>,
        source_entity_id: impl Into<String>,
        geometry_part_id: impl Into<String>,
        category: CandidateCategory,
        display: CandidateDisplay,
        shape: CandidateShape,
        validation: CandidateValidation,
        eligibility: CandidateEligibility,
    ) -> Self {
        let now = Utc::now();
        Self {
            candidate_id: candidate_id.into(),
            plan_id: plan_id.into(),
            collection_batch_id: None,
            raw_observation_id: raw_observation_id.into(),
            data_source_tag: data_source_tag.into(),
            source_entity_id: source_entity_id.into(),
            geometry_part_id: geometry_part_id.into(),
            category,
            display,
            shape,
            validation,
            eligibility,
            isolation_reason: None,
            automatically_repaired: matches!(validation, CandidateValidation::Repaired),
            missing_in_latest_batch: false,
            created_at: now,
            updated_at: now,
        }
    }
    pub fn isolated_reason(mut self, reason: impl Into<String>) -> Self {
        self.isolation_reason = Some(reason.into());
        self
    }
    pub fn missing_in_latest_batch(mut self) -> Self {
        self.missing_in_latest_batch = true;
        self
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBatch {
    pub id: String,
    pub plan_id: String,
    pub status: CandidateBatchStatus,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateBatchStatus {
    Building,
    Published,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBatchSummary {
    pub batch: CandidateBatch,
    pub projection_count: usize,
    pub reviewable_count: usize,
    pub isolated_count: usize,
    pub repaired_count: usize,
    pub missing_count: usize,
}

pub trait CandidateProjectionsApi {
    fn prepare_candidate_batch(&mut self, plan_id: &str) -> Result<CandidateBatch>;
    fn write_candidate_projections(
        &mut self,
        batch_id: &str,
        projections: &[CandidateProjection],
    ) -> Result<()>;
    /// 把当前完整批次中本次未返回的投影带入构建批次，并显式标记缺失。
    fn carry_forward_missing_candidate_projections(&mut self, batch_id: &str) -> Result<()>;
    fn publish_candidate_batch(&mut self, batch_id: &str) -> Result<()>;
    fn list_reviewable_candidate_projections(
        &self,
        plan_id: &str,
    ) -> Result<Vec<CandidateProjection>>;
    fn get_current_candidate_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<CandidateProjection>>;
    fn candidate_batch_summary(&self, batch_id: &str) -> Result<CandidateBatchSummary>;
}
impl CandidateProjectionsApi for Database {
    fn prepare_candidate_batch(&mut self, plan_id: &str) -> Result<CandidateBatch> {
        let now = Utc::now();
        let batch = CandidateBatch {
            id: Uuid::new_v4().to_string(),
            plan_id: plan_id.to_owned(),
            status: CandidateBatchStatus::Building,
            created_at: now,
            published_at: None,
        };
        self.conn.execute("INSERT INTO candidate_batches (id, plan_id, status, created_at) VALUES (?1,?2,'building',?3)",params![batch.id,batch.plan_id,timestamp_to_db(now)])?;
        Ok(batch)
    }
    fn write_candidate_projections(
        &mut self,
        batch_id: &str,
        projections: &[CandidateProjection],
    ) -> Result<()> {
        let batch = self.batch(batch_id)?;
        if batch.status != CandidateBatchStatus::Building {
            return Err(Error::CandidateBatchRejected(
                "已发布批次不可再写入".to_owned(),
            ));
        }
        let tx = self.conn.transaction()?;
        {
            let mut statement=tx.prepare("INSERT INTO candidate_projections (collection_batch_id,candidate_id,plan_id,raw_observation_id,data_source_tag,source_entity_id,geometry_part_id,category,display_title,display_tags,geometry_kind,normalized_geometry,validation,eligibility,isolation_reason,automatically_repaired,missing_in_latest_batch,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)")?;
            for projection in projections {
                if projection.plan_id != batch.plan_id {
                    return Err(Error::CandidateBatchRejected(
                        "投影方案与批次不一致".to_owned(),
                    ));
                }
                statement.execute(params![
                    batch_id,
                    projection.candidate_id,
                    projection.plan_id,
                    projection.raw_observation_id,
                    projection.data_source_tag,
                    projection.source_entity_id,
                    projection.geometry_part_id,
                    category_to_db(projection.category)?,
                    projection.display.title,
                    serde_json::to_string(&projection.display.tags)?,
                    projection.shape.kind,
                    projection.shape.coordinates.to_string(),
                    validation_db(projection.validation),
                    eligibility_db(projection.eligibility),
                    projection.isolation_reason,
                    projection.automatically_repaired,
                    projection.missing_in_latest_batch,
                    timestamp_to_db(projection.created_at),
                    timestamp_to_db(projection.updated_at)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    fn publish_candidate_batch(&mut self, batch_id: &str) -> Result<()> {
        let batch = self.batch(batch_id)?;
        if batch.status != CandidateBatchStatus::Building {
            return Err(Error::CandidateBatchRejected("批次不是构建状态".to_owned()));
        }
        let now = Utc::now();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE candidate_batches SET status='published', published_at=?1 WHERE id=?2",
            params![timestamp_to_db(now), batch_id],
        )?;
        tx.execute("INSERT INTO current_candidate_batches (plan_id,collection_batch_id) VALUES (?1,?2) ON CONFLICT(plan_id) DO UPDATE SET collection_batch_id=excluded.collection_batch_id",params![batch.plan_id,batch_id])?;
        tx.commit()?;
        Ok(())
    }

    fn carry_forward_missing_candidate_projections(&mut self, batch_id: &str) -> Result<()> {
        let batch = self.batch(batch_id)?;
        if batch.status != CandidateBatchStatus::Building {
            return Err(Error::CandidateBatchRejected(
                "已发布批次不可继承缺失候选".to_owned(),
            ));
        }
        let mut projections = self.current_projections(&batch.plan_id)?;
        for projection in &mut projections {
            projection.collection_batch_id = None;
            projection.missing_in_latest_batch = true;
            projection.updated_at = Utc::now();
        }
        self.write_candidate_projections(batch_id, &projections)
    }
    fn list_reviewable_candidate_projections(
        &self,
        plan_id: &str,
    ) -> Result<Vec<CandidateProjection>> {
        self.read_projections("SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id WHERE c.plan_id=?1 AND p.eligibility='reviewable' ORDER BY p.candidate_id",params![plan_id])
    }
    fn get_current_candidate_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<CandidateProjection>> {
        Ok(self.read_projections("SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id WHERE c.plan_id=?1 AND p.candidate_id=?2",params![plan_id,candidate_id])?.into_iter().next())
    }
    fn candidate_batch_summary(&self, batch_id: &str) -> Result<CandidateBatchSummary> {
        let batch = self.batch(batch_id)?;
        let (all,reviewable,isolated,repaired,missing):(usize,usize,usize,usize,usize)=self.conn.query_row("SELECT COUNT(*),SUM(eligibility='reviewable'),SUM(eligibility='isolated'),SUM(automatically_repaired),SUM(missing_in_latest_batch) FROM candidate_projections WHERE collection_batch_id=?1",params![batch_id],|row| Ok((row.get(0)?,row.get::<_,Option<usize>>(1)?.unwrap_or(0),row.get::<_,Option<usize>>(2)?.unwrap_or(0),row.get::<_,Option<usize>>(3)?.unwrap_or(0),row.get::<_,Option<usize>>(4)?.unwrap_or(0))))?;
        Ok(CandidateBatchSummary {
            batch,
            projection_count: all,
            reviewable_count: reviewable,
            isolated_count: isolated,
            repaired_count: repaired,
            missing_count: missing,
        })
    }
}
impl Database {
    fn batch(&self, batch_id: &str) -> Result<CandidateBatch> {
        self.conn.query_row("SELECT id,plan_id,status,created_at,published_at FROM candidate_batches WHERE id=?1",params![batch_id],|row| { let status:String=row.get(2)?; Ok(CandidateBatch { id:row.get(0)?,plan_id:row.get(1)?,status: if status=="building" {CandidateBatchStatus::Building} else {CandidateBatchStatus::Published},created_at:timestamp_from_db(&row.get::<_,String>(3)?).map_err(to_sql_error)?,published_at:row.get::<_,Option<String>>(4)?.map(|value| timestamp_from_db(&value).map_err(to_sql_error)).transpose()?}) }).map_err(Error::from)
    }
    fn read_projections<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<Vec<CandidateProjection>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params, projection_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    fn current_projections(&self, plan_id: &str) -> Result<Vec<CandidateProjection>> {
        self.read_projections(
            "SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id WHERE c.plan_id=?1",
            params![plan_id],
        )
    }
}
fn projection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CandidateProjection> {
    let validation = match row.get::<_, String>(12)?.as_str() {
        "retained" => CandidateValidation::Retained,
        "repaired" => CandidateValidation::Repaired,
        _ => CandidateValidation::Rejected,
    };
    let eligibility = if row.get::<_, String>(13)? == "reviewable" {
        CandidateEligibility::Reviewable
    } else {
        CandidateEligibility::Isolated
    };
    Ok(CandidateProjection {
        collection_batch_id: Some(row.get(0)?),
        candidate_id: row.get(1)?,
        plan_id: row.get(2)?,
        raw_observation_id: row.get(3)?,
        data_source_tag: row.get(4)?,
        source_entity_id: row.get(5)?,
        geometry_part_id: row.get(6)?,
        category: category_from_db(&row.get::<_, String>(7)?).map_err(to_sql_error)?,
        display: CandidateDisplay {
            title: row.get(8)?,
            tags: serde_json::from_str(&row.get::<_, String>(9)?)
                .map_err(|error| to_sql_error(Error::from(error)))?,
        },
        shape: CandidateShape {
            kind: row.get(10)?,
            coordinates: serde_json::from_str(&row.get::<_, String>(11)?)
                .map_err(|error| to_sql_error(Error::from(error)))?,
        },
        validation,
        eligibility,
        isolation_reason: row.get(14)?,
        automatically_repaired: row.get(15)?,
        missing_in_latest_batch: row.get(16)?,
        created_at: timestamp_from_db(&row.get::<_, String>(17)?).map_err(to_sql_error)?,
        updated_at: timestamp_from_db(&row.get::<_, String>(18)?).map_err(to_sql_error)?,
    })
}
fn validation_db(value: CandidateValidation) -> &'static str {
    match value {
        CandidateValidation::Retained => "retained",
        CandidateValidation::Repaired => "repaired",
        CandidateValidation::Rejected => "rejected",
    }
}
fn eligibility_db(value: CandidateEligibility) -> &'static str {
    match value {
        CandidateEligibility::Reviewable => "reviewable",
        CandidateEligibility::Isolated => "isolated",
    }
}
fn to_sql_error(error: Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}
