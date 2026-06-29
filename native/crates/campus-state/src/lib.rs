use serde::{Deserialize, Serialize};
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
    pub foundation_style_preset: FoundationStylePreset,
    #[serde(default)]
    pub foundation_preview_path: Option<PathBuf>,
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
            foundation_style_preset: FoundationStylePreset::ArnisClassic,
            foundation_preview_path: None,
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

    pub fn confirm_step(&mut self) {
        if !self.completed_steps.contains(&self.foundation_step) {
            self.completed_steps.push(self.foundation_step);
        }
        self.foundation_step = self.foundation_step.next();
    }
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
            review: ReviewDecision::Pending,
        });
        assert!(project.accept_candidate("osm:1"));
        assert_eq!(project.features.len(), 1);
        assert_eq!(project.building_slots.len(), 1);
        assert_eq!(project.features[0].block, "minecraft:quartz_block");
        project.apply_foundation_style(FoundationStylePreset::HistoricRedBrick);
        assert_eq!(project.features[0].block, "minecraft:bricks");
        assert_eq!(project.foundation_style_preset.road_width_blocks(), 3);
        assert!(project.reset_candidate_review("osm:1"));
        assert_eq!(project.candidates[0].review, ReviewDecision::Pending);
        assert!(project.features.is_empty());
        assert!(project.building_slots.is_empty());
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
              "campusMemory": {}
            }"#,
        )
        .unwrap();
        let mut state = DesktopApplicationState::default();
        state.open(path).unwrap();
        let project = state.project.unwrap();
        assert_eq!(project.name, "旧项目");
        assert_eq!(project.orientation_degrees, 18.0);
        assert_eq!(project.building_slots.len(), 1);
        assert_eq!(
            project.foundation_style_preset,
            FoundationStylePreset::ModernCampus
        );
    }
}
