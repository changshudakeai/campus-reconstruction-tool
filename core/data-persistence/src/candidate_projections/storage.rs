//! 候选投影生命周期的 SQLite 行映射与稳定身份编码。

use rusqlite::{params, Transaction};

use super::{
    CandidateBatch, CandidateBatchStatus, CandidateDisplay, CandidateEligibility,
    CandidateNameSource, CandidateProjection, CandidateShape, CandidateSourceIdentity,
    CandidateValidation,
};
use crate::database::Database;
use crate::entities::{category_from_db, category_to_db, timestamp_from_db, timestamp_to_db};
use crate::error::{Error, Result};

pub(super) fn insert_projection(
    tx: &Transaction<'_>,
    batch_id: &str,
    projection: &CandidateProjection,
) -> Result<()> {
    tx.execute(
        "INSERT INTO candidate_projections
            (collection_batch_id, candidate_id, plan_id, raw_observation_id,
             data_source_tag, source_entity_id, geometry_part_id, category,
             display_title, display_tags, geometry_kind, normalized_geometry,
             validation, eligibility, isolation_reason, automatically_repaired,
             missing_in_latest_batch, created_at, updated_at, name_source)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
             ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
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
            timestamp_to_db(projection.updated_at),
            projection.name_source.to_db(),
        ],
    )?;
    Ok(())
}

impl Database {
    pub(super) fn batch(&self, batch_id: &str) -> Result<CandidateBatch> {
        self.conn
            .query_row(
                "SELECT id,plan_id,status,created_at,published_at FROM candidate_batches WHERE id=?1",
                params![batch_id],
                |row| {
                    let status: String = row.get(2)?;
                    Ok(CandidateBatch {
                        id: row.get(0)?,
                        plan_id: row.get(1)?,
                        status: if status == "building" {
                            CandidateBatchStatus::Building
                        } else {
                            CandidateBatchStatus::Published
                        },
                        created_at: timestamp_from_db(&row.get::<_, String>(3)?)
                            .map_err(to_sql_error)?,
                        published_at: row
                            .get::<_, Option<String>>(4)?
                            .map(|value| timestamp_from_db(&value).map_err(to_sql_error))
                            .transpose()?,
                    })
                },
            )
            .map_err(Error::from)
    }

    pub(super) fn read_projections<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<Vec<CandidateProjection>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params, projection_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    pub(super) fn current_projections(&self, plan_id: &str) -> Result<Vec<CandidateProjection>> {
        self.read_projections(
            "SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at,p.name_source FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id WHERE c.plan_id=?1",
            params![plan_id],
        )
    }
}

fn projection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CandidateProjection> {
    let validation_value = row.get::<_, String>(12)?;
    let validation = match validation_value.as_str() {
        "retained" => CandidateValidation::Retained,
        "repaired" => CandidateValidation::Repaired,
        "rejected" => CandidateValidation::Rejected,
        _ => {
            return Err(to_sql_error(Error::CandidateBatchRejected(format!(
                "数据库候选投影含未知验证状态：{validation_value}"
            ))));
        }
    };
    let eligibility_value = row.get::<_, String>(13)?;
    let eligibility = match eligibility_value.as_str() {
        "reviewable" => CandidateEligibility::Reviewable,
        "isolated" => CandidateEligibility::Isolated,
        _ => {
            return Err(to_sql_error(Error::CandidateBatchRejected(format!(
                "数据库候选投影含未知资格状态：{eligibility_value}"
            ))));
        }
    };
    let isolation_reason = row.get::<_, Option<String>>(14)?;
    let automatically_repaired = row.get::<_, bool>(15)?;
    let legal = match eligibility {
        CandidateEligibility::Reviewable => {
            matches!(
                validation,
                CandidateValidation::Retained | CandidateValidation::Repaired
            ) && isolation_reason.is_none()
        }
        CandidateEligibility::Isolated => isolation_reason
            .as_deref()
            .is_some_and(|reason| !reason.trim().is_empty()),
    } && automatically_repaired == (validation == CandidateValidation::Repaired);
    if !legal {
        return Err(to_sql_error(Error::CandidateBatchRejected(
            "数据库候选投影的验证、资格、隔离原因或修复标记互相矛盾".to_owned(),
        )));
    }
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
        isolation_reason,
        automatically_repaired,
        missing_in_latest_batch: row.get(16)?,
        created_at: timestamp_from_db(&row.get::<_, String>(17)?).map_err(to_sql_error)?,
        updated_at: timestamp_from_db(&row.get::<_, String>(18)?).map_err(to_sql_error)?,
        name_source: CandidateNameSource::from_db(&row.get::<_, String>(19)?),
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

pub(super) fn stable_candidate_id(plan_id: &str, source: &CandidateSourceIdentity) -> String {
    fn component(value: &str) -> String {
        value.replace('%', "%25").replace(':', "%3A")
    }
    format!(
        "candidate:{}:{}:{}:{}",
        component(plan_id),
        component(&source.data_source_tag),
        component(&source.source_entity_id),
        component(&source.geometry_part_id)
    )
}

fn to_sql_error(error: Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}
