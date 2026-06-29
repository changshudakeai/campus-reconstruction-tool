use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PROJECT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DesktopMode {
    #[default]
    Foundation,
    Detailed,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FoundationStep {
    #[default]
    Campus,
    Boundary,
    Orientation,
    Building,
    Road,
    Water,
    Vegetation,
    Sports,
    Export,
}

impl FoundationStep {
    pub const ALL: [Self; 9] = [
        Self::Campus,
        Self::Boundary,
        Self::Orientation,
        Self::Building,
        Self::Road,
        Self::Water,
        Self::Vegetation,
        Self::Sports,
        Self::Export,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Campus => "校区",
            Self::Boundary => "边界",
            Self::Orientation => "朝向",
            Self::Building => "建筑",
            Self::Road => "道路",
            Self::Water => "水域",
            Self::Vegetation => "植被",
            Self::Sports => "体育",
            Self::Export => "导出",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|step| *step == self).unwrap_or(0);
        Self::ALL[(index + 1).min(Self::ALL.len() - 1)]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    Building,
    Road,
    Water,
    Vegetation,
    Sports,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FoundationStylePreset {
    #[default]
    ArnisClassic,
    ModernCampus,
    HistoricRedBrick,
    LightweightDraft,
}

impl FoundationStylePreset {
    pub const ALL: [Self; 4] = [
        Self::ArnisClassic,
        Self::ModernCampus,
        Self::HistoricRedBrick,
        Self::LightweightDraft,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ArnisClassic => "Arnis Classic",
            Self::ModernCampus => "现代校园",
            Self::HistoricRedBrick => "历史红砖校园",
            Self::LightweightDraft => "轻量草稿",
        }
    }

    pub fn block(self, kind: FeatureKind) -> &'static str {
        match (self, kind) {
            (Self::ArnisClassic, FeatureKind::Building) => "minecraft:quartz_block",
            (Self::ArnisClassic, FeatureKind::Road) => "minecraft:gray_concrete",
            (Self::ArnisClassic, FeatureKind::Water) => "minecraft:water",
            (Self::ArnisClassic, FeatureKind::Vegetation) => "minecraft:moss_block",
            (Self::ArnisClassic, FeatureKind::Sports) => "minecraft:green_concrete",
            (Self::ModernCampus, FeatureKind::Building) => "minecraft:smooth_quartz",
            (Self::ModernCampus, FeatureKind::Road) => "minecraft:light_gray_concrete",
            (Self::ModernCampus, FeatureKind::Water) => "minecraft:water",
            (Self::ModernCampus, FeatureKind::Vegetation) => "minecraft:birch_leaves",
            (Self::ModernCampus, FeatureKind::Sports) => "minecraft:green_concrete",
            (Self::HistoricRedBrick, FeatureKind::Building) => "minecraft:bricks",
            (Self::HistoricRedBrick, FeatureKind::Road) => "minecraft:stone_bricks",
            (Self::HistoricRedBrick, FeatureKind::Water) => "minecraft:water",
            (Self::HistoricRedBrick, FeatureKind::Vegetation) => "minecraft:dark_oak_leaves",
            (Self::HistoricRedBrick, FeatureKind::Sports) => "minecraft:terracotta",
            (Self::LightweightDraft, FeatureKind::Building) => "minecraft:stone",
            (Self::LightweightDraft, FeatureKind::Road) => "minecraft:gray_concrete",
            (Self::LightweightDraft, FeatureKind::Water) => "minecraft:water",
            (Self::LightweightDraft, FeatureKind::Vegetation) => "minecraft:moss_block",
            (Self::LightweightDraft, FeatureKind::Sports) => "minecraft:green_concrete",
        }
    }

    pub fn road_width_blocks(self) -> i32 {
        match self {
            Self::ArnisClassic | Self::ModernCampus => 4,
            Self::HistoricRedBrick => 3,
            Self::LightweightDraft => 2,
        }
    }

    fn from_legacy_id(id: &str) -> Self {
        match id {
            "arnis:modern-campus/v1" => Self::ModernCampus,
            "arnis:historic-red-brick/v1" => Self::HistoricRedBrick,
            "arnis:lightweight-draft/v1" => Self::LightweightDraft,
            _ => Self::ArnisClassic,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CandidateConfidenceFilter {
    #[default]
    All,
    High,
    Medium,
    Low,
    Confirmed,
    Rejected,
}

impl CandidateConfidenceFilter {
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::High,
        Self::Medium,
        Self::Low,
        Self::Confirmed,
        Self::Rejected,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部待审核",
            Self::High => "高置信度",
            Self::Medium => "中置信度",
            Self::Low => "低置信度",
            Self::Confirmed => "已确认",
            Self::Rejected => "已拒绝",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeoPoint {
    pub lng: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapViewState {
    pub center: GeoPoint,
    pub zoom: f64,
    pub pitch: f64,
    pub rotation: f64,
    pub capture_bounds: Option<[GeoPoint; 2]>,
}

impl Default for MapViewState {
    fn default() -> Self {
        Self {
            center: GeoPoint {
                lng: 121.406_582,
                lat: 31.228_318,
            },
            zoom: 17.0,
            pitch: 45.0,
            rotation: 0.0,
            capture_bounds: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapCandidate {
    pub id: String,
    pub name: String,
    pub kind: FeatureKind,
    pub source: String,
    pub confidence: String,
    pub points: Vec<GeoPoint>,
    #[serde(default)]
    pub height_m: Option<f64>,
    #[serde(default)]
    pub floors: Option<u32>,
    #[serde(default)]
    pub roof_shape: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub review: ReviewDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapFeature {
    pub id: String,
    pub name: String,
    pub kind: FeatureKind,
    pub points: Vec<GeoPoint>,
    pub block: String,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuildingDirectoryRecord {
    pub source_id: String,
    pub name: String,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuildingSuppression {
    pub source_id: String,
    pub reason: String,
    pub suppressed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildingSlot {
    pub id: String,
    pub name: String,
    pub footprint: Vec<GeoPoint>,
    pub height_m: Option<f64>,
    pub floors: Option<u32>,
    #[serde(default)]
    pub roof_shape: Option<String>,
    pub refined: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefinementStatus {
    Draft,
    Confirmed,
    Archived,
}

impl RefinementStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "草稿",
            Self::Confirmed => "已确认",
            Self::Archived => "已归档",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildingRefinement {
    pub id: String,
    pub slot_id: String,
    pub version: u32,
    pub status: RefinementStatus,
    pub generated_path: PathBuf,
    pub style_preset: ArnisStylePreset,
    pub wall_block: Option<String>,
    pub window_density: u8,
    pub wall_depth: u8,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewBlockSelection {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFeatureKind {
    EntranceEmphasis,
    WindowBand,
    RoofRidge,
    Cornice,
    Frame,
}

impl SemanticFeatureKind {
    pub const ALL: [Self; 5] = [
        Self::EntranceEmphasis,
        Self::WindowBand,
        Self::RoofRidge,
        Self::Cornice,
        Self::Frame,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::EntranceEmphasis => "入口强调",
            Self::WindowBand => "连续窗带",
            Self::RoofRidge => "屋脊",
            Self::Cornice => "檐口",
            Self::Frame => "立面框架",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFeatureSide {
    North,
    South,
    East,
    West,
    Center,
}

impl SemanticFeatureSide {
    pub const ALL: [Self; 5] = [
        Self::North,
        Self::South,
        Self::East,
        Self::West,
        Self::Center,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::North => "北侧",
            Self::South => "南侧",
            Self::East => "东侧",
            Self::West => "西侧",
            Self::Center => "中心",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticHeightBand {
    Lower,
    Middle,
    Upper,
    Roof,
}

impl SemanticHeightBand {
    pub const ALL: [Self; 4] = [Self::Lower, Self::Middle, Self::Upper, Self::Roof];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lower => "下部",
            Self::Middle => "中部",
            Self::Upper => "上部",
            Self::Roof => "屋顶",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticStrength {
    Subtle,
    Visible,
    Strong,
}

impl SemanticStrength {
    pub const ALL: [Self; 3] = [Self::Subtle, Self::Visible, Self::Strong];

    pub fn label(self) -> &'static str {
        match self {
            Self::Subtle => "轻微",
            Self::Visible => "清晰可见",
            Self::Strong => "强强调",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticFeatureRecord {
    pub id: String,
    pub slot_id: String,
    pub refinement_id: String,
    pub kind: SemanticFeatureKind,
    pub label: String,
    pub side: SemanticFeatureSide,
    pub height_band: SemanticHeightBand,
    pub strength: SemanticStrength,
    pub reason: String,
    pub affected_blocks: usize,
    pub block: String,
    pub applied_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SemanticFeatureDraft {
    pub kind: SemanticFeatureKind,
    pub label: String,
    pub side: SemanticFeatureSide,
    pub height_band: SemanticHeightBand,
    pub strength: SemanticStrength,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalModelEligibility {
    Eligible,
    Blocked,
}

impl ExternalModelEligibility {
    pub fn label(self) -> &'static str {
        match self {
            Self::Eligible => "许可允许适配",
            Self::Blocked => "许可阻止适配",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalModelDecision {
    #[default]
    Pending,
    EligiblePrimary,
    SupportingEvidence,
    Rejected,
}

impl ExternalModelDecision {
    pub const ALL: [Self; 4] = [
        Self::Pending,
        Self::EligiblePrimary,
        Self::SupportingEvidence,
        Self::Rejected,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "待审核",
            Self::EligiblePrimary => "采用为主几何",
            Self::SupportingEvidence => "仅作辅助证据",
            Self::Rejected => "拒绝",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalModelReview {
    pub id: String,
    pub slot_id: String,
    pub title: String,
    pub source: String,
    pub source_url: String,
    pub author: String,
    pub license_name: Option<String>,
    pub eligibility: ExternalModelEligibility,
    pub decision: ExternalModelDecision,
    pub decision_reason: String,
    pub reviewed_at_unix_ms: Option<u64>,
    pub width_m: Option<f64>,
    pub height_m: Option<f64>,
    pub length_m: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceConflictDecision {
    #[default]
    Unresolved,
    PrimarySelected,
    SupportingOnly,
    Rejected,
}

impl SourceConflictDecision {
    pub const ALL: [Self; 4] = [
        Self::Unresolved,
        Self::PrimarySelected,
        Self::SupportingOnly,
        Self::Rejected,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unresolved => "未解决",
            Self::PrimarySelected => "选择主来源",
            Self::SupportingOnly => "仅作辅助",
            Self::Rejected => "拒绝冲突来源",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceConflictReview {
    pub id: String,
    pub slot_id: String,
    pub external_model_id: String,
    pub kind: String,
    pub severity: String,
    pub summary: String,
    pub decision: SourceConflictDecision,
    pub decision_reason: String,
    pub decided_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArnisStylePreset {
    House,
    Residential,
    Farm,
    Commercial,
    Office,
    Hotel,
    Industrial,
    Warehouse,
    #[default]
    School,
    Hospital,
    Religious,
    Historic,
    Tower,
    Garage,
    Shed,
    Greenhouse,
    TallBuilding,
    GlassySkyscraper,
    ModernSkyscraper,
}

impl ArnisStylePreset {
    pub const ALL: [Self; 19] = [
        Self::House,
        Self::Residential,
        Self::Farm,
        Self::Commercial,
        Self::Office,
        Self::Hotel,
        Self::Industrial,
        Self::Warehouse,
        Self::School,
        Self::Hospital,
        Self::Religious,
        Self::Historic,
        Self::Tower,
        Self::Garage,
        Self::Shed,
        Self::Greenhouse,
        Self::TallBuilding,
        Self::GlassySkyscraper,
        Self::ModernSkyscraper,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::House => "独栋住宅",
            Self::Residential => "宿舍与住宅",
            Self::Farm => "农用建筑",
            Self::Commercial => "商业建筑",
            Self::Office => "办公建筑",
            Self::Hotel => "酒店建筑",
            Self::Industrial => "工业建筑",
            Self::Warehouse => "仓储建筑",
            Self::School => "校园与公共建筑",
            Self::Hospital => "医疗建筑",
            Self::Religious => "宗教建筑",
            Self::Historic => "历史建筑",
            Self::Tower => "塔楼",
            Self::Garage => "车库",
            Self::Shed => "棚屋",
            Self::Greenhouse => "温室",
            Self::TallBuilding => "高层建筑",
            Self::GlassySkyscraper => "玻璃幕墙高层",
            Self::ModernSkyscraper => "现代高层",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::House => "house",
            Self::Residential => "residential",
            Self::Farm => "farm",
            Self::Commercial => "commercial",
            Self::Office => "office",
            Self::Hotel => "hotel",
            Self::Industrial => "industrial",
            Self::Warehouse => "warehouse",
            Self::School => "school",
            Self::Hospital => "hospital",
            Self::Religious => "religious",
            Self::Historic => "historic",
            Self::Tower => "tower",
            Self::Garage => "garage",
            Self::Shed => "shed",
            Self::Greenhouse => "greenhouse",
            Self::TallBuilding => "tall_building",
            Self::GlassySkyscraper => "glassy_skyscraper",
            Self::ModernSkyscraper => "modern_skyscraper",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetailedBuildingState {
    pub selected_slot_id: Option<String>,
    pub style_preset: ArnisStylePreset,
    pub wall_block: Option<String>,
    pub window_density: u8,
    pub wall_depth: u8,
    pub generated_path: Option<PathBuf>,
    #[serde(default)]
    pub refinements: Vec<BuildingRefinement>,
    #[serde(default)]
    pub semantic_features: Vec<SemanticFeatureRecord>,
    #[serde(default)]
    pub external_models: Vec<ExternalModelReview>,
    #[serde(default)]
    pub source_conflicts: Vec<SourceConflictReview>,
}

impl Default for DetailedBuildingState {
    fn default() -> Self {
        Self {
            selected_slot_id: None,
            style_preset: ArnisStylePreset::School,
            wall_block: None,
            window_density: 50,
            wall_depth: 50,
            generated_path: None,
            refinements: Vec::new(),
            semantic_features: Vec::new(),
            external_models: Vec::new(),
            source_conflicts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CampusProject {
    pub schema_version: u32,
    pub name: String,
    pub campus_name: String,
    pub mode: DesktopMode,
    #[serde(default)]
    pub foundation_step: FoundationStep,
    #[serde(default)]
    pub completed_steps: Vec<FoundationStep>,
    #[serde(default)]
    pub boundary: Vec<GeoPoint>,
    #[serde(default)]
    pub orientation_degrees: f64,
    #[serde(default = "default_scale")]
    pub blocks_per_meter: f64,
    #[serde(default)]
    pub map_view: MapViewState,
    #[serde(default)]
    pub candidates: Vec<MapCandidate>,
    #[serde(default)]
    pub features: Vec<MapFeature>,
    #[serde(default)]
    pub building_slots: Vec<BuildingSlot>,
    #[serde(default)]
    pub building_directory: Vec<BuildingDirectoryRecord>,
    #[serde(default)]
    pub building_suppressions: Vec<BuildingSuppression>,
    #[serde(default)]
    pub foundation_style_preset: FoundationStylePreset,
    #[serde(default)]
    pub foundation_preview_path: Option<PathBuf>,
    #[serde(default)]
    pub visual_capture_path: Option<PathBuf>,
    #[serde(default)]
    pub detailed: DetailedBuildingState,
}

fn default_scale() -> f64 {
    1.0
}

impl CampusProject {
    pub fn new(name: impl Into<String>, campus_name: impl Into<String>) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            name: name.into(),
            campus_name: campus_name.into(),
            mode: DesktopMode::Foundation,
            foundation_step: FoundationStep::Campus,
            completed_steps: Vec::new(),
            boundary: Vec::new(),
            orientation_degrees: 0.0,
            blocks_per_meter: 1.0,
            map_view: MapViewState::default(),
            candidates: Vec::new(),
            features: Vec::new(),
            building_slots: Vec::new(),
            building_directory: Vec::new(),
            building_suppressions: Vec::new(),
            foundation_style_preset: FoundationStylePreset::ArnisClassic,
            foundation_preview_path: None,
            visual_capture_path: None,
            detailed: DetailedBuildingState::default(),
        }
    }

    pub fn accept_candidate(&mut self, id: &str) -> bool {
        let Some(candidate) = self.candidates.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        candidate.review = ReviewDecision::Accepted;
        if !self
            .features
            .iter()
            .any(|feature| feature.source_id.as_deref() == Some(id))
        {
            self.features.push(MapFeature {
                id: format!("accepted-{}", candidate.id),
                name: candidate.name.clone(),
                kind: candidate.kind,
                points: candidate.points.clone(),
                block: self
                    .foundation_style_preset
                    .block(candidate.kind)
                    .to_string(),
                source_id: Some(candidate.id.clone()),
            });
        }
        if candidate.kind == FeatureKind::Building
            && !self
                .building_slots
                .iter()
                .any(|slot| slot.id == candidate.id)
        {
            self.building_slots.push(BuildingSlot {
                id: candidate.id.clone(),
                name: candidate.name.clone(),
                footprint: candidate.points.clone(),
                height_m: candidate.height_m,
                floors: candidate.floors,
                roof_shape: candidate.roof_shape.clone(),
                refined: false,
            });
            let source_id = candidate.id.clone();
            let name = candidate.name.clone();
            self.building_directory
                .retain(|record| record.source_id != source_id);
            self.building_directory.push(BuildingDirectoryRecord {
                source_id,
                name,
                updated_at_unix_ms: now_unix_ms(),
            });
        }
        true
    }

    pub fn apply_foundation_style(&mut self, preset: FoundationStylePreset) {
        self.foundation_style_preset = preset;
        self.foundation_preview_path = None;
        for feature in &mut self.features {
            feature.block = preset.block(feature.kind).to_string();
        }
    }

    pub fn next_refinement_version(&self, slot_id: &str) -> u32 {
        self.detailed
            .refinements
            .iter()
            .filter(|refinement| refinement.slot_id == slot_id)
            .map(|refinement| refinement.version)
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn record_refinement_draft(
        &mut self,
        slot_id: &str,
        version: u32,
        generated_path: PathBuf,
    ) {
        for refinement in self.detailed.refinements.iter_mut().filter(|refinement| {
            refinement.slot_id == slot_id && refinement.status == RefinementStatus::Draft
        }) {
            refinement.status = RefinementStatus::Archived;
        }
        let created_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.detailed.refinements.push(BuildingRefinement {
            id: format!("{slot_id}:v{version}"),
            slot_id: slot_id.to_string(),
            version,
            status: RefinementStatus::Draft,
            generated_path: generated_path.clone(),
            style_preset: self.detailed.style_preset,
            wall_block: self.detailed.wall_block.clone(),
            window_density: self.detailed.window_density,
            wall_depth: self.detailed.wall_depth,
            created_at_unix_ms,
        });
        self.detailed.generated_path = Some(generated_path);
    }

    pub fn latest_refinement(&self, slot_id: &str) -> Option<&BuildingRefinement> {
        self.detailed
            .refinements
            .iter()
            .filter(|refinement| refinement.slot_id == slot_id)
            .max_by_key(|refinement| refinement.version)
    }

    pub fn confirm_latest_refinement(&mut self, slot_id: &str) -> Option<u32> {
        let index = self
            .detailed
            .refinements
            .iter()
            .enumerate()
            .filter(|(_, refinement)| refinement.slot_id == slot_id)
            .max_by_key(|(_, refinement)| refinement.version)
            .map(|(index, _)| index)?;
        for refinement in self.detailed.refinements.iter_mut().filter(|refinement| {
            refinement.slot_id == slot_id && refinement.status == RefinementStatus::Confirmed
        }) {
            refinement.status = RefinementStatus::Archived;
        }
        self.detailed.refinements[index].status = RefinementStatus::Confirmed;
        if let Some(slot) = self
            .building_slots
            .iter_mut()
            .find(|slot| slot.id == slot_id)
        {
            slot.refined = true;
        }
        Some(self.detailed.refinements[index].version)
    }

    pub fn record_semantic_feature(
        &mut self,
        slot_id: &str,
        refinement_id: &str,
        draft: SemanticFeatureDraft,
        affected_blocks: usize,
        block: String,
    ) {
        let applied_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.detailed.semantic_features.push(SemanticFeatureRecord {
            id: format!(
                "{refinement_id}:semantic:{}",
                self.detailed.semantic_features.len() + 1
            ),
            slot_id: slot_id.to_string(),
            refinement_id: refinement_id.to_string(),
            kind: draft.kind,
            label: draft.label,
            side: draft.side,
            height_band: draft.height_band,
            strength: draft.strength,
            reason: draft.reason,
            affected_blocks,
            block,
            applied_at_unix_ms,
        });
    }

    pub fn discover_external_models_for_slot(&mut self, slot_id: &str) {
        let Some(slot) = self
            .building_slots
            .iter()
            .find(|slot| slot.id == slot_id)
            .cloned()
        else {
            return;
        };
        let Some(candidate) = self
            .candidates
            .iter()
            .find(|candidate| candidate.id == slot_id)
            .cloned()
        else {
            return;
        };
        let (observed_width, observed_length) = footprint_dimensions_m(&slot.footprint);
        let mut discovered = Vec::new();
        for key in ["3dmr", "3dmr:id", "model:3dmr", "model", "3d_model"] {
            let Some(value) = candidate
                .tags
                .get(key)
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let id = format!("{}:3dmr:{}", candidate.id, slug(value));
            if discovered
                .iter()
                .any(|review: &ExternalModelReview| review.id == id)
            {
                continue;
            }
            discovered.push(external_model_from_tags(
                &candidate,
                &slot,
                id,
                "3DMR",
                value,
                observed_width,
                observed_length,
            ));
        }
        if let Some(value) = candidate
            .tags
            .get("wikidata")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            discovered.push(external_model_from_tags(
                &candidate,
                &slot,
                format!("{}:wikidata:{}", candidate.id, slug(value)),
                "Wikidata",
                value,
                observed_width,
                observed_length,
            ));
        }
        for review in discovered {
            if self
                .detailed
                .external_models
                .iter()
                .any(|existing| existing.id == review.id)
            {
                continue;
            }
            let mut conflicts = conflicts_for_external_model(&slot, &review);
            self.detailed.external_models.push(review);
            for conflict in conflicts.drain(..) {
                if !self
                    .detailed
                    .source_conflicts
                    .iter()
                    .any(|existing| existing.id == conflict.id)
                {
                    self.detailed.source_conflicts.push(conflict);
                }
            }
        }
    }

    pub fn review_external_model(
        &mut self,
        id: &str,
        decision: ExternalModelDecision,
        reason: &str,
    ) -> Result<(), String> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("外部模型审核需要理由".into());
        }
        let review = self
            .detailed
            .external_models
            .iter_mut()
            .find(|review| review.id == id)
            .ok_or("外部模型候选不存在")?;
        if decision == ExternalModelDecision::EligiblePrimary
            && review.eligibility != ExternalModelEligibility::Eligible
        {
            return Err("许可不明确或阻止适配的模型不能作为主几何".into());
        }
        if decision == ExternalModelDecision::EligiblePrimary
            && review
                .license_name
                .as_deref()
                .is_some_and(|license| license.to_ascii_uppercase().contains("BY"))
            && review.author.trim().is_empty()
        {
            return Err("需要署名的外部模型必须记录作者".into());
        }
        review.decision = decision;
        review.decision_reason = reason.to_string();
        review.reviewed_at_unix_ms = Some(now_unix_ms());
        Ok(())
    }

    pub fn review_source_conflict(
        &mut self,
        id: &str,
        decision: SourceConflictDecision,
        reason: &str,
    ) -> Result<(), String> {
        if decision == SourceConflictDecision::Unresolved {
            return Err("请选择明确的来源冲突决策".into());
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("来源冲突决策需要理由".into());
        }
        let conflict = self
            .detailed
            .source_conflicts
            .iter_mut()
            .find(|conflict| conflict.id == id)
            .ok_or("来源冲突不存在")?;
        conflict.decision = decision;
        conflict.decision_reason = reason.to_string();
        conflict.decided_at_unix_ms = Some(now_unix_ms());
        Ok(())
    }

    pub fn reject_candidate(&mut self, id: &str) -> bool {
        let Some(candidate) = self.candidates.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        candidate.review = ReviewDecision::Rejected;
        self.features
            .retain(|feature| feature.source_id.as_deref() != Some(id));
        self.building_slots.retain(|slot| slot.id != id);
        true
    }

    pub fn reset_candidate_review(&mut self, id: &str) -> bool {
        let Some(candidate) = self.candidates.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        candidate.review = ReviewDecision::Pending;
        self.features
            .retain(|feature| feature.source_id.as_deref() != Some(id));
        self.building_slots.retain(|slot| slot.id != id);
        true
    }

    pub fn add_manual_feature(
        &mut self,
        kind: FeatureKind,
        points: Vec<GeoPoint>,
    ) -> Result<String, String> {
        let minimum = if kind == FeatureKind::Road { 2 } else { 3 };
        if points.len() < minimum {
            return Err(format!("手绘地物至少需要 {minimum} 个节点"));
        }
        let id = format!(
            "manual:{}:{}",
            match kind {
                FeatureKind::Building => "building",
                FeatureKind::Road => "road",
                FeatureKind::Water => "water",
                FeatureKind::Vegetation => "vegetation",
                FeatureKind::Sports => "sports",
            },
            now_unix_ms()
        );
        let name = format!(
            "手绘{} {}",
            match kind {
                FeatureKind::Building => "建筑",
                FeatureKind::Road => "道路",
                FeatureKind::Water => "水域",
                FeatureKind::Vegetation => "植被",
                FeatureKind::Sports => "体育设施",
            },
            self.features
                .iter()
                .filter(|feature| feature.kind == kind && feature.source_id.is_none())
                .count()
                + 1
        );
        self.features.push(MapFeature {
            id: id.clone(),
            name: name.clone(),
            kind,
            points: points.clone(),
            block: self.foundation_style_preset.block(kind).into(),
            source_id: None,
        });
        if kind == FeatureKind::Building {
            self.building_slots.push(BuildingSlot {
                id: id.clone(),
                name,
                footprint: points,
                height_m: None,
                floors: None,
                roof_shape: None,
                refined: false,
            });
        }
        Ok(id)
    }

    pub fn rename_building(&mut self, source_id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("建筑名称不能为空".into());
        }
        let candidate = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.id == source_id)
            .ok_or("建筑来源对象不存在")?;
        if candidate.kind != FeatureKind::Building {
            return Err("只有建筑候选可以进入建筑目录".into());
        }
        candidate.name = name.to_string();
        for feature in self
            .features
            .iter_mut()
            .filter(|feature| feature.source_id.as_deref() == Some(source_id))
        {
            feature.name = name.to_string();
        }
        for slot in self
            .building_slots
            .iter_mut()
            .filter(|slot| slot.id == source_id)
        {
            slot.name = name.to_string();
        }
        self.building_directory
            .retain(|record| record.source_id != source_id);
        self.building_directory.push(BuildingDirectoryRecord {
            source_id: source_id.to_string(),
            name: name.to_string(),
            updated_at_unix_ms: now_unix_ms(),
        });
        Ok(())
    }

    pub fn suppress_building(&mut self, source_id: &str, reason: &str) -> Result<(), String> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("抑制建筑需要理由".into());
        }
        let candidate = self
            .candidates
            .iter()
            .find(|candidate| candidate.id == source_id)
            .ok_or("建筑来源对象不存在")?;
        if candidate.kind != FeatureKind::Building {
            return Err("只有建筑来源对象可以被持久抑制".into());
        }
        self.building_suppressions
            .retain(|record| record.source_id != source_id);
        self.building_suppressions.push(BuildingSuppression {
            source_id: source_id.to_string(),
            reason: reason.to_string(),
            suppressed_at_unix_ms: now_unix_ms(),
        });
        self.building_directory
            .retain(|record| record.source_id != source_id);
        self.candidates
            .retain(|candidate| candidate.id != source_id);
        self.features
            .retain(|feature| feature.source_id.as_deref() != Some(source_id));
        self.building_slots.retain(|slot| slot.id != source_id);
        Ok(())
    }

    pub fn restore_building_suppression(&mut self, source_id: &str) -> bool {
        let before = self.building_suppressions.len();
        self.building_suppressions
            .retain(|record| record.source_id != source_id);
        self.building_suppressions.len() != before
    }

    pub fn confirm_step(&mut self) {
        if !self.completed_steps.contains(&self.foundation_step) {
            self.completed_steps.push(self.foundation_step);
        }
        self.foundation_step = self.foundation_step.next();
    }
}

fn external_model_from_tags(
    candidate: &MapCandidate,
    slot: &BuildingSlot,
    id: String,
    source: &str,
    value: &str,
    observed_width: f64,
    observed_length: f64,
) -> ExternalModelReview {
    let namespace = if source == "Wikidata" {
        "wikidata"
    } else {
        "3dmr"
    };
    let license_name = candidate
        .tags
        .get(&format!("{namespace}:license"))
        .or_else(|| candidate.tags.get("model:license"))
        .or_else(|| candidate.tags.get("license"))
        .cloned();
    let eligibility = if license_name
        .as_deref()
        .is_some_and(license_allows_adaptation)
    {
        ExternalModelEligibility::Eligible
    } else {
        ExternalModelEligibility::Blocked
    };
    let source_url = candidate
        .tags
        .get(&format!("{namespace}:url"))
        .or_else(|| candidate.tags.get("model:url"))
        .cloned()
        .unwrap_or_else(|| {
            if value.starts_with("http://") || value.starts_with("https://") {
                value.to_string()
            } else if source == "Wikidata" {
                format!("https://www.wikidata.org/wiki/{value}")
            } else {
                format!("https://3dmr.eu/models/{value}")
            }
        });
    ExternalModelReview {
        id,
        slot_id: slot.id.clone(),
        title: candidate
            .tags
            .get(&format!("{namespace}:title"))
            .or_else(|| candidate.tags.get("model:title"))
            .cloned()
            .unwrap_or_else(|| candidate.name.clone()),
        source: source.into(),
        source_url,
        author: candidate
            .tags
            .get(&format!("{namespace}:author"))
            .or_else(|| candidate.tags.get("model:author"))
            .or_else(|| candidate.tags.get("author"))
            .cloned()
            .unwrap_or_default(),
        license_name,
        eligibility,
        decision: ExternalModelDecision::Pending,
        decision_reason: String::new(),
        reviewed_at_unix_ms: None,
        width_m: tag_number(&candidate.tags, &["model:width", "3dmr:width"])
            .or(Some(observed_width)),
        height_m: tag_number(&candidate.tags, &["model:height", "3dmr:height"]).or(slot.height_m),
        length_m: tag_number(&candidate.tags, &["model:length", "3dmr:length"])
            .or(Some(observed_length)),
    }
}

fn conflicts_for_external_model(
    slot: &BuildingSlot,
    model: &ExternalModelReview,
) -> Vec<SourceConflictReview> {
    let mut result = Vec::new();
    if model.eligibility == ExternalModelEligibility::Blocked {
        result.push(SourceConflictReview {
            id: format!("license:{}", model.id),
            slot_id: slot.id.clone(),
            external_model_id: model.id.clone(),
            kind: "license_blocked".into(),
            severity: "blocking".into(),
            summary: "外部模型缺少明确的可改编许可，不能进入最终 schematic。".into(),
            decision: SourceConflictDecision::Unresolved,
            decision_reason: String::new(),
            decided_at_unix_ms: None,
        });
    }
    let (observed_width, observed_length) = footprint_dimensions_m(&slot.footprint);
    let deltas = [
        model
            .width_m
            .map(|value| percent_delta(value, observed_width)),
        model
            .length_m
            .map(|value| percent_delta(value, observed_length)),
        model
            .height_m
            .zip(slot.height_m)
            .map(|(value, observed)| percent_delta(value, observed)),
    ];
    let max_delta = deltas.into_iter().flatten().fold(0.0, f64::max);
    if max_delta > 15.0 {
        result.push(SourceConflictReview {
            id: format!("dimensions:{}", model.id),
            slot_id: slot.id.clone(),
            external_model_id: model.id.clone(),
            kind: "dimension_mismatch".into(),
            severity: if max_delta > 30.0 {
                "blocking".into()
            } else {
                "warning".into()
            },
            summary: format!("外部模型尺寸与已审核观测证据最多相差 {max_delta:.1}%。"),
            decision: SourceConflictDecision::Unresolved,
            decision_reason: String::new(),
            decided_at_unix_ms: None,
        });
    }
    result
}

fn tag_number(tags: &BTreeMap<String, String>, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| tags.get(*key)?.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn license_allows_adaptation(license: &str) -> bool {
    let normalized = license.to_ascii_lowercase();
    !normalized.contains("-nd")
        && !normalized.contains(" no derivatives")
        && [
            "cc0",
            "cc-by",
            "cc by",
            "odbl",
            "mit",
            "apache",
            "bsd",
            "public domain",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn footprint_dimensions_m(points: &[GeoPoint]) -> (f64, f64) {
    if points.is_empty() {
        return (0.0, 0.0);
    }
    let min_lng = points
        .iter()
        .map(|point| point.lng)
        .fold(f64::INFINITY, f64::min);
    let max_lng = points
        .iter()
        .map(|point| point.lng)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_lat = points
        .iter()
        .map(|point| point.lat)
        .fold(f64::INFINITY, f64::min);
    let max_lat = points
        .iter()
        .map(|point| point.lat)
        .fold(f64::NEG_INFINITY, f64::max);
    let center_lat = ((min_lat + max_lat) / 2.0).to_radians();
    (
        (max_lng - min_lng).abs() * 111_320.0 * center_lat.cos(),
        (max_lat - min_lat).abs() * 111_320.0,
    )
}

fn percent_delta(value: f64, observed: f64) -> f64 {
    if observed <= 0.0 {
        0.0
    } else {
        (value - observed).abs() / observed * 100.0
    }
}

fn slug(value: &str) -> String {
    let slug = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        "model".into()
    } else {
        slug
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn default_block(kind: FeatureKind) -> &'static str {
    FoundationStylePreset::ArnisClassic.block(kind)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DesktopApplicationState {
    pub project: Option<CampusProject>,
    pub project_path: Option<PathBuf>,
    pub dirty: bool,
    pub last_error: Option<String>,
    pub candidate_filter: CandidateConfidenceFilter,
    pub candidate_page: usize,
    pub tool_status: Option<String>,
    pub selected_preview_block: Option<PreviewBlockSelection>,
    pub active_preview_path: Option<PathBuf>,
    pub selected_external_model: usize,
    pub selected_source_conflict: usize,
    pub selected_candidate_id: Option<String>,
    pub selected_suppression: usize,
    undo_stack: Vec<CampusProject>,
    redo_stack: Vec<CampusProject>,
}

impl DesktopApplicationState {
    pub fn new_project(&mut self, name: impl Into<String>, campus: impl Into<String>) {
        self.project = Some(CampusProject::new(name, campus));
        self.project_path = None;
        self.dirty = true;
        self.last_error = None;
        self.candidate_filter = CandidateConfidenceFilter::All;
        self.candidate_page = 0;
        self.tool_status = None;
        self.selected_preview_block = None;
        self.active_preview_path = None;
        self.selected_external_model = 0;
        self.selected_source_conflict = 0;
        self.selected_candidate_id = None;
        self.selected_suppression = 0;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn set_mode(&mut self, mode: DesktopMode) {
        self.snapshot_for_undo();
        if let Some(project) = &mut self.project {
            project.mode = mode;
            self.dirty = true;
        }
    }

    pub fn mutate_project(&mut self, mutation: impl FnOnce(&mut CampusProject)) {
        self.snapshot_for_undo();
        if let Some(project) = &mut self.project {
            mutation(project);
            self.dirty = true;
        }
    }

    fn snapshot_for_undo(&mut self) {
        if let Some(project) = &self.project {
            self.undo_stack.push(project.clone());
            if self.undo_stack.len() > 100 {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        if let Some(current) = self.project.replace(previous) {
            self.redo_stack.push(current);
        }
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        if let Some(current) = self.project.replace(next) {
            self.undo_stack.push(current);
        }
        self.dirty = true;
        true
    }

    pub fn save(&mut self) -> Result<(), String> {
        let path = self
            .project_path
            .clone()
            .ok_or("Project has not been named on disk")?;
        self.save_to(path)
    }

    pub fn save_to(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let project = self.project.as_ref().ok_or("No project is open")?;
        let path = path.as_ref();
        let parent = path.parent().ok_or("Project path has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(project).map_err(|error| error.to_string())?;
        let recovery = path.with_extension("campus.recovery.json");
        if path.exists() {
            fs::copy(path, &recovery).map_err(|error| error.to_string())?;
        }
        atomicwrites::AtomicFile::new(path, atomicwrites::AllowOverwrite)
            .write(|file| file.write_all(&bytes))
            .map_err(|error| error.to_string())?;
        self.project_path = Some(path.to_path_buf());
        self.dirty = false;
        self.last_error = None;
        Ok(())
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        let project = match serde_json::from_slice::<CampusProject>(&bytes) {
            Ok(project) => project,
            Err(native_error) => {
                let value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
                legacy_project_from_value(&value).map_err(|legacy_error| {
                    format!("无法读取项目；原生格式：{native_error}；旧版格式：{legacy_error}")
                })?
            }
        };
        if project.schema_version > PROJECT_SCHEMA_VERSION {
            return Err(format!(
                "Project schema {} is newer than supported schema {}",
                project.schema_version, PROJECT_SCHEMA_VERSION
            ));
        }
        self.project = Some(project);
        self.project_path = Some(path.to_path_buf());
        self.dirty = false;
        self.last_error = None;
        self.candidate_filter = CandidateConfidenceFilter::All;
        self.candidate_page = 0;
        self.tool_status = None;
        self.selected_preview_block = None;
        self.active_preview_path = None;
        self.selected_external_model = 0;
        self.selected_source_conflict = 0;
        self.selected_candidate_id = None;
        self.selected_suppression = 0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        Ok(())
    }
}

fn legacy_project_from_value(value: &serde_json::Value) -> Result<CampusProject, String> {
    let root = value.get("project").unwrap_or(value);
    if root.get("schemaVersion").and_then(|value| value.as_str()) != Some("1.0") {
        return Err("不支持的旧版项目格式".into());
    }
    let name = root
        .get("name")
        .and_then(|value| value.as_str())
        .ok_or("旧版项目缺少名称")?;
    let campus_name = root
        .pointer("/campus/canonicalName")
        .and_then(|value| value.as_str())
        .or_else(|| {
            root.pointer("/campus/displayName")
                .and_then(|value| value.as_str())
        })
        .ok_or("旧版项目缺少校区名称")?;
    let mut project = CampusProject::new(name, campus_name);
    let foundation = root.get("foundation").ok_or("旧版项目缺少地基数据")?;
    project.orientation_degrees = foundation
        .get("orientationDegrees")
        .and_then(|value| value.as_f64())
        .unwrap_or_default();
    project.blocks_per_meter = foundation
        .pointer("/foundationStyle/blocksPerMeter")
        .and_then(|value| value.as_f64())
        .unwrap_or(1.0)
        .clamp(0.25, 4.0);
    project.foundation_style_preset = foundation
        .pointer("/foundationStylePack/id")
        .and_then(|value| value.as_str())
        .map(FoundationStylePreset::from_legacy_id)
        .unwrap_or_default();
    project.boundary = points_from_value(
        foundation
            .pointer("/boundaryDraft/points")
            .unwrap_or(&serde_json::Value::Null),
    );
    let reviews = foundation
        .get("reviews")
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default();
    if let Some(candidates) = foundation
        .get("candidates")
        .and_then(|value| value.as_array())
    {
        for candidate in candidates {
            let Some(kind) = candidate
                .get("kind")
                .and_then(|value| value.as_str())
                .and_then(parse_kind)
            else {
                continue;
            };
            let id = candidate
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("legacy-candidate")
                .to_string();
            let review = reviews
                .get(&id)
                .and_then(review_from_value)
                .unwrap_or(ReviewDecision::Pending);
            project.candidates.push(MapCandidate {
                id,
                name: candidate
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("旧版候选")
                    .to_string(),
                kind,
                source: candidate
                    .pointer("/provenance/sourceLabel")
                    .or_else(|| candidate.get("source"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("旧版项目")
                    .to_string(),
                confidence: candidate
                    .get("confidence")
                    .and_then(|value| value.as_str())
                    .unwrap_or("medium")
                    .to_string(),
                points: points_from_value(
                    candidate
                        .pointer("/geometry/points")
                        .unwrap_or(&serde_json::Value::Null),
                ),
                height_m: candidate.get("heightM").and_then(|value| value.as_f64()),
                floors: candidate
                    .get("floors")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32),
                roof_shape: candidate
                    .get("roofShape")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned),
                tags: candidate
                    .get("tags")
                    .and_then(|value| value.as_object())
                    .into_iter()
                    .flatten()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_string()))
                    })
                    .collect(),
                review,
            });
        }
    }
    if let Some(features) = foundation
        .get("manualFeatures")
        .and_then(|value| value.as_array())
    {
        for feature in features {
            let Some(kind) = feature
                .get("kind")
                .and_then(|value| value.as_str())
                .and_then(parse_kind)
            else {
                continue;
            };
            project.features.push(MapFeature {
                id: feature
                    .get("id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("legacy-feature")
                    .to_string(),
                name: feature
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("旧版手绘地物")
                    .to_string(),
                kind,
                points: points_from_value(
                    feature
                        .pointer("/geometry/points")
                        .or_else(|| feature.get("points"))
                        .unwrap_or(&serde_json::Value::Null),
                ),
                block: feature
                    .get("block")
                    .and_then(|value| value.as_str())
                    .map(normalize_block)
                    .unwrap_or_else(|| default_block(kind).to_string()),
                source_id: None,
            });
        }
    }
    if let Some(records) = value
        .pointer("/campusMemory/buildingNames")
        .and_then(|value| value.as_array())
    {
        for record in records {
            let Some(source_id) = record.get("sourceId").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(name) = record.get("name").and_then(|value| value.as_str()) else {
                continue;
            };
            if record.get("status").and_then(|value| value.as_str()) == Some("excluded") {
                continue;
            }
            project.building_directory.push(BuildingDirectoryRecord {
                source_id: source_id.to_string(),
                name: name.to_string(),
                updated_at_unix_ms: now_unix_ms(),
            });
            if let Some(candidate) = project
                .candidates
                .iter_mut()
                .find(|candidate| candidate.id == source_id)
            {
                candidate.name = name.to_string();
            }
        }
    }
    if let Some(suppressions) = value
        .pointer("/campusMemory/suppressions")
        .and_then(|value| value.as_array())
    {
        for suppression in suppressions {
            let Some(source_id) = suppression.get("sourceId").and_then(|value| value.as_str())
            else {
                continue;
            };
            project.building_suppressions.push(BuildingSuppression {
                source_id: source_id.to_string(),
                reason: suppression
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Imported legacy suppression")
                    .to_string(),
                suppressed_at_unix_ms: now_unix_ms(),
            });
        }
        let suppressed = project
            .building_suppressions
            .iter()
            .map(|record| record.source_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        project
            .candidates
            .retain(|candidate| !suppressed.contains(candidate.id.as_str()));
    }
    let accepted_ids = project
        .candidates
        .iter()
        .filter(|candidate| candidate.review == ReviewDecision::Accepted)
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    for id in accepted_ids {
        project.accept_candidate(&id);
    }
    Ok(project)
}

fn points_from_value(value: &serde_json::Value) -> Vec<GeoPoint> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|point| {
            Some(GeoPoint {
                lng: point.get("lng")?.as_f64()?,
                lat: point.get("lat")?.as_f64()?,
            })
        })
        .collect()
}

fn parse_kind(value: &str) -> Option<FeatureKind> {
    match value {
        "building" => Some(FeatureKind::Building),
        "road" => Some(FeatureKind::Road),
        "water" => Some(FeatureKind::Water),
        "vegetation" => Some(FeatureKind::Vegetation),
        "sports" => Some(FeatureKind::Sports),
        _ => None,
    }
}

fn review_from_value(value: &serde_json::Value) -> Option<ReviewDecision> {
    let value = value
        .as_str()
        .or_else(|| value.get("status").and_then(|value| value.as_str()))?;
    match value {
        "accepted" | "accept" => Some(ReviewDecision::Accepted),
        "rejected" | "reject" => Some(ReviewDecision::Rejected),
        _ => Some(ReviewDecision::Pending),
    }
}

fn normalize_block(value: &str) -> String {
    if value.starts_with("minecraft:") {
        value.into()
    } else {
        format!("minecraft:{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_round_trip_preserves_workflow() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("putuo.campus.json");
        let mut state = DesktopApplicationState::default();
        state.new_project("普陀复刻", "华东师范大学普陀校区");
        state.set_mode(DesktopMode::Detailed);
        state.mutate_project(|project| {
            project.foundation_step = FoundationStep::Building;
            project.detailed.style_preset = ArnisStylePreset::Historic;
        });
        state.save_to(&path).unwrap();

        let mut restored = DesktopApplicationState::default();
        restored.open(&path).unwrap();
        let project = restored.project.unwrap();
        assert_eq!(project.mode, DesktopMode::Detailed);
        assert_eq!(project.foundation_step, FoundationStep::Building);
        assert_eq!(project.detailed.style_preset, ArnisStylePreset::Historic);
        assert!(!restored.dirty);
    }

    #[test]
    fn accepting_building_creates_feature_and_slot() {
        let mut project = CampusProject::new("test", "campus");
        project.candidates.push(MapCandidate {
            id: "osm:1".into(),
            name: "图书馆".into(),
            kind: FeatureKind::Building,
            source: "osm".into(),
            confidence: "high".into(),
            points: vec![GeoPoint { lng: 1.0, lat: 2.0 }],
            height_m: Some(18.0),
            floors: Some(4),
            roof_shape: Some("flat".into()),
            tags: BTreeMap::from([
                ("3dmr".into(), "model-42".into()),
                ("3dmr:license".into(), "CC-BY-4.0".into()),
                ("3dmr:author".into(), "Example Author".into()),
                ("model:height".into(), "30".into()),
            ]),
            review: ReviewDecision::Pending,
        });
        assert!(project.accept_candidate("osm:1"));
        assert_eq!(project.features.len(), 1);
        assert_eq!(project.building_slots.len(), 1);
        assert_eq!(project.features[0].block, "minecraft:quartz_block");
        project.apply_foundation_style(FoundationStylePreset::HistoricRedBrick);
        assert_eq!(project.features[0].block, "minecraft:bricks");
        assert_eq!(project.foundation_style_preset.road_width_blocks(), 3);
        project.discover_external_models_for_slot("osm:1");
        assert_eq!(project.detailed.external_models.len(), 1);
        assert_eq!(
            project.detailed.external_models[0].eligibility,
            ExternalModelEligibility::Eligible
        );
        assert!(project
            .detailed
            .source_conflicts
            .iter()
            .any(|conflict| conflict.kind == "dimension_mismatch"));
        let model_id = project.detailed.external_models[0].id.clone();
        project
            .review_external_model(
                &model_id,
                ExternalModelDecision::EligiblePrimary,
                "许可和身份均已核对",
            )
            .unwrap();
        let conflict_id = project.detailed.source_conflicts[0].id.clone();
        project
            .review_source_conflict(
                &conflict_id,
                SourceConflictDecision::PrimarySelected,
                "以现场测量为准",
            )
            .unwrap();
        project.rename_building("osm:1", "新图书馆").unwrap();
        assert_eq!(project.building_slots[0].name, "新图书馆");
        assert_eq!(project.building_directory[0].name, "新图书馆");
        assert!(project.reset_candidate_review("osm:1"));
        assert_eq!(project.candidates[0].review, ReviewDecision::Pending);
        assert!(project.features.is_empty());
        assert!(project.building_slots.is_empty());
        let manual_id = project
            .add_manual_feature(
                FeatureKind::Building,
                vec![
                    GeoPoint { lng: 1.0, lat: 2.0 },
                    GeoPoint {
                        lng: 1.001,
                        lat: 2.0,
                    },
                    GeoPoint {
                        lng: 1.001,
                        lat: 1.999,
                    },
                ],
            )
            .unwrap();
        assert!(manual_id.starts_with("manual:building:"));
        assert_eq!(project.features.len(), 1);
        assert_eq!(project.building_slots.len(), 1);
        assert_eq!(project.features[0].source_id, None);
    }

    #[test]
    fn building_suppression_is_persistent_and_recoverable() {
        let mut project = CampusProject::new("test", "campus");
        project.candidates.push(MapCandidate {
            id: "osm:off-campus".into(),
            name: "Neighbor".into(),
            kind: FeatureKind::Building,
            source: "osm".into(),
            confidence: "low".into(),
            points: vec![
                GeoPoint { lng: 1.0, lat: 2.0 },
                GeoPoint {
                    lng: 1.001,
                    lat: 2.0,
                },
                GeoPoint {
                    lng: 1.001,
                    lat: 1.999,
                },
            ],
            height_m: None,
            floors: None,
            roof_shape: None,
            tags: BTreeMap::new(),
            review: ReviewDecision::Pending,
        });
        project
            .suppress_building("osm:off-campus", "neighboring school")
            .unwrap();
        assert!(project.candidates.is_empty());
        assert_eq!(project.building_suppressions.len(), 1);
        assert!(project.restore_building_suppression("osm:off-campus"));
        assert!(project.building_suppressions.is_empty());
    }

    #[test]
    fn imports_portable_web_project_v1() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.campus.json");
        fs::write(
            &path,
            r#"{
              "project": {
                "schemaVersion": "1.0",
                "name": "旧项目",
                "campus": {"canonicalName": "测试大学"},
                "foundation": {
                  "orientationDegrees": 18,
                  "foundationStyle": {"blocksPerMeter": 1.5},
                  "foundationStylePack": {"id": "arnis:modern-campus/v1"},
                  "boundaryDraft": {"points": [{"lng":121.0,"lat":31.0},{"lng":121.1,"lat":31.0},{"lng":121.1,"lat":30.9}]},
                  "reviews": {"old-building": "accepted"},
                  "candidates": [{
                    "id":"old-building","name":"图书馆","kind":"building","source":"overture","confidence":"high",
                    "geometry":{"points":[{"lng":121.0,"lat":31.0},{"lng":121.01,"lat":31.0},{"lng":121.01,"lat":30.99}]}
                  }],
                  "manualFeatures": []
                }
              },
              "campusMemory": {
                "buildingNames": [{"sourceId":"old-building","name":"旧版命名图书馆"}],
                "suppressions": []
              }
            }"#,
        )
        .unwrap();
        let mut state = DesktopApplicationState::default();
        state.open(path).unwrap();
        let project = state.project.unwrap();
        assert_eq!(project.name, "旧项目");
        assert_eq!(project.orientation_degrees, 18.0);
        assert_eq!(project.building_slots.len(), 1);
        assert_eq!(project.building_slots[0].name, "旧版命名图书馆");
        assert_eq!(
            project.foundation_style_preset,
            FoundationStylePreset::ModernCampus
        );
    }

    #[test]
    fn detailed_refinements_are_versioned_and_confirmed_explicitly() {
        let mut project = CampusProject::new("test", "campus");
        project.building_slots.push(BuildingSlot {
            id: "library".into(),
            name: "Library".into(),
            footprint: Vec::new(),
            height_m: Some(24.0),
            floors: Some(6),
            roof_shape: Some("flat".into()),
            refined: false,
        });
        project.record_refinement_draft("library", 1, PathBuf::from("library-v1.json"));
        project.record_refinement_draft("library", 2, PathBuf::from("library-v2.json"));
        assert_eq!(project.next_refinement_version("library"), 3);
        assert_eq!(
            project.detailed.refinements[0].status,
            RefinementStatus::Archived
        );
        assert!(!project.building_slots[0].refined);
        assert_eq!(project.confirm_latest_refinement("library"), Some(2));
        assert_eq!(
            project.latest_refinement("library").unwrap().status,
            RefinementStatus::Confirmed
        );
        assert!(project.building_slots[0].refined);
        project.record_semantic_feature(
            "library",
            "library:v2",
            SemanticFeatureDraft {
                kind: SemanticFeatureKind::EntranceEmphasis,
                label: "main entrance".into(),
                side: SemanticFeatureSide::South,
                height_band: SemanticHeightBand::Lower,
                strength: SemanticStrength::Visible,
                reason: "visible in reference".into(),
            },
            15,
            "minecraft:polished_andesite".into(),
        );
        assert_eq!(project.detailed.semantic_features.len(), 1);
        assert_eq!(project.detailed.semantic_features[0].affected_blocks, 15);
    }
}
