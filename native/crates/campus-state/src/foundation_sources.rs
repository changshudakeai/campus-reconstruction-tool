use crate::{
    CampusProject, CandidateConfidence, FoundationWorkflow, GeoPoint, MapCandidate, ReviewDecision,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FoundationSourceProvider {
    OpenStreetMap,
    Overture,
    VisualFeatureProvider,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FoundationSourceStatus {
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationSourceSnapshot {
    pub id: String,
    pub provider: FoundationSourceProvider,
    pub provider_version: String,
    pub status: FoundationSourceStatus,
    pub south_west: GeoPoint,
    pub north_east: GeoPoint,
    pub acquired_at_unix_ms: u64,
    #[serde(default)]
    pub candidates: Vec<MapCandidate>,
    #[serde(default)]
    pub error: Option<String>,
}

impl FoundationSourceSnapshot {
    pub fn from_result(
        provider: FoundationSourceProvider,
        provider_version: impl Into<String>,
        south_west: GeoPoint,
        north_east: GeoPoint,
        result: Result<Vec<MapCandidate>, String>,
    ) -> Self {
        let acquired_at_unix_ms = now_unix_ms();
        let provider_version = provider_version.into();
        let provider_slug = match provider {
            FoundationSourceProvider::OpenStreetMap => "osm",
            FoundationSourceProvider::Overture => "overture",
            FoundationSourceProvider::VisualFeatureProvider => "visual",
        };
        let (status, candidates, error) = match result {
            Ok(candidates) => (FoundationSourceStatus::Complete, candidates, None),
            Err(error) => (FoundationSourceStatus::Failed, Vec::new(), Some(error)),
        };
        Self {
            id: format!("{provider_slug}:{acquired_at_unix_ms}"),
            provider,
            provider_version,
            status,
            south_west,
            north_east,
            acquired_at_unix_ms,
            candidates,
            error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewLedgerEntry {
    pub candidate_id: String,
    pub source_snapshot_id: Option<String>,
    pub decision: ReviewDecision,
    pub decided_at_unix_ms: u64,
}

pub struct FoundationSourceRegistry;

impl FoundationSourceRegistry {
    pub fn ingest(
        project: &mut CampusProject,
        mut snapshot: FoundationSourceSnapshot,
    ) -> Result<usize, String> {
        FoundationWorkflow::ensure_feature_discovery_allowed(project)?;
        if snapshot.south_west.lng >= snapshot.north_east.lng
            || snapshot.south_west.lat >= snapshot.north_east.lat
        {
            return Err("Foundation Source Snapshot 覆盖范围无效".into());
        }
        if snapshot.status == FoundationSourceStatus::Failed {
            snapshot.candidates.clear();
            project.foundation_source_snapshots.push(snapshot);
            return Ok(0);
        }
        let old_snapshot_ids = project
            .foundation_source_snapshots
            .iter()
            .filter(|existing| existing.provider == snapshot.provider)
            .map(|existing| existing.id.clone())
            .collect::<Vec<_>>();
        let count = snapshot.candidates.len();
        for candidate in &mut snapshot.candidates {
            candidate.source_snapshot_id = Some(snapshot.id.clone());
            if let Some(previous) = project
                .candidates
                .iter()
                .find(|previous| previous.id == candidate.id)
            {
                if previous.points == candidate.points {
                    candidate.review =
                        latest_decision(project, &candidate.id).unwrap_or(previous.review);
                } else {
                    candidate.review = ReviewDecision::Pending;
                }
            }
        }
        snapshot.candidates.retain(|candidate| {
            !project
                .building_suppressions
                .iter()
                .any(|record| record.source_id == candidate.id)
        });
        let refreshed_ids = snapshot
            .candidates
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<Vec<_>>();
        project.candidates.retain(|candidate| {
            let from_replaced_provider = candidate
                .source_snapshot_id
                .as_ref()
                .is_some_and(|id| old_snapshot_ids.contains(id));
            !from_replaced_provider
                || (candidate.review != ReviewDecision::Pending
                    && !refreshed_ids.contains(&candidate.id.as_str()))
        });
        project.candidates.extend(snapshot.candidates.clone());
        project.foundation_source_snapshots.push(snapshot);
        Ok(count)
    }

    pub fn record_review(
        project: &mut CampusProject,
        candidate_id: &str,
        decision: ReviewDecision,
    ) {
        let source_snapshot_id = project
            .candidates
            .iter()
            .find(|candidate| candidate.id == candidate_id)
            .and_then(|candidate| candidate.source_snapshot_id.clone());
        project
            .foundation_review_ledger
            .push(FoundationReviewLedgerEntry {
                candidate_id: candidate_id.to_string(),
                source_snapshot_id,
                decision,
                decided_at_unix_ms: now_unix_ms(),
            });
    }
}

fn latest_decision(project: &CampusProject, candidate_id: &str) -> Option<ReviewDecision> {
    project
        .foundation_review_ledger
        .iter()
        .rev()
        .find(|entry| entry.candidate_id == candidate_id)
        .map(|entry| entry.decision)
}

pub fn normalize_candidate_confidence(value: &str) -> CandidateConfidence {
    match value.trim().to_ascii_lowercase().as_str() {
        "high" | "较高" | "高" => CandidateConfidence::High,
        "medium" | "manual" | "中等" | "中" => CandidateConfidence::Medium,
        _ => CandidateConfidence::Low,
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CampusTargetEvidence, FeatureKind, FoundationStep, FoundationWorkflowIntent};
    use std::collections::BTreeMap;

    fn reviewed_project() -> CampusProject {
        let mut project = CampusProject::new("test", "campus");
        FoundationWorkflow::apply(
            &mut project,
            FoundationWorkflowIntent::SelectCampusTarget(CampusTargetEvidence {
                poi_id: "campus".into(),
                name: "campus".into(),
                gcj02: GeoPoint { lng: 1.0, lat: 1.0 },
                wgs84: GeoPoint { lng: 1.0, lat: 1.0 },
                acquisition: "test".into(),
            }),
        )
        .unwrap();
        FoundationWorkflow::apply(
            &mut project,
            FoundationWorkflowIntent::ConfirmCampusBoundary(vec![
                GeoPoint { lng: 0.0, lat: 0.0 },
                GeoPoint { lng: 3.0, lat: 0.0 },
                GeoPoint { lng: 3.0, lat: 3.0 },
            ]),
        )
        .unwrap();
        FoundationWorkflow::apply(&mut project, FoundationWorkflowIntent::CompleteCurrentStep)
            .unwrap();
        assert_eq!(project.foundation_step, FoundationStep::Building);
        project
    }

    fn snapshot(id: &str, name: &str) -> FoundationSourceSnapshot {
        FoundationSourceSnapshot {
            id: id.into(),
            provider: FoundationSourceProvider::OpenStreetMap,
            provider_version: "overpass/v1".into(),
            status: FoundationSourceStatus::Complete,
            south_west: GeoPoint { lng: 0.0, lat: 0.0 },
            north_east: GeoPoint { lng: 2.0, lat: 2.0 },
            acquired_at_unix_ms: 1,
            candidates: vec![MapCandidate {
                id: "building".into(),
                name: name.into(),
                kind: FeatureKind::Building,
                source: "OpenStreetMap".into(),
                confidence: CandidateConfidence::High,
                source_snapshot_id: None,
                points: Vec::new(),
                height_m: None,
                floors: None,
                roof_shape: None,
                tags: BTreeMap::new(),
                review: ReviewDecision::Pending,
            }],
            error: None,
        }
    }

    #[test]
    fn refresh_preserves_review_through_the_ledger() {
        let mut project = reviewed_project();
        FoundationSourceRegistry::ingest(&mut project, snapshot("s1", "old")).unwrap();
        assert!(project.accept_candidate("building"));

        FoundationSourceRegistry::ingest(&mut project, snapshot("s2", "refreshed")).unwrap();
        let refreshed = project
            .candidates
            .iter()
            .find(|candidate| candidate.name == "refreshed")
            .unwrap();
        assert_eq!(refreshed.review, ReviewDecision::Accepted);
        assert_eq!(project.foundation_source_snapshots.len(), 2);
        assert_eq!(project.foundation_review_ledger.len(), 1);
    }
}
