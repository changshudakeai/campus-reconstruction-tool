use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetBundle {
    pub id: String,
    pub osm_snapshot: String,
    pub overture_release: String,
    pub output_schema: String,
    pub classification_rules: String,
    pub assembly_rules: String,
    pub conflation_rules: String,
    pub derivation_rules: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderOutcomeStatus {
    Complete,
    CompleteEmpty,
    Partial,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum FoundationCategory {
    Building,
    Circulation,
    Water,
    Vegetation,
    Sports,
}

impl FoundationCategory {
    pub const ALL: [Self; 5] = [
        Self::Building,
        Self::Circulation,
        Self::Water,
        Self::Vegetation,
        Self::Sports,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceFailure {
    pub code: String,
    pub scope: String,
    pub retryable: bool,
    pub explanation: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderOutcome {
    pub provider: String,
    pub category: FoundationCategory,
    pub tile_id: String,
    pub status: ProviderOutcomeStatus,
    pub pagination_exhausted: bool,
    pub raw_count: u64,
    pub deduplicated_count: u64,
    pub relation_members_complete: bool,
    pub gaps: Vec<String>,
    #[serde(default)]
    pub failure: Option<ServiceFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageReport {
    pub outcomes: Vec<ProviderOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LicenceRecord {
    pub identifier: String,
    pub url: String,
    pub attribution: String,
    pub dataset_release: String,
    pub acquired_at: String,
    #[serde(default)]
    pub upstream_obligations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultChunk {
    pub id: String,
    pub stable_cursor: String,
    pub content_type: String,
    pub content_encoding: String,
    pub sha256: String,
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultManifest {
    pub contract_version: String,
    pub bundle: DatasetBundle,
    pub coverage_report: CoverageReport,
    pub licences: Vec<LicenceRecord>,
    pub chunks: Vec<ResultChunk>,
    pub result_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "coordinates")]
pub enum SourceGeometry {
    Point([f64; 2]),
    MultiPoint(Vec<[f64; 2]>),
    LineString(Vec<[f64; 2]>),
    MultiLineString(Vec<Vec<[f64; 2]>>),
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

impl SourceGeometry {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Point(_) => "Point",
            Self::MultiPoint(_) => "MultiPoint",
            Self::LineString(_) => "LineString",
            Self::MultiLineString(_) => "MultiLineString",
            Self::Polygon(_) => "Polygon",
            Self::MultiPolygon(_) => "MultiPolygon",
        }
    }

    pub fn all_points(&self) -> Vec<[f64; 2]> {
        match self {
            Self::Point(point) => vec![*point],
            Self::MultiPoint(points) | Self::LineString(points) => points.clone(),
            Self::MultiLineString(lines) | Self::Polygon(lines) => {
                lines.iter().flatten().copied().collect()
            }
            Self::MultiPolygon(polygons) => polygons.iter().flatten().flatten().copied().collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompleteRelation {
    pub relation_id: String,
    pub assembly_status: String,
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLineage {
    pub provider: String,
    pub dataset_release: String,
    pub source_record_id: String,
    pub source_record_version: String,
    pub upstream_records: Vec<String>,
    pub acquired_at: String,
    pub original_classification: String,
    #[serde(default)]
    pub relation: Option<CompleteRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinateSemantics {
    pub crs: String,
    pub axis_order: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeSemantics {
    pub dataset_release: String,
    pub acquired_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeometryDerivationRecord {
    pub rule_version: String,
    pub steps: Vec<String>,
    pub source_geometry_sha256: String,
    pub review_geometry_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcquisitionSuggestion {
    pub kind: String,
    pub rule_version: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AttributeDerivation {
    Direct,
    Derived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "attribute")]
pub enum AttributeProvenance {
    #[serde(rename = "height_m")]
    HeightMetres {
        value: f64,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: MetreUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "levels")]
    Levels {
        value: u32,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: LevelUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "width_m")]
    WidthMetres {
        value: f64,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: MetreUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "subtype")]
    Subtype {
        value: String,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: NoUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "name")]
    Name {
        value: String,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: NoUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetreUnit {
    #[serde(rename = "m")]
    Metres,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LevelUnit {
    #[serde(rename = "levels")]
    Levels,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NoUnit {
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceObservation {
    pub id: String,
    pub category: FoundationCategory,
    pub geometry: SourceGeometry,
    pub original_properties: BTreeMap<String, serde_json::Value>,
    pub lineage: SourceLineage,
    pub licence: LicenceRecord,
    pub coordinate_semantics: CoordinateSemantics,
    pub unit_semantics: BTreeMap<String, String>,
    pub time_semantics: TimeSemantics,
    pub geometry_sha256: String,
    pub derivation: GeometryDerivationRecord,
    pub review_geometry_proposal: SourceGeometry,
    pub raw_spatial_measures: BTreeMap<String, f64>,
    pub suggestions: Vec<AcquisitionSuggestion>,
    pub attribute_provenance: Vec<AttributeProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundaryRankingEvidence {
    pub name_match: f64,
    pub distance_m: f64,
    pub contains_anchor: bool,
    pub area_m2: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundaryCandidate {
    pub id: String,
    pub rank: u32,
    pub geometry: SourceGeometry,
    pub lineage: SourceLineage,
    pub licence: LicenceRecord,
    pub ranking_evidence: BoundaryRankingEvidence,
}
