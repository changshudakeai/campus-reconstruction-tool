#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod desktop_tool_adapters;
mod desktop_tool_process;
mod diagnostics;
mod v11_acquisition_client;
mod v11_boundary_evidence_desk;
mod v11_foundation_review_desk;
mod v11_guidance;
#[cfg(test)]
mod v11_project_kernel;
mod v11_project_library;
mod v11_tracer_bullet;

use desktop_tool_process::DesktopToolProcessSupervisor;
use v11_project_library::{CampusProjectLauncher, LauncherStep, ProjectRowSaveState};

use arnis_core::{FootprintComponent, GenerateBuildingRequest, MaterialOverrides};
use campus_state::{
    ArnisStylePreset, CampusProject, CampusProjectLibrary, CampusReconstructionWorkflow,
    CampusScope, CampusTargetEvidence, CandidateConfidence, CandidateConfidenceFilter,
    DesktopApplicationState, DesktopLocale, DesktopMode, DetailedBuildingHandoff,
    DetailedBuildingRuleStack, DetailedBuildingWorkspace, DetailedBuildingWorkspaceTask,
    ExternalModelDecision, FeatureKind, FoundationMapTask, FoundationPhase, FoundationStep,
    FoundationStylePack, FoundationStylePreset, FoundationWorkflow, FoundationWorkflowIntent,
    GeoPoint, MapCandidate, MapViewState, ReconstructionWorkflowIntent, ReviewDecision,
    SemanticFeatureDraft, SemanticFeatureKind, SemanticFeatureSide, SemanticHeightBand,
    SemanticStrength, SourceConflictDecision,
};
use campus_tool_protocol::{MapCoordinate, MapOverlay, MapPurpose};
#[cfg(test)]
use campus_tool_protocol::{ToolEvent, ToolKind, PROTOCOL_VERSION};
use rand::Rng;
#[cfg(test)]
use slint::Model;
use slint::{ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;

slint::include_modules!();

fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("CampusReconstructionTool")
}

fn install_diagnostics() {
    let log_directory = app_data_dir().join("logs");
    if diagnostics::initialise(&log_directory).is_ok() {
        let executable = std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "unknown".into());
        diagnostics::record(
            diagnostics::DiagnosticLevel::Info,
            "application.start",
            "Campus Reconstruction Tool started",
            &[("executable", executable.as_str())],
        );
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message =
            v11_guidance::sanitise_registered_diagnostic_value("panic", &info.to_string());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        diagnostics::record(
            diagnostics::DiagnosticLevel::Error,
            "application.panic",
            &message,
            &[("backtrace", backtrace.as_str())],
        );
        previous(info);
    }));
}

fn default_project_path() -> PathBuf {
    app_data_dir().join("projects").join("active.campus.json")
}

fn generated_model_dir() -> PathBuf {
    app_data_dir().join("generated")
}

fn locale_path() -> PathBuf {
    app_data_dir().join("locale.txt")
}

fn preferences_path() -> PathBuf {
    app_data_dir().join("preferences.json")
}

fn load_locale() -> DesktopLocale {
    match std::fs::read_to_string(locale_path())
        .unwrap_or_default()
        .trim()
    {
        "en" => DesktopLocale::En,
        _ => DesktopLocale::ZhCn,
    }
}

fn persist_locale(locale: DesktopLocale) -> Result<(), String> {
    std::fs::create_dir_all(app_data_dir()).map_err(|error| error.to_string())?;
    std::fs::write(
        locale_path(),
        if locale == DesktopLocale::En {
            "en"
        } else {
            "zh-CN"
        },
    )
    .map_err(|error| error.to_string())
}

fn project_file_dialog(save: bool) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("Campus Reconstruction Project", &["json"])
        .set_directory(app_data_dir());
    if save {
        dialog = dialog.set_file_name("campus-project.campus.json");
        dialog.save_file()
    } else {
        dialog.pick_file()
    }
}

fn schematic_file_dialog(default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Sponge Schematic", &["schem"])
        .set_directory(
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(app_data_dir)
                .join("Downloads"),
        )
        .set_file_name(default_name)
        .save_file()
}

fn foundation_style_file_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Foundation Style Pack", &["json"])
        .set_directory(app_data_dir())
        .pick_file()
}

fn local_evidence_file_dialog() -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Image files", &["jpg", "jpeg", "png", "webp"])
        .pick_files()
        .unwrap_or_default()
}

#[derive(Clone, Default)]
struct LocalCredentials {
    js_api_key: String,
    security_code: String,
    acquisition_secret: String,
}

fn credential_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("Campus Reconstruction Tool", account).map_err(|error| error.to_string())
}

fn load_local_credentials() -> LocalCredentials {
    let from_store = |account: &str| {
        credential_entry(account)
            .and_then(|entry| entry.get_password().map_err(|error| error.to_string()))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let stored_key = from_store("gaode-js-api-key");
    let stored_security = from_store("gaode-security-code");
    let stored_acquisition_secret = from_store("acquisition-service-secret");
    LocalCredentials {
        js_api_key: if stored_key.is_empty() {
            std::env::var("GAODE_JS_API_KEY")
                .or_else(|_| std::env::var("VITE_GAODE_JS_API_KEY"))
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            stored_key
        },
        security_code: if stored_security.is_empty() {
            std::env::var("GAODE_SECURITY_CODE")
                .or_else(|_| std::env::var("VITE_GAODE_SECURITY_CODE"))
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            stored_security
        },
        acquisition_secret: if stored_acquisition_secret.is_empty() {
            std::env::var("CAMPUS_ACQUISITION_SERVICE_SECRET")
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            stored_acquisition_secret
        },
    }
}

fn save_local_credentials(credentials: &LocalCredentials) -> Result<(), String> {
    credential_entry("gaode-js-api-key")?
        .set_password(credentials.js_api_key.trim())
        .map_err(|error| error.to_string())?;
    credential_entry("gaode-security-code")?
        .set_password(credentials.security_code.trim())
        .map_err(|error| error.to_string())?;
    credential_entry("acquisition-service-secret")?
        .set_password(credentials.acquisition_secret.trim())
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn autosave(state: &Rc<RefCell<DesktopApplicationState>>) -> Result<(), String> {
    let path = state
        .borrow()
        .project_path
        .clone()
        .unwrap_or_else(default_project_path);
    state.borrow_mut().save_to(path)
}

fn page_copy(
    step: FoundationStep,
    locale: DesktopLocale,
) -> (&'static str, &'static str, &'static str) {
    if locale == DesktopLocale::En {
        return match step {
            FoundationStep::Campus => (
                "Choose a school and campus",
                "Confirm the exact campus before discovering buildings, roads, water, vegetation, and sports facilities.",
                "Confirm campus and continue",
            ),
            FoundationStep::Boundary => (
                "Confirm campus boundary",
                "Adjust the map view and outline. The boundary defines scope; it does not replace Foundation features.",
                "Confirm boundary and continue",
            ),
            FoundationStep::Orientation => (
                "Set Minecraft orientation",
                "Align a campus axis to Minecraft. Every downstream feature shares this project orientation.",
                "Confirm orientation and continue",
            ),
            FoundationStep::Building => (
                "Review building evidence",
                "Resolve Building Entities from pinned source evidence and acknowledge any unsupported gaps.",
                "Complete building review",
            ),
            FoundationStep::Road => (
                "Review roads",
                "Review main roads and footways, then apply the style-pack road width.",
                "Complete road review",
            ),
            FoundationStep::Water => (
                "Review water",
                "Review rivers, ponds, and landscape water surfaces.",
                "Complete water review",
            ),
            FoundationStep::Vegetation => (
                "Review vegetation",
                "Review woodland, lawns, and major landscaped areas.",
                "Complete vegetation review",
            ),
            FoundationStep::Sports => (
                "Review sports facilities",
                "Review tracks, courts, fields, and other sports areas.",
                "Complete sports review",
            ),
            FoundationStep::Export => (
                "Export campus Foundation",
                "Inspect scope, block count, and Building Slots before exporting the portable project and schematic.",
                "Complete Foundation",
            ),
        };
    }
    match step {
        FoundationStep::Campus => (
            "选择学校与具体校区",
            "确认校区后再发现建筑、道路、水域、植被与体育设施。",
            "确认校区并继续",
        ),
        FoundationStep::Boundary => (
            "确认校园边界",
            "调整地图缩放与轮廓，边界只定义校区范围，不代替地基要素。",
            "确认边界并继续",
        ),
        FoundationStep::Orientation => (
            "确定 Minecraft 朝向",
            "选择一条校内主轴作为 Minecraft 水平轴，后续所有要素共用此方向。",
            "确认朝向并继续",
        ),
        FoundationStep::Building => (
            "审核建筑与补缺",
            "接受可靠建筑候选；缺失建筑通过当前地图视野或人工绘制补充。",
            "完成建筑审核",
        ),
        FoundationStep::Road => (
            "审核道路",
            "审核主路与步道，并调整道路宽度。",
            "完成道路审核",
        ),
        FoundationStep::Water => ("审核水域", "审核河道、池塘与景观水面。", "完成水域审核"),
        FoundationStep::Vegetation => {
            ("审核植被", "审核树林、草地与主要绿化区域。", "完成植被审核")
        }
        FoundationStep::Sports => (
            "审核体育设施",
            "审核操场、球场与其他体育区域。",
            "完成体育审核",
        ),
        FoundationStep::Export => (
            "导出校园地基",
            "检查范围、方块数量与建筑槽位后导出工程。",
            "完成 Foundation",
        ),
    }
}

fn tr<'a>(locale: DesktopLocale, zh: &'a str, en: &'a str) -> &'a str {
    if locale == DesktopLocale::En {
        en
    } else {
        zh
    }
}

fn strings(values: &[&str]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        values
            .iter()
            .map(|value| SharedString::from(*value))
            .collect::<Vec<_>>(),
    ))
}

fn format_latest_save_time(unix_ms: u64) -> String {
    let seconds = (unix_ms / 1_000) as i64;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_date_from_unix_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

fn civil_date_from_unix_days(days: i64) -> (i64, i64, i64) {
    let adjusted = days + 719_468;
    let era = if adjusted >= 0 {
        adjusted
    } else {
        adjusted - 146_096
    }
    .div_euclid(146_097);
    let day_of_era = adjusted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}
fn project_row_save_state_label(state: &ProjectRowSaveState, english: bool) -> SharedString {
    match (state, english) {
        (ProjectRowSaveState::Saved, true) => "Saved".into(),
        (ProjectRowSaveState::Saved, false) => "已保存".into(),
        (ProjectRowSaveState::RecoveryAvailable, true) => "Recovery available".into(),
        (ProjectRowSaveState::RecoveryAvailable, false) => "有项目恢复状态".into(),
        (ProjectRowSaveState::SaveFailed(reason), true) => format!("Save failed: {reason}").into(),
        (ProjectRowSaveState::SaveFailed(reason), false) => format!("保存失败：{reason}").into(),
    }
}

fn launcher_task_label(task: &str, english: bool) -> SharedString {
    if english {
        return task.into();
    }
    match task {
        "Confirm Campus Boundary" => "确认校园边界",
        "Acquire Foundation evidence" => "获取 Foundation 证据",
        "Review Buildings" => "审核建筑",
        "Review Circulation" => "审核交通",
        "Review Water" => "审核水域",
        "Review Vegetation" => "审核植被",
        "Review Sports" => "审核体育设施",
        "Generate Minecraft result" => "生成 Minecraft 结果",
        "Export .schem and Foundation Manifest" => "导出 .schem 与 Foundation Manifest",
        "Completion and export" => "完成与导出",
        other => other,
    }
    .into()
}
fn guidance_locale(ui: &AppWindow) -> v11_guidance::Locale {
    if ui.get_english() {
        v11_guidance::Locale::En
    } else {
        v11_guidance::Locale::ZhCn
    }
}

fn modal_state_from_code(value: i32) -> v11_guidance::ModalState {
    match value {
        1 => v11_guidance::ModalState::Guidance,
        2 => v11_guidance::ModalState::Settings,
        3 => v11_guidance::ModalState::QuickStart,
        4 => v11_guidance::ModalState::Utilities,
        5 => v11_guidance::ModalState::Error,
        _ => v11_guidance::ModalState::None,
    }
}

fn map_tool_state_from_code(value: i32) -> v11_guidance::MapToolState {
    match value {
        1 => v11_guidance::MapToolState::BoundaryVertexSelected,
        _ => v11_guidance::MapToolState::None,
    }
}

fn workflow_task_state(state: Option<&DesktopApplicationState>) -> v11_guidance::WorkflowTaskState {
    let Some(project) = state.and_then(|state| state.project.as_ref()) else {
        return v11_guidance::WorkflowTaskState::None;
    };
    if project.mode != DesktopMode::Foundation {
        return v11_guidance::WorkflowTaskState::None;
    }
    match project.foundation_step {
        FoundationStep::Campus | FoundationStep::Boundary => v11_guidance::WorkflowTaskState::None,
        FoundationStep::Orientation
            if project.campus_target.is_none() || project.boundary.len() < 3 =>
        {
            v11_guidance::WorkflowTaskState::None
        }
        _ => v11_guidance::WorkflowTaskState::Confirmable,
    }
}

fn shortcut_context(
    state: Option<&DesktopApplicationState>,
    launcher: Option<&CampusProjectLauncher>,
    modal: v11_guidance::ModalState,
    text_input_focused: bool,
    map_tool: v11_guidance::MapToolState,
) -> v11_guidance::ShortcutContext {
    let (has_active_project, can_create_project, can_undo, can_redo, workflow) =
        if let Some(launcher) = launcher {
            (
                launcher.active_project_id().is_some(),
                launcher.confirmed_campus().is_some()
                    && launcher.step() == LauncherStep::ProjectLibrary,
                launcher.can_undo(),
                launcher.can_redo(),
                v11_guidance::WorkflowTaskState::None,
            )
        } else if let Some(state) = state {
            (
                state.project.is_some(),
                false,
                state.can_undo(),
                state.can_redo(),
                workflow_task_state(Some(state)),
            )
        } else {
            (
                false,
                false,
                false,
                false,
                v11_guidance::WorkflowTaskState::None,
            )
        };

    v11_guidance::ShortcutContext {
        text_input_focused,
        modal,
        map_tool,
        workflow,
        has_active_project,
        can_create_project,
        can_undo,
        can_redo,
    }
}

fn sync_shortcut_rows(
    ui: &AppWindow,
    state: Option<&DesktopApplicationState>,
    launcher: Option<&CampusProjectLauncher>,
) {
    let locale = guidance_locale(ui);
    let context = shortcut_context(
        state,
        launcher,
        modal_state_from_code(ui.get_shortcut_modal()),
        ui.get_text_input_focused(),
        map_tool_state_from_code(ui.get_map_tool_state()),
    );
    let rows = v11_guidance::Shortcut::ALL
        .into_iter()
        .map(|shortcut| {
            let outcome = v11_guidance::resolve_shortcut(shortcut, context, locale);
            ShortcutRow {
                label: shortcut.label(locale).into(),
                keys: shortcut.keys().into(),
                available: outcome.is_available(),
                reason: outcome.reason().into(),
            }
        })
        .collect::<Vec<_>>();
    ui.set_shortcut_rows(ModelRc::new(VecModel::from(rows)));
}

fn sync_project_launcher_ui(
    ui: &AppWindow,
    launcher: &CampusProjectLauncher,
) -> Result<(), String> {
    ui.set_campus_launcher_visible(true);
    let step = match launcher.step() {
        LauncherStep::CampusTarget => 0,
        LauncherStep::ProjectLibrary => 1,
        LauncherStep::Workspace => 2,
    };
    ui.set_launcher_step(step);
    let english = ui.get_english();
    let campus = launcher
        .confirmed_campus()
        .or_else(|| launcher.offered_campus());
    ui.set_launcher_campus_name(
        campus
            .map(CampusScope::canonical_name)
            .unwrap_or_default()
            .into(),
    );

    let rows = if launcher.confirmed_campus().is_some() {
        launcher.rows()?
    } else {
        Vec::new()
    };
    let active_id = launcher.active_project_id().cloned();
    ui.set_launcher_active_project_id(
        active_id
            .as_ref()
            .map(|project_id| project_id.as_str())
            .unwrap_or_default()
            .into(),
    );
    let active = active_id
        .as_ref()
        .and_then(|project_id| rows.iter().find(|row| &row.project_id == project_id));
    let models = rows
        .iter()
        .map(|row| LauncherProjectRow {
            project_id: row.project_id.as_str().into(),
            project_name: row.project_name.clone().into(),
            latest_save: format_latest_save_time(row.latest_successful_save_unix_ms).into(),
            save_state: project_row_save_state_label(&row.save_state, english),

            progress: format!("{} / {}", row.completed_tasks, row.total_tasks).into(),
            next_task: launcher_task_label(&row.next_incomplete_task, english),
            action_label: if row.completed_tasks == row.total_tasks {
                if english { "View" } else { "查看" }.into()
            } else {
                if english { "Continue" } else { "继续" }.into()
            },
        })
        .collect::<Vec<_>>();
    ui.set_launcher_projects(ModelRc::new(VecModel::from(models)));
    ui.set_can_undo(launcher.can_undo());
    ui.set_can_redo(launcher.can_redo());

    if let Some(row) = active {
        ui.set_project_name(row.project_name.clone().into());
        ui.set_launcher_progress(format!("{} / {}", row.completed_tasks, row.total_tasks).into());
        ui.set_launcher_save_state(project_row_save_state_label(&row.save_state, english));

        ui.set_launcher_next_task(launcher_task_label(&row.next_incomplete_task, english));
        ui.set_launcher_compatibility(row.minecraft_compatibility.clone().into());
        ui.set_route_stage(if row.completed_tasks == row.total_tasks {
            4
        } else {
            2
        });
    } else {
        ui.set_project_name(
            if step == 0 {
                if english {
                    "Campus Target"
                } else {
                    "校区目标"
                }
            } else {
                if english {
                    "Campus Project Library"
                } else {
                    "校区项目库"
                }
            }
            .into(),
        );
        ui.set_launcher_progress(
            if english {
                format!("{} project(s)", rows.len())
            } else {
                format!("{} 个项目", rows.len())
            }
            .into(),
        );
        ui.set_launcher_save_state(
            if english {
                "No active project"
            } else {
                "没有活动项目"
            }
            .into(),
        );
        ui.set_launcher_next_task(if step == 0 {
            if english {
                "Confirm Campus Target"
            } else {
                "确认校区目标"
            }
            .into()
        } else {
            if english {
                "Choose or create a project"
            } else {
                "选择或新建项目"
            }
            .into()
        });
        ui.set_route_stage(step);
    }
    sync_shortcut_rows(ui, None, Some(launcher));
    Ok(())
}
fn launcher_project_id(
    launcher: &CampusProjectLauncher,
    value: &str,
) -> Result<campus_state::ProjectId, String> {
    launcher
        .rows()?
        .into_iter()
        .find(|row| row.project_id.as_str() == value)
        .map(|row| row.project_id)
        .ok_or_else(|| format!("Project is not in the confirmed campus library: {value}"))
}

fn sync_locale_models(ui: &AppWindow, locale: DesktopLocale) {
    let english = locale == DesktopLocale::En;
    ui.set_step_labels(if english {
        strings(&[
            "Campus",
            "Boundary",
            "Orientation",
            "Buildings",
            "Roads",
            "Water",
            "Vegetation",
            "Sports",
            "Export",
        ])
    } else {
        strings(&[
            "校区", "边界", "朝向", "建筑", "道路", "水域", "植被", "体育", "导出",
        ])
    });
    ui.set_arnis_styles(if english {
        strings(&[
            "House",
            "Residential / Dormitory",
            "Farm",
            "Commercial",
            "Office",
            "Hotel",
            "Industrial",
            "Warehouse",
            "School / Public",
            "Hospital",
            "Religious",
            "Historic",
            "Tower",
            "Garage",
            "Shed",
            "Greenhouse",
            "Tall Building",
            "Glassy Skyscraper",
            "Modern Skyscraper",
        ])
    } else {
        strings(
            &ArnisStylePreset::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_foundation_styles(if english {
        strings(&[
            "Arnis Classic",
            "Modern Campus",
            "Historic Red-Brick Campus",
            "Lightweight Draft",
        ])
    } else {
        strings(
            &FoundationStylePreset::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_candidate_filters(if english {
        strings(&[
            "All pending",
            "High confidence",
            "Medium confidence",
            "Low confidence",
            "Confirmed",
            "Rejected",
        ])
    } else {
        strings(
            &CandidateConfidenceFilter::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_semantic_feature_kinds(if english {
        strings(&["Entrance", "Window band", "Roof ridge", "Cornice", "Frame"])
    } else {
        strings(
            &SemanticFeatureKind::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_semantic_feature_sides(if english {
        strings(&["North", "South", "East", "West", "Center"])
    } else {
        strings(
            &SemanticFeatureSide::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_semantic_height_bands(if english {
        strings(&["Lower", "Middle", "Upper", "Roof"])
    } else {
        strings(
            &SemanticHeightBand::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_semantic_strengths(if english {
        strings(&["Subtle", "Visible", "Strong"])
    } else {
        strings(
            &SemanticStrength::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_external_model_decisions(if english {
        strings(&[
            "Pending",
            "Use as primary geometry",
            "Supporting evidence only",
            "Reject",
        ])
    } else {
        strings(
            &ExternalModelDecision::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
    ui.set_source_conflict_decisions(if english {
        strings(&[
            "Unresolved",
            "Select primary source",
            "Supporting only",
            "Reject conflicting source",
        ])
    } else {
        strings(
            &SourceConflictDecision::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        )
    });
}

fn external_decision_label(value: ExternalModelDecision, locale: DesktopLocale) -> &'static str {
    if locale != DesktopLocale::En {
        return value.label();
    }
    match value {
        ExternalModelDecision::Pending => "Pending",
        ExternalModelDecision::EligiblePrimary => "Primary geometry",
        ExternalModelDecision::SupportingEvidence => "Supporting evidence",
        ExternalModelDecision::Rejected => "Rejected",
    }
}

fn conflict_decision_label(value: SourceConflictDecision, locale: DesktopLocale) -> &'static str {
    if locale != DesktopLocale::En {
        return value.label();
    }
    match value {
        SourceConflictDecision::Unresolved => "Unresolved",
        SourceConflictDecision::PrimarySelected => "Primary selected",
        SourceConflictDecision::SupportingOnly => "Supporting only",
        SourceConflictDecision::Rejected => "Rejected",
    }
}

fn refinement_status_label(
    value: campus_state::RefinementStatus,
    locale: DesktopLocale,
) -> &'static str {
    if locale != DesktopLocale::En {
        return value.label();
    }
    match value {
        campus_state::RefinementStatus::Draft => "Draft",
        campus_state::RefinementStatus::Confirmed => "Confirmed",
        campus_state::RefinementStatus::Archived => "Archived",
    }
}

fn sync_ui(ui: &AppWindow, state: &DesktopApplicationState) {
    let locale = state.locale;
    let english = locale == DesktopLocale::En;
    ui.set_english(english);
    sync_locale_models(ui, locale);
    let Some(project) = &state.project else {
        ui.set_save_status(tr(locale, "未保存", "Unsaved").into());
        return;
    };
    ui.set_project_name(project.name.clone().into());
    ui.set_campus_name(project.campus_name.clone().into());
    ui.set_has_campus_target(project.campus_target.is_some());
    ui.set_tool_status(state.tool_status.clone().unwrap_or_default().into());
    ui.set_selected_block_summary(
        state
            .selected_preview_block
            .as_ref()
            .map(|selection| {
                format!(
                    "{} · ({}, {}, {})",
                    selection.block, selection.x, selection.y, selection.z
                )
            })
            .unwrap_or_else(|| {
                tr(
                    locale,
                    "尚未在预览中选择方块",
                    "No block selected in preview",
                )
                .into()
            })
            .into(),
    );
    let reconstruction_workflow = CampusReconstructionWorkflow::projection(project);
    ui.set_detailed_active(reconstruction_workflow.mode == DesktopMode::Detailed);
    ui.set_can_enter_detailed(matches!(
        reconstruction_workflow.detailed_building_handoff,
        DetailedBuildingHandoff::Ready { .. }
    ));
    let detailed_workspace = DetailedBuildingWorkspace::projection(project);
    ui.set_detailed_task(match detailed_workspace.task {
        DetailedBuildingWorkspaceTask::BlockedNoReviewedBuildingSlot => 0,
        DetailedBuildingWorkspaceTask::SelectBuilding => 1,
        DetailedBuildingWorkspaceTask::ChooseEvidenceOrTemplate => 2,
        DetailedBuildingWorkspaceTask::ReviewFacadeRules => 3,
        DetailedBuildingWorkspaceTask::ReviewPreview => 4,
    });
    let foundation_workflow = FoundationWorkflow::projection(project);
    ui.set_active_step(
        FoundationStep::ALL
            .iter()
            .position(|step| *step == foundation_workflow.step)
            .unwrap_or(0) as i32,
    );
    ui.set_can_review_foundation(foundation_workflow.can_enter_review);
    ui.set_can_generate_foundation(foundation_workflow.can_enter_generate);
    let (title, help, primary) = if reconstruction_workflow.mode == DesktopMode::Detailed {
        if english {
            (
                "Detailed building template",
                "Measured footprint, height, and floors remain fixed. The template controls blocks, windows, and facade character only.",
                "",
            )
        } else {
            (
                "精细建筑模板",
                "实测轮廓、高度与楼层保持不变；模板只控制方块、窗户与墙面质感。",
                "",
            )
        }
    } else {
        page_copy(project.foundation_step, locale)
    };
    ui.set_page_title(title.into());
    ui.set_page_help(help.into());
    ui.set_primary_label(primary.into());
    if reconstruction_workflow.mode == DesktopMode::Detailed {
        let (task_title, task_help) = match (detailed_workspace.task, english) {
            (DetailedBuildingWorkspaceTask::SelectBuilding, true) => (
                "Choose a building",
                "Start from one building already reviewed in Foundation.",
            ),
            (DetailedBuildingWorkspaceTask::SelectBuilding, false) => (
                "选择要精修的建筑",
                "从地基模式已经审阅确认的建筑中选择一栋。",
            ),
            (DetailedBuildingWorkspaceTask::ChooseEvidenceOrTemplate, true) => (
                "Match photos and template",
                "Building use is inferred from its name and map tags. Add photos when available, then choose the closest Arnis template.",
            ),
            (DetailedBuildingWorkspaceTask::ChooseEvidenceOrTemplate, false) => (
                "匹配照片与模板",
                "系统根据建筑名称与地图标签自动识别用途；有照片时补充照片，然后选择最接近的 Arnis 模板。",
            ),
            (DetailedBuildingWorkspaceTask::ReviewFacadeRules, true) => (
                "Review editable facade rules",
                "Keep measured geometry fixed, adjust the facade parameters, then generate a preview.",
            ),
            (DetailedBuildingWorkspaceTask::ReviewFacadeRules, false) => (
                "审阅可编辑立面规则",
                "实测轮廓、高度和楼层保持不变；调整立面参数后生成预览。",
            ),
            (DetailedBuildingWorkspaceTask::ReviewPreview, true) => (
                "Review and confirm preview",
                "Inspect the generated building, correct blocks if needed, then confirm or export.",
            ),
            (DetailedBuildingWorkspaceTask::ReviewPreview, false) => (
                "审阅并确认预览",
                "检查生成结果，必要时修正方块，然后确认版本或导出。",
            ),
            (_, true) => (
                "Detailed building unavailable",
                "Review a building in Foundation first.",
            ),
            (_, false) => ("单栋精修暂不可用", "请先在地基模式确认至少一栋建筑。"),
        };
        ui.set_page_title(task_title.into());
        ui.set_page_help(task_help.into());
    }
    ui.set_save_status(
        if state.last_error.is_some() {
            tr(
                locale,
                "已从恢复副本打开，请保存修复主文件",
                "Recovered from backup; save to repair the primary file",
            )
        } else if state.dirty {
            tr(locale, "待保存", "Unsaved changes")
        } else {
            tr(locale, "已保存", "Saved")
        }
        .into(),
    );
    ui.set_project_summary(
        if english {
            format!(
                "{} candidates · {} accepted features · {} Building Slots",
                project.candidates.len(),
                project.features.len(),
                project.building_slots.len()
            )
        } else {
            format!(
                "{} 个候选 · {} 个已采用地物 · {} 个建筑槽位",
                project.candidates.len(),
                project.features.len(),
                project.building_slots.len()
            )
        }
        .into(),
    );
    let current_kind = match project.foundation_step {
        FoundationStep::Building => Some(FeatureKind::Building),
        FoundationStep::Road => Some(FeatureKind::Road),
        FoundationStep::Water => Some(FeatureKind::Water),
        FoundationStep::Vegetation => Some(FeatureKind::Vegetation),
        FoundationStep::Sports => Some(FeatureKind::Sports),
        _ => None,
    };
    let filtered_candidates = project
        .candidates
        .iter()
        .filter(|candidate| current_kind.is_none() || current_kind == Some(candidate.kind))
        .filter(|candidate| candidate_matches_filter(candidate, state.candidate_filter))
        .collect::<Vec<_>>();
    const CANDIDATE_PAGE_SIZE: usize = 8;
    let total_pages = filtered_candidates
        .len()
        .div_ceil(CANDIDATE_PAGE_SIZE)
        .max(1);
    let page = state.candidate_page.min(total_pages - 1);
    let candidates = filtered_candidates
        .into_iter()
        .skip(page * CANDIDATE_PAGE_SIZE)
        .take(CANDIDATE_PAGE_SIZE)
        .map(|candidate| CandidateRow {
            id: candidate.id.clone().into(),
            name: candidate.name.clone().into(),
            meta: format!("{} · {}", candidate.source, candidate.confidence).into(),
            status: match candidate.review {
                ReviewDecision::Pending => tr(locale, "待审核", "Pending"),
                ReviewDecision::Accepted => tr(locale, "已接受", "Accepted"),
                ReviewDecision::Rejected => tr(locale, "已拒绝", "Rejected"),
            }
            .into(),
            pending: candidate.review == ReviewDecision::Pending,
        })
        .collect::<Vec<_>>();
    ui.set_candidates(ModelRc::new(VecModel::from(candidates)));
    ui.set_selected_candidate_filter(
        CandidateConfidenceFilter::ALL
            .iter()
            .position(|filter| *filter == state.candidate_filter)
            .unwrap_or(0) as i32,
    );
    ui.set_candidate_page(page as i32);
    ui.set_candidate_pages(total_pages as i32);
    if let Some(candidate) = state.selected_candidate_id.as_deref().and_then(|id| {
        project
            .candidates
            .iter()
            .find(|candidate| candidate.id == id)
    }) {
        let tags = candidate
            .tags
            .iter()
            .take(12)
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(" · ");
        ui.set_selected_candidate_name(candidate.name.clone().into());
        ui.set_candidate_name_draft(candidate.name.clone().into());
        ui.set_selected_candidate_is_building(candidate.kind == FeatureKind::Building);
        ui.set_selected_candidate_details(
            if english {
                format!(
                    "ID {} · Source {} · Confidence {} · {} points{}",
                    candidate.id,
                    candidate.source,
                    candidate.confidence,
                    candidate.points.len(),
                    if tags.is_empty() {
                        String::new()
                    } else {
                        format!(" · Tags {tags}")
                    }
                )
            } else {
                format!(
                    "ID {} · 来源 {} · 置信度 {} · {} 个节点{}",
                    candidate.id,
                    candidate.source,
                    candidate.confidence,
                    candidate.points.len(),
                    if tags.is_empty() {
                        String::new()
                    } else {
                        format!(" · 标签 {tags}")
                    }
                )
            }
            .into(),
        );
    } else {
        ui.set_selected_candidate_name(tr(locale, "未选择候选", "No candidate selected").into());
        ui.set_selected_candidate_details("".into());
        ui.set_selected_candidate_is_building(false);
    }
    let suppression_index = state
        .selected_suppression
        .min(project.building_suppressions.len().saturating_sub(1));
    ui.set_selected_building_suppression(suppression_index as i32);
    ui.set_building_suppressions(ModelRc::new(VecModel::from(
        project
            .building_suppressions
            .iter()
            .map(|record| SharedString::from(format!("{} · {}", record.source_id, record.reason)))
            .collect::<Vec<_>>(),
    )));
    let slots = if project.building_slots.is_empty() {
        vec![SharedString::from(tr(
            locale,
            "暂无已审核建筑",
            "No reviewed buildings",
        ))]
    } else {
        project
            .building_slots
            .iter()
            .map(|slot| SharedString::from(slot.name.as_str()))
            .collect()
    };
    ui.set_building_slots(ModelRc::new(VecModel::from(slots)));
    let selected_slot = project
        .detailed
        .selected_slot_id
        .as_deref()
        .and_then(|id| project.building_slots.iter().position(|slot| slot.id == id))
        .unwrap_or(0);
    ui.set_selected_slot(selected_slot as i32);
    let selected_measurements = project.building_slots.get(selected_slot);
    let selected_slot_id = selected_measurements.map(|slot| slot.id.as_str());
    let template_proposals = selected_slot_id
        .map(|slot_id| project.template_proposals_for_slot(slot_id))
        .unwrap_or_default();
    let selected_template_id = selected_slot_id.and_then(|slot_id| {
        project
            .detailed
            .selected_templates
            .iter()
            .find(|selection| selection.slot_id == slot_id)
            .map(|selection| selection.template.id.as_str())
    });
    let selected_template_proposal = template_proposals
        .iter()
        .position(|proposal| Some(proposal.template.id.as_str()) == selected_template_id)
        .unwrap_or(0);
    ui.set_template_proposals(ModelRc::new(VecModel::from(
        if template_proposals.is_empty() {
            vec![SharedString::from(tr(
                locale,
                "请选择建筑以生成模板提案",
                "Choose a building to generate template proposals",
            ))]
        } else {
            template_proposals
                .iter()
                .map(|proposal| {
                    SharedString::from(format!(
                        "{} · {}% · {}",
                        proposal.template.label, proposal.confidence, proposal.rationale
                    ))
                })
                .collect()
        },
    )));
    ui.set_selected_template_proposal(selected_template_proposal as i32);
    ui.set_function_classification_summary(
        selected_slot_id
            .and_then(|slot_id| project.classification_for_slot(slot_id))
            .map(|classification| {
                let reason = classification
                    .reasons
                    .first()
                    .map(String::as_str)
                    .unwrap_or("");
                if english {
                    format!(
                        "Inferred function: {} · {}% · {}",
                        classification.function.label(),
                        classification.confidence,
                        reason
                    )
                } else {
                    format!(
                        "自动用途：{} · {}% · {}",
                        classification.function.label(),
                        classification.confidence,
                        reason
                    )
                }
            })
            .unwrap_or_else(|| {
                tr(
                    locale,
                    "尚未识别用途；选择建筑后将根据名称和地图标签自动匹配。",
                    "No function inferred yet. Select a building to match its name and map tags.",
                )
                .into()
            })
            .into(),
    );
    ui.set_facade_rule_summary(
        selected_slot_id
            .and_then(|slot_id| {
                project
                    .detailed
                    .facade_drafts
                    .iter()
                    .rev()
                    .find(|draft| draft.slot_id == slot_id)
            })
            .map(|draft| {
                if english {
                    format!(
                        "Editable facade rule draft · {} rules · {}% template confidence",
                        draft.rules.len(),
                        draft.confidence
                    )
                } else {
                    format!(
                        "可编辑立面规则草案 · {} 条规则 · 模板置信度 {}%",
                        draft.rules.len(),
                        draft.confidence
                    )
                }
            })
            .unwrap_or_else(|| {
                tr(
                    locale,
                    "选择一个模板后建立可编辑立面规则草案。",
                    "Select a template to create an editable facade rule draft.",
                )
                .into()
            })
            .into(),
    );
    let local_evidence_count = selected_slot_id
        .map(|slot_id| {
            project
                .detailed
                .evidence_assets
                .iter()
                .filter(|asset| asset.slot_id == slot_id)
                .count()
        })
        .unwrap_or(0);
    ui.set_local_evidence_summary(
        if english {
            format!("{local_evidence_count} local photo(s) · kept beside the project")
        } else {
            format!("{local_evidence_count} 张本地照片 · 与项目并列保存")
        }
        .into(),
    );
    let external_models = project
        .detailed
        .external_models
        .iter()
        .filter(|review| Some(review.slot_id.as_str()) == selected_slot_id)
        .collect::<Vec<_>>();
    let external_index = state
        .selected_external_model
        .min(external_models.len().saturating_sub(1));
    ui.set_external_models(ModelRc::new(VecModel::from(
        if external_models.is_empty() {
            vec![SharedString::from(tr(
                locale,
                "暂无外部模型候选",
                "No external model candidates",
            ))]
        } else {
            external_models
                .iter()
                .map(|review| SharedString::from(format!("{} · {}", review.source, review.title)))
                .collect()
        },
    )));
    ui.set_selected_external_model(external_index as i32);
    if let Some(review) = external_models.get(external_index) {
        let eligibility = if english {
            match review.eligibility {
                campus_state::ExternalModelEligibility::Eligible => "License permits adaptation",
                campus_state::ExternalModelEligibility::Blocked => "License blocks adaptation",
            }
        } else {
            review.eligibility.label()
        };
        ui.set_external_model_summary(
            format!(
                "{} · {} · {} {} · {} {} · {} · {} {}×{}×{}m · {}",
                review.source,
                review.source_url,
                tr(locale, "作者", "Author"),
                if review.author.is_empty() {
                    tr(locale, "未知", "Unknown")
                } else {
                    review.author.as_str()
                },
                tr(locale, "许可", "License"),
                review
                    .license_name
                    .as_deref()
                    .unwrap_or(tr(locale, "缺失", "Missing")),
                eligibility,
                tr(locale, "尺寸", "Size"),
                review
                    .width_m
                    .map(|value| format!("{value:.1}"))
                    .unwrap_or_else(|| "?".into()),
                review
                    .height_m
                    .map(|value| format!("{value:.1}"))
                    .unwrap_or_else(|| "?".into()),
                review
                    .length_m
                    .map(|value| format!("{value:.1}"))
                    .unwrap_or_else(|| "?".into()),
                external_decision_label(review.decision, locale)
            )
            .into(),
        );
        ui.set_selected_external_decision(
            ExternalModelDecision::ALL
                .iter()
                .position(|decision| *decision == review.decision)
                .unwrap_or(0) as i32,
        );
    } else {
        ui.set_external_model_summary(
            tr(
                locale,
                "当前建筑来源对象没有 3DMR/Wikidata 模型标签。",
                "The current source object has no 3DMR/Wikidata model tags.",
            )
            .into(),
        );
        ui.set_selected_external_decision(0);
    }
    let source_conflicts = project
        .detailed
        .source_conflicts
        .iter()
        .filter(|conflict| Some(conflict.slot_id.as_str()) == selected_slot_id)
        .collect::<Vec<_>>();
    let conflict_index = state
        .selected_source_conflict
        .min(source_conflicts.len().saturating_sub(1));
    ui.set_source_conflicts(ModelRc::new(VecModel::from(
        if source_conflicts.is_empty() {
            vec![SharedString::from(tr(
                locale,
                "暂无来源冲突",
                "No source conflicts",
            ))]
        } else {
            source_conflicts
                .iter()
                .map(|conflict| {
                    SharedString::from(format!("{} · {}", conflict.severity, conflict.kind))
                })
                .collect()
        },
    )));
    ui.set_selected_source_conflict(conflict_index as i32);
    if let Some(conflict) = source_conflicts.get(conflict_index) {
        ui.set_source_conflict_summary(
            format!(
                "{} · {} · {}",
                conflict.summary,
                conflict_decision_label(conflict.decision, locale),
                if conflict.decision_reason.is_empty() {
                    tr(locale, "尚无决策理由", "No decision reason yet")
                } else {
                    conflict.decision_reason.as_str()
                }
            )
            .into(),
        );
        ui.set_selected_source_conflict_decision(
            SourceConflictDecision::ALL
                .iter()
                .position(|decision| *decision == conflict.decision)
                .unwrap_or(0) as i32,
        );
    } else {
        ui.set_source_conflict_summary(
            tr(
                locale,
                "当前建筑没有待处理的来源冲突。",
                "The current building has no unresolved source conflicts.",
            )
            .into(),
        );
        ui.set_selected_source_conflict_decision(0);
    }
    ui.set_observed_evidence_summary(
        selected_measurements
            .map(|slot| {
                let source = project
                    .candidates
                    .iter()
                    .find(|candidate| candidate.id == slot.id)
                    .map(|candidate| {
                        if english {
                            format!(
                                "{} · {} confidence · {}",
                                candidate.source, candidate.confidence, candidate.id
                            )
                        } else {
                            format!(
                                "{} · {}置信度 · {}",
                                candidate.source, candidate.confidence, candidate.id
                            )
                        }
                    })
                    .unwrap_or_else(|| tr(locale, "项目审核槽位", "Project-reviewed slot").into());
                if english {
                    format!(
                        "Footprint {} points · Height {} · Floors {} · Roof {} · Source {}",
                        slot.footprint.len(),
                        slot.height_m
                            .map(|value| format!("{value:.2}m"))
                            .unwrap_or_else(|| "Unknown".into()),
                        slot.floors
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "Unknown".into()),
                        slot.roof_shape.as_deref().unwrap_or("Unknown"),
                        source
                    )
                } else {
                    format!(
                        "轮廓 {} 点 · 高度 {} · 楼层 {} · 屋顶 {} · 来源 {}",
                        slot.footprint.len(),
                        slot.height_m
                            .map(|value| format!("{value:.2}m"))
                            .unwrap_or_else(|| "未知".into()),
                        slot.floors
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "未知".into()),
                        slot.roof_shape.as_deref().unwrap_or("未知"),
                        source
                    )
                }
            })
            .unwrap_or_else(|| tr(locale, "尚未选择建筑槽位", "No Building Slot selected").into())
            .into(),
    );
    let latest_refinement =
        selected_measurements.and_then(|slot| project.latest_refinement(&slot.id));
    ui.set_refinement_summary(
        latest_refinement
            .map(|refinement| {
                format!(
                    "v{} · {} · {}",
                    refinement.version,
                    refinement_status_label(refinement.status, locale),
                    if english {
                        ArnisStylePreset::ALL
                            .iter()
                            .position(|preset| *preset == refinement.style_preset)
                            .and_then(|index| {
                                [
                                    "House",
                                    "Residential",
                                    "Farm",
                                    "Commercial",
                                    "Office",
                                    "Hotel",
                                    "Industrial",
                                    "Warehouse",
                                    "School",
                                    "Hospital",
                                    "Religious",
                                    "Historic",
                                    "Tower",
                                    "Garage",
                                    "Shed",
                                    "Greenhouse",
                                    "Tall Building",
                                    "Glassy Skyscraper",
                                    "Modern Skyscraper",
                                ]
                                .get(index)
                                .copied()
                            })
                            .unwrap_or("Unknown")
                    } else {
                        refinement.style_preset.label()
                    }
                )
            })
            .unwrap_or_else(|| tr(locale, "尚无生成版本", "No generated version").into())
            .into(),
    );
    ui.set_can_confirm_refinement(
        latest_refinement
            .is_some_and(|refinement| refinement.status == campus_state::RefinementStatus::Draft),
    );
    ui.set_semantic_feature_summary(
        latest_refinement
            .map(|refinement| {
                let records = project
                    .detailed
                    .semantic_features
                    .iter()
                    .filter(|record| record.refinement_id == refinement.id)
                    .collect::<Vec<_>>();
                if records.is_empty() {
                    tr(locale, "尚未标注识别特征", "No semantic features annotated").into()
                } else {
                    let labels = records
                        .iter()
                        .rev()
                        .take(3)
                        .map(|record| record.label.as_str())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    if english {
                        format!("{} items · {labels}", records.len())
                    } else {
                        format!("{} 项 · {labels}", records.len())
                    }
                }
            })
            .unwrap_or_else(|| {
                tr(
                    locale,
                    "请先生成一个 refinement 草稿",
                    "Generate a refinement draft first",
                )
                .into()
            })
            .into(),
    );
    ui.set_generated_interpretation_summary(
        project
            .detailed
            .generated_path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<arnis_core::GeneratedBuilding>(&bytes).ok())
            .map(|generated| {
                if english {
                    format!(
                        "{}×{}×{} · {} non-air blocks · {} · scale {:.2} · {} floors · {} roof · {} corrections",
                        generated.width,
                        generated.height,
                        generated.length,
                        generated.report.non_air_blocks,
                        generated.report.generator,
                        generated.report.blocks_per_meter,
                        generated.report.floor_count,
                        generated.report.roof_shape,
                        generated.report.correction_notes.len()
                    )
                } else {
                    format!(
                        "{}×{}×{} · {} 非空气方块 · {} · 比例 {:.2} · {} 层 · {} 屋顶 · {} 条修正记录",
                        generated.width,
                        generated.height,
                        generated.length,
                        generated.report.non_air_blocks,
                        generated.report.generator,
                        generated.report.blocks_per_meter,
                        generated.report.floor_count,
                        generated.report.roof_shape,
                        generated.report.correction_notes.len()
                    )
                }
            })
            .unwrap_or_else(|| {
                tr(
                    locale,
                    "尚未生成解释结果",
                    "No generated interpretation",
                )
                .into()
            })
            .into(),
    );
    ui.set_measured_height(
        selected_measurements
            .and_then(|slot| slot.height_m)
            .map(|value| format!("{value:.2}"))
            .unwrap_or_default()
            .into(),
    );
    ui.set_measured_floors(
        selected_measurements
            .and_then(|slot| slot.floors)
            .map(|value| value.to_string())
            .unwrap_or_default()
            .into(),
    );
    ui.set_measured_roof(
        selected_measurements
            .and_then(|slot| slot.roof_shape.clone())
            .unwrap_or_default()
            .into(),
    );
    ui.set_palette_summary(generated_palette_summary(project, locale).into());
    let selected_style = ArnisStylePreset::ALL
        .iter()
        .position(|preset| *preset == project.detailed.style_preset)
        .unwrap_or(8);
    ui.set_selected_style(selected_style as i32);
    let foundation_style = FoundationStylePreset::ALL
        .iter()
        .position(|preset| *preset == project.foundation_style_preset)
        .unwrap_or(0);
    ui.set_selected_foundation_style(foundation_style as i32);
    ui.set_foundation_style_name(project.foundation_style_pack.name.clone().into());
    ui.set_window_density(project.detailed.window_density as f32);
    ui.set_wall_depth(project.detailed.wall_depth as f32);
    ui.set_orientation_degrees(project.orientation_degrees as f32);
    ui.set_blocks_per_meter(project.blocks_per_meter as f32);
    ui.set_can_undo(state.can_undo());
    ui.set_can_redo(state.can_redo());
    sync_shortcut_rows(ui, Some(state), None);
}

fn candidate_matches_filter(candidate: &MapCandidate, filter: CandidateConfidenceFilter) -> bool {
    match filter {
        CandidateConfidenceFilter::All => candidate.review == ReviewDecision::Pending,
        CandidateConfidenceFilter::High => {
            candidate.review == ReviewDecision::Pending
                && candidate.confidence == CandidateConfidence::High
        }
        CandidateConfidenceFilter::Medium => {
            candidate.review == ReviewDecision::Pending
                && candidate.confidence == CandidateConfidence::Medium
        }
        CandidateConfidenceFilter::Low => {
            candidate.review == ReviewDecision::Pending
                && candidate.confidence == CandidateConfidence::Low
        }
        CandidateConfidenceFilter::Confirmed => candidate.review == ReviewDecision::Accepted,
        CandidateConfidenceFilter::Rejected => candidate.review == ReviewDecision::Rejected,
    }
}

fn save_and_sync(
    ui: &AppWindow,
    state: &Rc<RefCell<DesktopApplicationState>>,
) -> Result<(), String> {
    autosave(state)?;
    sync_ui(ui, &state.borrow());
    Ok(())
}

#[track_caller]
fn set_error(ui: &AppWindow, message: impl AsRef<str>) {
    set_error_for(ui, "desktop.operation", message);
}

#[track_caller]
fn set_error_for(ui: &AppWindow, event: &str, message: impl AsRef<str>) {
    let location = std::panic::Location::caller();
    let source = format!("{}:{}", location.file(), location.line());
    let message = v11_guidance::sanitise_registered_diagnostic_value("message", message.as_ref());
    let record = diagnostics::record(
        diagnostics::DiagnosticLevel::Error,
        event,
        &message,
        &[("source", source.as_str())],
    );
    let incident_id = record
        .as_ref()
        .map(|record| record.id.as_str())
        .unwrap_or("log-unavailable");
    let log_path = record
        .as_ref()
        .map(|record| record.log_path.display().to_string())
        .unwrap_or_else(|| {
            diagnostics::log_directory()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "diagnostic log unavailable".into())
        });
    ui.set_error_visible(true);
    ui.set_error_summary(
        if ui.get_english() {
            format!("Operation failed: {message}")
        } else {
            format!("操作失败：{message}")
        }
        .into(),
    );
    ui.set_error_details(
        if ui.get_english() {
            format!("Incident {incident_id} · {log_path}")
        } else {
            format!("事件编号 {incident_id} · {log_path}")
        }
        .into(),
    );
    ui.set_save_status(
        if ui.get_english() {
            "Operation failed"
        } else {
            "操作失败"
        }
        .into(),
    );
}

fn set_status(ui: &AppWindow, zh: impl Into<String>, en: impl Into<String>) {
    ui.set_save_status(
        if ui.get_english() {
            en.into()
        } else {
            zh.into()
        }
        .into(),
    );
}

#[track_caller]
fn set_localized_error(ui: &AppWindow, event: &str, zh: impl Into<String>, en: impl Into<String>) {
    let message = if ui.get_english() {
        en.into()
    } else {
        zh.into()
    };
    set_error_for(ui, event, message);
}

fn compile_foundation(project: &CampusProject) -> Result<campus_export::VoxelModel, String> {
    let reviewed = campus_export::ReviewedCampusModel::from(project);
    campus_export::foundation_model_from_reviewed(&reviewed)
}

fn generate_foundation_preview(
    state: &Rc<RefCell<DesktopApplicationState>>,
) -> Result<(PathBuf, String), String> {
    let project = state.borrow().project.clone().ok_or("请先创建项目")?;
    let model = compile_foundation(&project)?;
    let directory = generated_model_dir();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("foundation-preview.json");
    campus_export::write_preview_model(&path, &model)?;
    state.borrow_mut().mutate_project(|project| {
        project.foundation_preview_path = Some(path.clone());
    });
    Ok((path, format!("{} · Foundation", project.name)))
}

fn generate_detailed_model(
    state: &Rc<RefCell<DesktopApplicationState>>,
) -> Result<(PathBuf, String), String> {
    generate_detailed_model_to(state, &generated_model_dir())
}

fn generate_detailed_model_to(
    state: &Rc<RefCell<DesktopApplicationState>>,
    output_directory: &Path,
) -> Result<(PathBuf, String), String> {
    let (slot, rules, scale, version) = {
        let borrowed = state.borrow();
        let project = borrowed.project.as_ref().ok_or("请先创建项目")?;
        let slot = project
            .detailed
            .selected_slot_id
            .as_deref()
            .and_then(|id| project.building_slots.iter().find(|slot| slot.id == id))
            .or_else(|| project.building_slots.first())
            .cloned()
            .ok_or("请先在地基模式接受至少一个建筑候选")?;
        let version = project.next_refinement_version(&slot.id);
        let rules = DetailedBuildingRuleStack::compile(project, &slot.id)?;
        (slot, rules, project.blocks_per_meter, version)
    };
    let mut correction_notes = vec![format!(
        "Detailed Building Rule Stack: {}",
        if rules.applied_rule_ids.is_empty() {
            "legacy-compatible baseline".into()
        } else {
            rules.applied_rule_ids.join(",")
        }
    )];
    if rules.template_provisional {
        correction_notes.push("Template-Provisional Detailed Building".into());
    }
    let generated = arnis_core::generate_building(GenerateBuildingRequest {
        candidate_id: slot.id.clone(),
        source: "campus-project".into(),
        components: vec![FootprintComponent {
            exterior: slot
                .footprint
                .iter()
                .map(|point| arnis_core::GeoPoint {
                    lng: point.lng,
                    lat: point.lat,
                })
                .collect(),
            interior_rings: Vec::new(),
        }],
        height_m: slot.height_m,
        floors: rules.floors,
        roof_shape: rules.roof_shape.clone(),
        blocks_per_meter: scale,
        seed: 42,
        materials: MaterialOverrides {
            wall: rules.wall_material.clone(),
            accent: rules.accent_material.clone(),
            ..MaterialOverrides::default()
        },
        correction_notes,
        parts: Vec::new(),
        style_preset: rules.style_preset.slug().into(),
        window_density: rules.window_density,
        wall_depth: rules.wall_depth,
    })?;
    std::fs::create_dir_all(output_directory).map_err(|error| error.to_string())?;
    let safe_id = slot
        .id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let path = output_directory.join(format!("{safe_id}-v{version}.arnis.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&generated).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    state.borrow_mut().mutate_project(|project| {
        project.detailed.selected_slot_id = Some(slot.id.clone());
        project.record_refinement_draft(&slot.id, version, path.clone());
    });
    Ok((path, slot.name))
}

fn generated_palette_summary(
    project: &campus_state::CampusProject,
    locale: DesktopLocale,
) -> String {
    let Some(path) = &project.detailed.generated_path else {
        return tr(locale, "尚未生成模型", "No model generated").into();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return tr(
            locale,
            "生成文件已移动，请重新生成",
            "Generated file was moved; generate again",
        )
        .into();
    };
    let Ok(generated) = serde_json::from_slice::<arnis_core::GeneratedBuilding>(&bytes) else {
        return tr(
            locale,
            "生成文件格式无效",
            "Generated file format is invalid",
        )
        .into();
    };
    generated
        .palette
        .iter()
        .filter(|block| block.as_str() != "minecraft:air")
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn normalize_minecraft_block(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, ':' | '_' | '[' | ']' | '=' | ',')
        })
    {
        return Err("方块 ID 无效".into());
    }
    Ok(if value.contains(':') {
        value
    } else {
        format!("minecraft:{value}")
    })
}

fn replace_generated_block(path: &PathBuf, source: &str, target: &str) -> Result<usize, String> {
    let source = normalize_minecraft_block(source)?;
    let target = normalize_minecraft_block(target)?;
    if source == "minecraft:air" {
        return Err("V1 不允许把全部空气作为替换来源".into());
    }
    if source == target {
        return Err("原方块和新方块相同".into());
    }
    let mut generated: arnis_core::GeneratedBuilding =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let sources = generated
        .palette
        .iter()
        .enumerate()
        .filter(|(_, block)| **block == source)
        .map(|(index, _)| index as u16)
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Err(format!("模型中没有 {source}"));
    }
    let target_index = generated
        .palette
        .iter()
        .position(|block| *block == target)
        .unwrap_or_else(|| {
            generated.palette.push(target);
            generated.palette.len() - 1
        }) as u16;
    let mut replaced = 0usize;
    for run in &mut generated.block_runs {
        if sources.contains(&run.palette_index) {
            replaced += run.run_length as usize;
            run.palette_index = target_index;
        }
    }
    let mut merged: Vec<arnis_core::BlockRun> = Vec::new();
    for run in generated.block_runs {
        if let Some(previous) = merged.last_mut() {
            if previous.palette_index == run.palette_index {
                previous.run_length += run.run_length;
                continue;
            }
        }
        merged.push(run);
    }
    generated.block_runs = merged;
    generated.report.correction_notes.push(format!(
        "batch replacement: {source} -> {}",
        generated.palette[target_index as usize]
    ));
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&generated).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(replaced)
}

fn replace_generated_block_at(
    path: &PathBuf,
    x: i32,
    y: i32,
    z: i32,
    target: &str,
) -> Result<String, String> {
    let target = normalize_minecraft_block(target)?;
    let mut generated: arnis_core::GeneratedBuilding =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if x < 0
        || y < 0
        || z < 0
        || x as usize >= generated.width
        || y as usize >= generated.height
        || z as usize >= generated.length
    {
        return Err("所选坐标超出生成模型".into());
    }
    let target_linear =
        x as usize + z as usize * generated.width + y as usize * generated.width * generated.length;
    let target_index = generated
        .palette
        .iter()
        .position(|block| *block == target)
        .unwrap_or_else(|| {
            generated.palette.push(target.clone());
            generated.palette.len() - 1
        }) as u16;
    let mut cursor = 0usize;
    let mut replaced_block = None;
    let mut edited = Vec::with_capacity(generated.block_runs.len() + 2);
    for run in generated.block_runs {
        let run_start = cursor;
        let run_end = cursor + run.run_length as usize;
        if replaced_block.is_none() && (run_start..run_end).contains(&target_linear) {
            let before = target_linear - run_start;
            let after = run_end - target_linear - 1;
            if before > 0 {
                edited.push(arnis_core::BlockRun {
                    palette_index: run.palette_index,
                    run_length: before as u32,
                });
            }
            replaced_block = generated.palette.get(run.palette_index as usize).cloned();
            edited.push(arnis_core::BlockRun {
                palette_index: target_index,
                run_length: 1,
            });
            if after > 0 {
                edited.push(arnis_core::BlockRun {
                    palette_index: run.palette_index,
                    run_length: after as u32,
                });
            }
        } else {
            edited.push(run);
        }
        cursor = run_end;
    }
    let replaced_block = replaced_block.ok_or("无法定位所选方块")?;
    let mut merged: Vec<arnis_core::BlockRun> = Vec::with_capacity(edited.len());
    for run in edited {
        if let Some(previous) = merged.last_mut() {
            if previous.palette_index == run.palette_index {
                previous.run_length += run.run_length;
                continue;
            }
        }
        merged.push(run);
    }
    generated.block_runs = merged;
    generated.report.correction_notes.push(format!(
        "single block edit: ({x}, {y}, {z}) {replaced_block} -> {target}"
    ));
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&generated).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(replaced_block)
}

#[derive(Clone, Copy)]
struct OccupiedBounds {
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
    min_z: usize,
    max_z: usize,
}

fn apply_semantic_feature(
    path: &PathBuf,
    kind: SemanticFeatureKind,
    side: SemanticFeatureSide,
    height_band: SemanticHeightBand,
    strength: SemanticStrength,
    label: &str,
    reason: &str,
) -> Result<(usize, String), String> {
    if label.trim().is_empty() || reason.trim().is_empty() {
        return Err("语义特征需要名称和证据理由".into());
    }
    let mut generated: arnis_core::GeneratedBuilding =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let total = generated.width * generated.height * generated.length;
    let mut blocks = Vec::with_capacity(total);
    for run in &generated.block_runs {
        blocks.extend(std::iter::repeat_n(
            run.palette_index,
            run.run_length as usize,
        ));
    }
    if blocks.len() != total {
        return Err("生成模型 RLE 尺寸无效".into());
    }
    let mut bounds = OccupiedBounds {
        min_x: generated.width,
        max_x: 0,
        min_y: generated.height,
        max_y: 0,
        min_z: generated.length,
        max_z: 0,
    };
    let mut occupied = false;
    for (index, palette_index) in blocks.iter().enumerate() {
        if *palette_index == 0 {
            continue;
        }
        occupied = true;
        let x = index % generated.width;
        let z = (index / generated.width) % generated.length;
        let y = index / (generated.width * generated.length);
        bounds.min_x = bounds.min_x.min(x);
        bounds.max_x = bounds.max_x.max(x);
        bounds.min_y = bounds.min_y.min(y);
        bounds.max_y = bounds.max_y.max(y);
        bounds.min_z = bounds.min_z.min(z);
        bounds.max_z = bounds.max_z.max(z);
    }
    if !occupied {
        return Err("生成模型没有可标注的非空气方块".into());
    }
    let preferred = match kind {
        SemanticFeatureKind::WindowBand => "minecraft:glass",
        SemanticFeatureKind::EntranceEmphasis => {
            if generated
                .palette
                .iter()
                .any(|block| block == "minecraft:dark_oak_door")
            {
                "minecraft:dark_oak_door"
            } else {
                "minecraft:polished_andesite"
            }
        }
        SemanticFeatureKind::RoofRidge => {
            if generated.report.roof_shape == "flat" {
                "minecraft:polished_andesite"
            } else {
                "minecraft:dark_oak_slab"
            }
        }
        SemanticFeatureKind::Cornice | SemanticFeatureKind::Frame => "minecraft:polished_andesite",
    };
    let palette_index = generated
        .palette
        .iter()
        .position(|block| block == preferred)
        .unwrap_or_else(|| {
            generated.palette.push(preferred.into());
            generated.palette.len() - 1
        }) as u16;
    let span_y = bounds.max_y.saturating_sub(bounds.min_y);
    let base_y = match height_band {
        SemanticHeightBand::Lower => bounds.min_y + span_y / 4,
        SemanticHeightBand::Middle => bounds.min_y + span_y / 2,
        SemanticHeightBand::Upper => bounds.min_y + span_y * 3 / 4,
        SemanticHeightBand::Roof => bounds.max_y,
    };
    let feature_width = match strength {
        SemanticStrength::Subtle => 3,
        SemanticStrength::Visible => 5,
        SemanticStrength::Strong => 7,
    };
    let half = feature_width / 2;
    let mut cells = Vec::new();
    if kind == SemanticFeatureKind::RoofRidge {
        let z = (bounds.min_z + bounds.max_z) / 2;
        cells.extend((bounds.min_x..=bounds.max_x).map(|x| (x, bounds.max_y, z)));
    } else if matches!(side, SemanticFeatureSide::East | SemanticFeatureSide::West) {
        let x = if side == SemanticFeatureSide::East {
            bounds.max_x
        } else {
            bounds.min_x
        };
        let center = (bounds.min_z + bounds.max_z) / 2;
        for z in center.saturating_sub(half)..=(center + half).min(bounds.max_z) {
            add_semantic_vertical_cells(&mut cells, x, base_y, z, kind, generated.height);
        }
    } else {
        let z = match side {
            SemanticFeatureSide::North => bounds.min_z,
            SemanticFeatureSide::South => bounds.max_z,
            _ => (bounds.min_z + bounds.max_z) / 2,
        };
        let center = (bounds.min_x + bounds.max_x) / 2;
        for x in center.saturating_sub(half)..=(center + half).min(bounds.max_x) {
            add_semantic_vertical_cells(&mut cells, x, base_y, z, kind, generated.height);
        }
    }
    cells.sort_unstable();
    cells.dedup();
    let mut affected = 0usize;
    for (x, y, z) in cells {
        let index = x + z * generated.width + y * generated.width * generated.length;
        if blocks[index] != palette_index {
            blocks[index] = palette_index;
            affected += 1;
        }
    }
    generated.block_runs = compress_palette_indices(&blocks);
    generated.report.correction_notes.push(format!(
        "semantic feature: {} · {} · {} block(s) · {}",
        kind.label(),
        affected,
        label.trim(),
        reason.trim()
    ));
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&generated).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok((affected, preferred.into()))
}

fn add_semantic_vertical_cells(
    cells: &mut Vec<(usize, usize, usize)>,
    x: usize,
    y: usize,
    z: usize,
    kind: SemanticFeatureKind,
    model_height: usize,
) {
    let offsets: &[i32] = match kind {
        SemanticFeatureKind::EntranceEmphasis => &[0, 1, 2],
        SemanticFeatureKind::WindowBand => &[-1, 0, 1],
        _ => &[0],
    };
    for offset in offsets {
        let target_y = (y as i32 + offset).clamp(0, model_height.saturating_sub(1) as i32) as usize;
        cells.push((x, target_y, z));
    }
}

fn compress_palette_indices(blocks: &[u16]) -> Vec<arnis_core::BlockRun> {
    let mut runs: Vec<arnis_core::BlockRun> = Vec::new();
    for palette_index in blocks {
        if let Some(previous) = runs.last_mut() {
            if previous.palette_index == *palette_index && previous.run_length < u32::MAX {
                previous.run_length += 1;
                continue;
            }
        }
        runs.push(arnis_core::BlockRun {
            palette_index: *palette_index,
            run_length: 1,
        });
    }
    runs
}

enum ToolUpdate {
    Status(String),
    Error {
        event: String,
        message: String,
    },
    PreviewBlockSelected {
        x: i32,
        y: i32,
        z: i32,
        block: String,
    },
    MapCamera {
        center: GeoPoint,
        zoom: f64,
        pitch: f64,
        rotation: f64,
    },
    MapPoint(GeoPoint),
    MapCampusTarget(CampusTargetEvidence),
    MapBoundary(Vec<GeoPoint>),
}

#[cfg(test)]
fn tool_event_ends_stream(event: &ToolEvent) -> bool {
    matches!(event, ToolEvent::Closed { .. })
}

#[derive(Clone)]
struct ToolSupervisor {
    processes: DesktopToolProcessSupervisor,
    updates: mpsc::Sender<ToolUpdate>,
}

struct MapLaunchRequest {
    title: String,
    view: MapViewState,
    boundary: Vec<GeoPoint>,
    js_api_key: String,
    security_code: String,
    purpose: MapPurpose,
    overlays: Vec<MapOverlay>,
    feature_kind: Option<String>,
    english: bool,
}

fn run_self_test(cycles: usize) -> Result<serde_json::Value, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable_dir = executable
        .parent()
        .ok_or("executable has no parent directory")?;
    for helper in ["campus-map.exe", "campus-preview.exe"] {
        if !executable_dir.join(helper).is_file() {
            return Err(format!("installed helper is missing: {helper}"));
        }
    }

    let random: u128 = rand::rng().random();
    let temp = std::env::temp_dir().join(format!("campus-v1-self-test-{random:032x}"));
    std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut state = DesktopApplicationState::default();
        state.new_project("V1 self-test", "华东师范大学普陀校区");
        state.mutate_project(|project| {
            project.boundary = vec![
                GeoPoint {
                    lng: 121.4000,
                    lat: 31.2300,
                },
                GeoPoint {
                    lng: 121.4003,
                    lat: 31.2300,
                },
                GeoPoint {
                    lng: 121.4003,
                    lat: 31.2297,
                },
                GeoPoint {
                    lng: 121.4000,
                    lat: 31.2297,
                },
            ];
            let building = project
                .add_manual_feature(FeatureKind::Building, project.boundary.clone())
                .expect("self-test building geometry");
            project
                .add_manual_feature(
                    FeatureKind::Road,
                    vec![
                        GeoPoint {
                            lng: 121.4000,
                            lat: 31.22985,
                        },
                        GeoPoint {
                            lng: 121.4003,
                            lat: 31.22985,
                        },
                    ],
                )
                .expect("self-test road geometry");
            for (kind, west, east, south, north) in [
                (FeatureKind::Water, 121.40003, 121.40010, 31.22973, 31.22980),
                (
                    FeatureKind::Vegetation,
                    121.40012,
                    121.40021,
                    31.22973,
                    31.22982,
                ),
                (
                    FeatureKind::Sports,
                    121.40022,
                    121.40029,
                    31.22973,
                    31.22982,
                ),
            ] {
                project
                    .add_manual_feature(
                        kind,
                        vec![
                            GeoPoint {
                                lng: west,
                                lat: north,
                            },
                            GeoPoint {
                                lng: east,
                                lat: north,
                            },
                            GeoPoint {
                                lng: east,
                                lat: south,
                            },
                            GeoPoint {
                                lng: west,
                                lat: south,
                            },
                        ],
                    )
                    .expect("self-test area geometry");
            }
            project.detailed.selected_slot_id = Some(building);
            if let Some(slot) = project.building_slots.first_mut() {
                slot.height_m = Some(18.0);
                slot.floors = Some(5);
                slot.roof_shape = Some("flat".into());
            }
        });

        let project_path = temp.join("self-test.campus.json");
        state.save_to(&project_path)?;
        state.mutate_project(|project| project.name = "V1 self-test second save".into());
        state.save()?;
        std::fs::write(&project_path, b"{corrupt").map_err(|error| error.to_string())?;
        let mut recovered = DesktopApplicationState::default();
        recovered.open(&project_path)?;
        if !recovered.dirty || recovered.project.is_none() {
            return Err("project recovery did not activate".into());
        }
        recovered.save()?;

        for preset in FoundationStylePreset::ALL {
            recovered
                .project
                .as_mut()
                .expect("recovered project")
                .apply_foundation_style(preset);
            let model = compile_foundation(recovered.project.as_ref().expect("project"))?;
            if !model.blocks.iter().any(|block| *block != 0) {
                return Err(format!("Foundation preset {:?} produced no blocks", preset));
            }
        }
        let foundation_model = compile_foundation(recovered.project.as_ref().expect("project"))?;
        let foundation_path = temp.join("foundation.schem");
        campus_export::write_schematic(
            &foundation_path,
            "self-test-foundation",
            &foundation_model,
        )?;

        let shared = Rc::new(RefCell::new(recovered));
        let cycles = cycles.clamp(1, 20);
        let mut generated_count = 0usize;
        let mut generated_paths = Vec::new();
        for _ in 0..cycles {
            for preset in ArnisStylePreset::ALL {
                shared
                    .borrow_mut()
                    .mutate_project(|project| project.detailed.style_preset = preset);
                let (path, _) = generate_detailed_model(&shared)?;
                let generated: arnis_core::GeneratedBuilding = serde_json::from_slice(
                    &std::fs::read(&path).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                if generated.report.non_air_blocks == 0 {
                    return Err(format!("Arnis preset {:?} produced no blocks", preset));
                }
                generated_paths.push(path);
                generated_count += 1;
            }
        }
        let generated_path = shared
            .borrow()
            .project
            .as_ref()
            .and_then(|project| project.detailed.generated_path.clone())
            .ok_or("self-test detailed output missing")?;
        let generated: arnis_core::GeneratedBuilding = serde_json::from_slice(
            &std::fs::read(&generated_path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let detailed_model = campus_export::model_from_runs(
            generated.width,
            generated.height,
            generated.length,
            generated.palette,
            generated
                .block_runs
                .into_iter()
                .map(|run| (run.palette_index, run.run_length)),
        )?;
        let detailed_path = temp.join("detailed.schem");
        campus_export::write_schematic(&detailed_path, "self-test-detailed", &detailed_model)?;
        if std::fs::metadata(&foundation_path)
            .map_err(|error| error.to_string())?
            .len()
            < 64
            || std::fs::metadata(&detailed_path)
                .map_err(|error| error.to_string())?
                .len()
                < 64
        {
            return Err("self-test schematic output is unexpectedly small".into());
        }
        for path in generated_paths {
            let _ = std::fs::remove_file(path);
        }
        Ok(serde_json::json!({
            "status": "pass",
            "offline": true,
            "recovery": true,
            "campus": "华东师范大学普陀校区",
            "featureKinds": 5,
            "foundationPresets": FoundationStylePreset::ALL.len(),
            "arnisGenerations": generated_count,
            "helpers": ["campus-map.exe", "campus-preview.exe"]
        }))
    })();
    let _ = std::fs::remove_dir_all(&temp);
    result
}

fn main() -> Result<(), slint::PlatformError> {
    let production_acquisition_client =
        match v11_acquisition_client::production_client_if_configured(
            std::env::var("CAMPUS_ACQUISITION_SERVICE_URL")
                .ok()
                .as_deref(),
        ) {
            Ok(client) => client,
            Err(error) => {
                eprintln!("V1.1 production acquisition client failed: {error}");
                None
            }
        };
    #[cfg(debug_assertions)]
    {
        if let Err(error) = v11_acquisition_client::bootstrap_fixture_if_enabled(
            true,
            std::env::var("CAMPUS_V11_ACQUISITION_FIXTURE")
                .ok()
                .as_deref(),
        ) {
            eprintln!("V1.1 development acquisition fixture failed: {error}");
        }
    }
    let project_kernel_gate = std::env::var("CAMPUS_V11_PROJECT_KERNEL").ok();
    let (v11_launcher, v11_launcher_error) = if project_kernel_gate.as_deref() == Some("1") {
        match (|| {
            let capability = campus_state::V11ConstructionCapability::request(
                cfg!(debug_assertions),
                project_kernel_gate.as_deref(),
            )?;
            let mut launcher = CampusProjectLauncher::open(
                &app_data_dir(),
                capability,
                campus_state::InstallationId::new("campus-native")?,
            )?;
            if launcher.offered_campus().is_none() {
                launcher.select_campus_candidate(
                    CampusScope::new(
                        "gaode:B00155J6JH",
                        "华东师范大学普陀校区",
                        [121.395, 31.202],
                    )?
                    .with_gaode_poi_id("B00155J6JH")?,
                );
            }
            Ok::<_, String>(Rc::new(RefCell::new(launcher)))
        })() {
            Ok(launcher) => (Some(launcher), None),
            Err(error) => (None, Some(error)),
        }
    } else {
        (None, None)
    };
    let v11_tracer_error = match v11_tracer_bullet::bootstrap_if_enabled(
        &app_data_dir(),
        std::env::var("CAMPUS_V11_FIXED_TRACER").ok().as_deref(),
        production_acquisition_client.as_ref(),
    ) {
        Ok(Some(report)) => {
            eprintln!(
                "V1.1 fixed-Dataset tracer {} exported {} bytes to {} with manifest {}",
                report.project_id.as_str(),
                report.schematic_bytes,
                report.schematic_path.display(),
                report.manifest_path.display()
            );
            None
        }
        Ok(None) => None,
        Err(error) => {
            eprintln!("V1.1 fixed-Dataset tracer bullet failed: {error}");
            Some(error)
        }
    };
    install_diagnostics();
    let arguments = std::env::args().collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument == "--self-test") {
        let cycles = arguments
            .windows(2)
            .find(|pair| pair[0] == "--cycles")
            .and_then(|pair| pair[1].parse::<usize>().ok())
            .unwrap_or(1);
        let report_path = arguments
            .windows(2)
            .find(|pair| pair[0] == "--self-test-report")
            .map(|pair| PathBuf::from(&pair[1]));
        match run_self_test(cycles) {
            Ok(report) => {
                if let Some(path) = report_path {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap_or_default())
                        .is_err()
                    {
                        std::process::exit(2);
                    }
                }
                std::process::exit(0);
            }
            Err(error) => {
                if let Some(path) = report_path {
                    let report = serde_json::json!({"status": "fail", "error": error});
                    let _ = std::fs::write(
                        path,
                        serde_json::to_vec_pretty(&report).unwrap_or_default(),
                    );
                }
                std::process::exit(1);
            }
        }
    }
    let ui = AppWindow::new()?;
    ui.set_step_labels(ModelRc::new(VecModel::from(
        FoundationStep::ALL
            .iter()
            .map(|step| SharedString::from(step.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_arnis_styles(ModelRc::new(VecModel::from(
        ArnisStylePreset::ALL
            .iter()
            .map(|preset| SharedString::from(preset.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_foundation_styles(ModelRc::new(VecModel::from(
        FoundationStylePreset::ALL
            .iter()
            .map(|preset| SharedString::from(preset.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_candidate_filters(ModelRc::new(VecModel::from(
        CandidateConfidenceFilter::ALL
            .iter()
            .map(|filter| SharedString::from(filter.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_semantic_feature_kinds(ModelRc::new(VecModel::from(
        SemanticFeatureKind::ALL
            .iter()
            .map(|value| SharedString::from(value.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_semantic_feature_sides(ModelRc::new(VecModel::from(
        SemanticFeatureSide::ALL
            .iter()
            .map(|value| SharedString::from(value.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_semantic_height_bands(ModelRc::new(VecModel::from(
        SemanticHeightBand::ALL
            .iter()
            .map(|value| SharedString::from(value.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_semantic_strengths(ModelRc::new(VecModel::from(
        SemanticStrength::ALL
            .iter()
            .map(|value| SharedString::from(value.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_external_model_decisions(ModelRc::new(VecModel::from(
        ExternalModelDecision::ALL
            .iter()
            .map(|value| SharedString::from(value.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_source_conflict_decisions(ModelRc::new(VecModel::from(
        SourceConflictDecision::ALL
            .iter()
            .map(|value| SharedString::from(value.label()))
            .collect::<Vec<_>>(),
    )));

    let mut initial_state = DesktopApplicationState::default();
    initial_state.locale = load_locale();
    let state = Rc::new(RefCell::new(initial_state));
    let loaded_credentials = load_local_credentials();
    for secret in [
        loaded_credentials.js_api_key.as_str(),
        loaded_credentials.security_code.as_str(),
        loaded_credentials.acquisition_secret.as_str(),
    ] {
        v11_guidance::register_secret(secret);
    }
    let map_credentials = Rc::new(RefCell::new(loaded_credentials));
    ui.set_gaode_key(map_credentials.borrow().js_api_key.clone().into());
    ui.set_gaode_security(map_credentials.borrow().security_code.clone().into());
    ui.set_acquisition_secret(map_credentials.borrow().acquisition_secret.clone().into());
    let preferences = v11_guidance::AppPreferences::load(&preferences_path()).unwrap_or_default();
    ui.set_guidance_visible(preferences.should_show_guidance());
    ui.set_guidance_step(0);
    ui.set_quick_start_visible(false);
    if let Some(launcher) = &v11_launcher {
        let launcher = launcher.borrow();
        sync_shortcut_rows(&ui, None, Some(&launcher));
    } else {
        let state = state.borrow();
        sync_shortcut_rows(&ui, Some(&state), None);
    }
    let (tool_update_tx, tool_update_rx) = mpsc::channel();
    let tools = ToolSupervisor {
        processes: DesktopToolProcessSupervisor::new(),
        updates: tool_update_tx,
    };
    let tool_update_rx = Rc::new(RefCell::new(tool_update_rx));
    let startup_open_error = if v11_launcher.is_none() && default_project_path().exists() {
        state.borrow_mut().open(default_project_path()).err()
    } else {
        None
    };
    if v11_launcher.is_none() && state.borrow().project.is_none() {
        state
            .borrow_mut()
            .new_project("未命名项目", "华东师范大学普陀校区");
    }
    sync_ui(&ui, &state.borrow());
    if let Some(launcher) = &v11_launcher {
        if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
            set_error_for(&ui, "project-library.startup", error);
        }
    }
    if let Some(error) = v11_launcher_error {
        set_error_for(&ui, "project-library.startup", error);
    }
    if let Some(error) = v11_tracer_error {
        set_error(&ui, error);
    }
    if let Some(error) = startup_open_error {
        let error = v11_guidance::sanitise_registered_diagnostic_value("message", &error);
        diagnostics::record(
            diagnostics::DiagnosticLevel::Warning,
            "project.autoload",
            &error,
            &[("path", default_project_path().to_string_lossy().as_ref())],
        );
        set_error_for(&ui, "project.autoload", error);
    }

    let tool_timer = Timer::default();
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_confirm_launcher_campus(move || {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            match launcher.borrow_mut().confirm_selected_campus() {
                Ok(()) => {
                    if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                        set_error_for(&ui, "project-library.confirm-campus", error);
                    }
                }
                Err(error) => set_error_for(&ui, "project-library.confirm-campus", error),
            }
        });
    }
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_show_campus_selection(move || {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            launcher.borrow_mut().begin_campus_selection();
            if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                set_error_for(&ui, "project-library.switch-campus", error);
            }
        });
    }
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_search_launcher_campus(move || {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            launcher.borrow_mut().begin_campus_selection();
            if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                set_error_for(&ui, "project-library.search-campus", error);
                return;
            }
            ui.invoke_open_map();
        });
    }
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_show_project_library(move || {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            launcher.borrow_mut().show_project_library();
            if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                set_error_for(&ui, "project-library.show", error);
            }
        });
    }
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_create_library_project(move |name| {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            match launcher.borrow_mut().create_project(name.as_str()) {
                Ok(_) => {
                    ui.set_launcher_new_project_name("".into());
                    if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                        set_error_for(&ui, "project-library.create", error);
                    }
                }
                Err(error) => set_error_for(&ui, "project-library.create", error),
            }
        });
    }
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_open_library_project(move |value| {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            let result = {
                let borrowed = launcher.borrow();
                launcher_project_id(&borrowed, value.as_str())
            }
            .and_then(|project_id| launcher.borrow_mut().open_project(&project_id));
            match result {
                Ok(_) => {
                    if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                        set_error_for(&ui, "project-library.open", error);
                    }
                }
                Err(error) => set_error_for(&ui, "project-library.open", error),
            }
        });
    }
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_export_library_project(move |value| {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            let Some(destination) = project_file_dialog(true) else {
                return;
            };
            let project_id = {
                let borrowed = launcher.borrow();
                launcher_project_id(&borrowed, value.as_str())
            };
            let replace = destination.exists();
            if replace
                && rfd::MessageDialog::new()
                    .set_title("Replace Portable Project?")
                    .set_description("The selected Portable Project already exists. Replace it?")
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    != rfd::MessageDialogResult::Yes
            {
                return;
            }
            let result = project_id.and_then(|project_id| {
                launcher
                    .borrow_mut()
                    .export_project(&project_id, destination, replace)
            });
            if let Err(error) = result {
                set_error_for(&ui, "project-library.export", error);
            }
        });
    }
    {
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_import_portable_project(move || {
            let (Some(launcher), Some(ui)) = (&launcher, weak.upgrade()) else {
                return;
            };
            let Some(source) = project_file_dialog(false) else {
                return;
            };
            let portable_scope = match CampusProjectLibrary::portable_project_scope(&source) {
                Ok(scope) => scope,
                Err(error) => {
                    set_error_for(&ui, "project-library.import", error);
                    return;
                }
            };
            let crosses_campus = launcher
                .borrow()
                .confirmed_campus()
                .is_some_and(|current| current.target_id() != portable_scope.target_id());
            if crosses_campus
                && rfd::MessageDialog::new()
                    .set_title("Switch campus for import?")
                    .set_description("This Portable Project belongs to another campus. Save the current project and switch campuses?")
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    != rfd::MessageDialogResult::Yes
            {
                return;
            }
            let approval = !crosses_campus || launcher.borrow().confirmed_campus().is_some();
            match launcher.borrow_mut().import_portable_project(source, approval) {
                Ok(_) => {
                    if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                        set_error_for(&ui, "project-library.import", error);
                    }
                }
                Err(error) => set_error_for(&ui, "project-library.import", error),
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_dismiss_error(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_error_visible(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_open_diagnostics(move || {
            let Some(directory) = diagnostics::log_directory() else {
                if let Some(ui) = weak.upgrade() {
                    set_error_for(&ui, "diagnostics.open", "诊断日志目录不可用");
                }
                return;
            };
            #[cfg(target_os = "windows")]
            let result = Command::new("explorer").arg(&directory).spawn();
            #[cfg(not(target_os = "windows"))]
            let result = Command::new("xdg-open").arg(&directory).spawn();
            if let Err(error) = result {
                if let Some(ui) = weak.upgrade() {
                    set_error_for(
                        &ui,
                        "diagnostics.open",
                        format!("无法打开诊断日志目录：{error}"),
                    );
                }
            }
        });
    }
    {
        let state = state.clone();
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_set_locale(move |english| {
            let locale = if english {
                DesktopLocale::En
            } else {
                DesktopLocale::ZhCn
            };
            state.borrow_mut().locale = locale;
            if let Err(error) = persist_locale(locale) {
                if let Some(ui) = weak.upgrade() {
                    set_error(&ui, error);
                }
                return;
            }
            if let Some(ui) = weak.upgrade() {
                if let Some(launcher) = &launcher {
                    ui.set_english(english);
                    sync_locale_models(&ui, locale);
                    if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                        set_error(&ui, error);
                    }
                } else {
                    sync_ui(&ui, &state.borrow());
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_candidate_filter(move |index| {
            let filter = CandidateConfidenceFilter::ALL
                .get(index.max(0) as usize)
                .copied()
                .unwrap_or_default();
            let mut state = state.borrow_mut();
            state.candidate_filter = filter;
            state.candidate_page = 0;
            if let Some(ui) = weak.upgrade() {
                sync_ui(&ui, &state);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_import_foundation_style(move || {
            let Some(path) = foundation_style_file_dialog() else {
                return;
            };
            let result = std::fs::read(&path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| FoundationStylePack::parse_json(&bytes));
            match result {
                Ok(pack) => {
                    let name = pack.name.clone();
                    state
                        .borrow_mut()
                        .mutate_project(|project| project.apply_foundation_style_pack(pack));
                    if let Some(ui) = weak.upgrade() {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            set_status(
                                &ui,
                                format!("进阶 Foundation 样式包已导入：{name}"),
                                format!("Advanced Foundation style pack imported: {name}"),
                            );
                        }
                    }
                }
                Err(error) => {
                    if let Some(ui) = weak.upgrade() {
                        set_error(&ui, error);
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_change_candidate_page(move |delta| {
            let mut state = state.borrow_mut();
            state.candidate_page = if delta < 0 {
                state.candidate_page.saturating_sub(1)
            } else {
                state.candidate_page.saturating_add(1)
            };
            if let Some(ui) = weak.upgrade() {
                sync_ui(&ui, &state);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_open_candidate_details(move |id| {
            state.borrow_mut().selected_candidate_id = Some(id.to_string());
            if let Some(ui) = weak.upgrade() {
                sync_ui(&ui, &state.borrow());
                ui.set_candidate_details_visible(true);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_rename_selected_building(move |name| {
            let source_id = state.borrow().selected_candidate_id.clone();
            let result = source_id
                .ok_or_else(|| "请先选择建筑候选".to_string())
                .and_then(|source_id| {
                    let mut result = Ok(());
                    state.borrow_mut().mutate_project(|project| {
                        result = project.rename_building(&source_id, name.as_str());
                    });
                    result
                });
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            set_status(&ui, "建筑目录名称已保存", "Building directory name saved");
                        }
                    }
                    Err(error) => {
                        set_localized_error(
                            &ui,
                            "building.rename",
                            format!("建筑名称保存失败：{error}"),
                            format!("Failed to save building name: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_suppress_selected_building(move |reason| {
            let source_id = state.borrow().selected_candidate_id.clone();
            let result = source_id
                .ok_or_else(|| "请先选择建筑候选".to_string())
                .and_then(|source_id| {
                    let mut result = Ok(());
                    state.borrow_mut().mutate_project(|project| {
                        result = project.suppress_building(&source_id, reason.as_str());
                    });
                    result
                });
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        state.borrow_mut().selected_candidate_id = None;
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            ui.set_candidate_suppression_reason("".into());
                            ui.set_candidate_details_visible(false);
                            set_status(
                                &ui,
                                "建筑来源已持久抑制，可在详情面板恢复",
                                "Building source suppressed; it remains recoverable in Details",
                            );
                        }
                    }
                    Err(error) => {
                        set_localized_error(
                            &ui,
                            "building.suppress",
                            format!("建筑抑制失败：{error}"),
                            format!("Failed to suppress building: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_restore_building_suppression(move |index| {
            let source_id = state.borrow().project.as_ref().and_then(|project| {
                project
                    .building_suppressions
                    .get(index.max(0) as usize)
                    .map(|record| record.source_id.clone())
            });
            let restored = source_id.is_some_and(|source_id| {
                let mut restored = false;
                state.borrow_mut().mutate_project(|project| {
                    restored = project.restore_building_suppression(&source_id);
                });
                restored
            });
            if let Some(ui) = weak.upgrade() {
                if restored {
                    if let Err(error) = save_and_sync(&ui, &state) {
                        set_error(&ui, error);
                    } else {
                        set_status(
                            &ui,
                            "建筑抑制已恢复，请重新查询当前视野",
                            "Building suppression restored; query the current view again",
                        );
                    }
                } else {
                    set_status(
                        &ui,
                        "没有可恢复的建筑抑制",
                        "No recoverable building suppressions",
                    );
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_external_model(move |index| {
            let mut state = state.borrow_mut();
            state.selected_external_model = index.max(0) as usize;
            if let Some(ui) = weak.upgrade() {
                sync_ui(&ui, &state);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_review_external_model(move |model_index, decision_index, reason| {
            let decision = ExternalModelDecision::ALL
                .get(decision_index.max(0) as usize)
                .copied()
                .unwrap_or_default();
            let model_id = {
                let state = state.borrow();
                let project = state.project.as_ref();
                project.and_then(|project| {
                    let slot_id =
                        project.detailed.selected_slot_id.as_deref().or_else(|| {
                            project.building_slots.first().map(|slot| slot.id.as_str())
                        })?;
                    project
                        .detailed
                        .external_models
                        .iter()
                        .filter(|review| review.slot_id == slot_id)
                        .nth(model_index.max(0) as usize)
                        .map(|review| review.id.clone())
                })
            };
            let result = model_id
                .ok_or_else(|| "当前建筑没有可审核的外部模型".to_string())
                .and_then(|model_id| {
                    let mut result = Ok(());
                    state.borrow_mut().mutate_project(|project| {
                        result =
                            project.review_external_model(&model_id, decision, reason.as_str());
                    });
                    result
                });
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            ui.set_external_model_reason("".into());
                            set_status(&ui, "外部模型审核已保存", "External model review saved");
                        }
                    }
                    Err(error) => {
                        set_localized_error(
                            &ui,
                            "external-model.review",
                            format!("外部模型审核失败：{error}"),
                            format!("External model review failed: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_source_conflict(move |index| {
            let mut state = state.borrow_mut();
            state.selected_source_conflict = index.max(0) as usize;
            if let Some(ui) = weak.upgrade() {
                sync_ui(&ui, &state);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_review_source_conflict(move |conflict_index, decision_index, reason| {
            let decision = SourceConflictDecision::ALL
                .get(decision_index.max(0) as usize)
                .copied()
                .unwrap_or_default();
            let conflict_id = {
                let state = state.borrow();
                let project = state.project.as_ref();
                project.and_then(|project| {
                    let slot_id =
                        project.detailed.selected_slot_id.as_deref().or_else(|| {
                            project.building_slots.first().map(|slot| slot.id.as_str())
                        })?;
                    project
                        .detailed
                        .source_conflicts
                        .iter()
                        .filter(|conflict| conflict.slot_id == slot_id)
                        .nth(conflict_index.max(0) as usize)
                        .map(|conflict| conflict.id.clone())
                })
            };
            let result = conflict_id
                .ok_or_else(|| "当前建筑没有可审核的来源冲突".to_string())
                .and_then(|conflict_id| {
                    let mut result = Ok(());
                    state.borrow_mut().mutate_project(|project| {
                        result =
                            project.review_source_conflict(&conflict_id, decision, reason.as_str());
                    });
                    result
                });
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            ui.set_source_conflict_reason("".into());
                            set_status(&ui, "来源冲突决策已保存", "Source conflict decision saved");
                        }
                    }
                    Err(error) => {
                        set_localized_error(
                            &ui,
                            "source-conflict.review",
                            format!("来源冲突审核失败：{error}"),
                            format!("Source conflict review failed: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_dismiss_guidance(move |skipped| {
            let result =
                v11_guidance::AppPreferences::default().dismiss_guidance(&preferences_path());
            if let Some(ui) = weak.upgrade() {
                ui.set_guidance_visible(false);
                match result {
                    Ok(()) => {
                        if skipped {
                            set_status(
                                &ui,
                                "已跳过引导；可按 F1 或 ? 重新打开",
                                "Guidance skipped; press F1 or ? to reopen",
                            );
                        } else {
                            set_status(&ui, "首次运行引导已完成", "First-run guidance completed");
                        }
                    }
                    Err(error) => set_localized_error(
                        &ui,
                        "guidance.preference",
                        format!("引导偏好保存失败：{error}"),
                        format!("Failed to save guidance preference: {error}"),
                    ),
                }
            }
        });
    }
    {
        let state = state.clone();
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_open_settings(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.set_utilities_visible(false);
            ui.set_guidance_visible(false);
            ui.set_quick_start_visible(false);
            ui.set_settings_tab(0);
            ui.set_settings_visible(true);
            if let Some(launcher) = &launcher {
                let launcher = launcher.borrow();
                sync_shortcut_rows(&ui, None, Some(&launcher));
            } else {
                let state = state.borrow();
                sync_shortcut_rows(&ui, Some(&state), None);
            }
        });
    }
    {
        let state = state.clone();
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_shortcut_requested(move |index, text_input_focused, modal, map_tool| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(shortcut) = v11_guidance::Shortcut::ALL.get(index as usize).copied() else {
                return;
            };
            let outcome = {
                let state = state.borrow();
                if let Some(launcher) = &launcher {
                    let launcher = launcher.borrow();
                    v11_guidance::resolve_shortcut(
                        shortcut,
                        shortcut_context(
                            None,
                            Some(&launcher),
                            modal_state_from_code(modal),
                            text_input_focused,
                            map_tool_state_from_code(map_tool),
                        ),
                        guidance_locale(&ui),
                    )
                } else {
                    v11_guidance::resolve_shortcut(
                        shortcut,
                        shortcut_context(
                            Some(&state),
                            None,
                            modal_state_from_code(modal),
                            text_input_focused,
                            map_tool_state_from_code(map_tool),
                        ),
                        guidance_locale(&ui),
                    )
                }
            };
            ui.set_shortcut_feedback(outcome.reason().into());
            let Some(action) = outcome.action() else {
                set_status(&ui, outcome.reason(), outcome.reason());
                if let Some(launcher) = &launcher {
                    let launcher = launcher.borrow();
                    sync_shortcut_rows(&ui, None, Some(&launcher));
                } else {
                    let state = state.borrow();
                    sync_shortcut_rows(&ui, Some(&state), None);
                }
                return;
            };
            match action {
                v11_guidance::ShortcutAction::OpenGuidance => {
                    ui.set_settings_visible(false);
                    ui.set_utilities_visible(false);
                    ui.set_quick_start_visible(false);
                    ui.set_guidance_step(0);
                    ui.set_guidance_visible(true);
                }
                v11_guidance::ShortcutAction::CloseGuidance => ui.set_guidance_visible(false),
                v11_guidance::ShortcutAction::CloseSettings => ui.set_settings_visible(false),
                v11_guidance::ShortcutAction::CloseQuickStart => ui.set_quick_start_visible(false),
                v11_guidance::ShortcutAction::CloseUtilities => ui.set_utilities_visible(false),
                v11_guidance::ShortcutAction::DismissError => ui.invoke_dismiss_error(),
                v11_guidance::ShortcutAction::NewProject => {
                    ui.invoke_show_project_library();
                    set_status(
                        &ui,
                        "请在校区项目库中输入唯一项目名称",
                        "Enter a unique project name in the Campus Project Library",
                    );
                }
                v11_guidance::ShortcutAction::OpenProject => {
                    if launcher.is_some() {
                        ui.invoke_import_portable_project();
                    } else {
                        ui.invoke_open_project();
                    }
                }
                v11_guidance::ShortcutAction::SaveProject => ui.invoke_save_project(),
                v11_guidance::ShortcutAction::ExportPortableProject => {
                    if let Some(launcher) = &launcher {
                        let project_id = launcher
                            .borrow()
                            .active_project_id()
                            .map(|project_id| project_id.as_str().to_string());
                        if let Some(project_id) = project_id {
                            ui.invoke_export_library_project(project_id.into());
                        }
                    } else {
                        ui.invoke_export_portable_project();
                    }
                }
                v11_guidance::ShortcutAction::UndoProjectHistory => ui.invoke_undo(),
                v11_guidance::ShortcutAction::RedoProjectHistory => ui.invoke_redo(),
                v11_guidance::ShortcutAction::ConfirmWorkflowTask => ui.invoke_confirm_step(),
                v11_guidance::ShortcutAction::CancelWorkflowTask
                | v11_guidance::ShortcutAction::CancelMapTool
                | v11_guidance::ShortcutAction::DeleteBoundaryVertex => {
                    unreachable!("map and cancellable task shortcuts are handled by their surface")
                }
            }
            if let Some(launcher) = &launcher {
                let launcher = launcher.borrow();
                sync_shortcut_rows(&ui, None, Some(&launcher));
            } else {
                let state = state.borrow();
                sync_shortcut_rows(&ui, Some(&state), None);
            }
        });
    }

    {
        let map_credentials = map_credentials.clone();
        let weak = ui.as_weak();
        ui.on_save_credentials(move |key, security, acquisition_secret| {
            let updated = LocalCredentials {
                js_api_key: key.trim().to_string(),
                security_code: security.trim().to_string(),
                acquisition_secret: acquisition_secret.trim().to_string(),
            };
            if let Some(ui) = weak.upgrade() {
                let validation = if updated.js_api_key.is_empty() {
                    Err(tr(
                        if ui.get_english() {
                            DesktopLocale::En
                        } else {
                            DesktopLocale::ZhCn
                        },
                        "请填写高德 Web JS API Key",
                        "Enter a Gaode Web JS API Key",
                    )
                    .to_string())
                } else if updated.security_code.is_empty() {
                    Err(tr(
                        if ui.get_english() {
                            DesktopLocale::En
                        } else {
                            DesktopLocale::ZhCn
                        },
                        "请填写与该 Key 配对的 securityJsCode",
                        "Enter the securityJsCode paired with this key",
                    )
                    .to_string())
                } else {
                    save_local_credentials(&updated)
                };
                match validation {
                    Ok(()) => {
                        for secret in [
                            updated.js_api_key.as_str(),
                            updated.security_code.as_str(),
                            updated.acquisition_secret.as_str(),
                        ] {
                            v11_guidance::register_secret(secret);
                        }
                        *map_credentials.borrow_mut() = updated;
                        set_status(
                            &ui,
                            "本机凭据已安全保存",
                            "Local credentials saved securely",
                        );
                    }
                    Err(error) => {
                        set_localized_error(
                            &ui,
                            "credentials.save",
                            format!("本机凭据保存失败：{error}"),
                            format!("Failed to save local credentials: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let receiver = tool_update_rx.clone();
        let launcher = v11_launcher.clone();
        tool_timer.start(
            TimerMode::Repeated,
            std::time::Duration::from_millis(120),
            move || {
                let mut changed = false;
                while let Ok(update) = receiver.borrow_mut().try_recv() {
                    if launcher.is_some()
                        && !matches!(
                            &update,
                            ToolUpdate::Status(_)
                                | ToolUpdate::Error { .. }
                                | ToolUpdate::MapCampusTarget(_)
                        )
                    {
                        continue;
                    }
                    match update {
                        ToolUpdate::Status(message) => {
                            if launcher.is_none() {
                                state.borrow_mut().tool_status = Some(message.clone());
                            }
                            if let Some(ui) = weak.upgrade() {
                                ui.set_tool_status(message.into());
                            }
                        }
                        ToolUpdate::Error { event, message } => {
                            if launcher.is_none() {
                                state.borrow_mut().tool_status = Some(message.clone());
                            }
                            if let Some(ui) = weak.upgrade() {
                                set_error_for(&ui, &event, message);
                            }
                        }
                        ToolUpdate::PreviewBlockSelected { x, y, z, block } => {
                            state.borrow_mut().selected_preview_block =
                                Some(campus_state::PreviewBlockSelection {
                                    x,
                                    y,
                                    z,
                                    block: block.clone(),
                                });
                            if let Some(ui) = weak.upgrade() {
                                ui.set_selected_block_summary(
                                    format!("{block} · ({x}, {y}, {z})").into(),
                                );
                            }
                        }
                        ToolUpdate::MapCamera {
                            center,
                            zoom,
                            pitch,
                            rotation,
                        } => {
                            state.borrow_mut().mutate_project(|project| {
                                project.map_view.center = center;
                                project.map_view.zoom = zoom;
                                project.map_view.pitch = pitch;
                                project.map_view.rotation = rotation;
                            });
                            changed = true;
                        }
                        ToolUpdate::MapPoint(point) => {
                            state.borrow_mut().mutate_project(|project| {
                                project.map_view.center = point;
                            });
                            changed = true;
                        }
                        ToolUpdate::MapCampusTarget(target) => {
                            let name = target.name.clone();
                            if let Some(launcher) = &launcher {
                                let result = CampusScope::new(
                                    format!("gaode:{}", target.poi_id),
                                    target.name,
                                    [target.wgs84.lng, target.wgs84.lat],
                                )
                                .and_then(|scope| scope.with_gaode_poi_id(target.poi_id))
                                .map(|scope| {
                                    launcher.borrow_mut().select_campus_candidate(scope);
                                });
                                if let Some(ui) = weak.upgrade() {
                                    match result {
                                        Ok(()) => {
                                            set_status(
                                                &ui,
                                                format!("Campus candidate selected: {name}"),
                                                format!("Campus candidate selected: {name}"),
                                            );
                                            if let Err(error) =
                                                sync_project_launcher_ui(&ui, &launcher.borrow())
                                            {
                                                set_error_for(
                                                    &ui,
                                                    "project-library.search-campus",
                                                    error,
                                                );
                                            }
                                        }
                                        Err(error) => set_error(&ui, error),
                                    }
                                }
                                changed = false;
                            } else {
                                let result = state.borrow_mut().apply_foundation_intent(
                                    FoundationWorkflowIntent::SelectCampusTarget(target),
                                );
                                if let Some(ui) = weak.upgrade() {
                                    match &result {
                                        Ok(()) => set_status(
                                            &ui,
                                            format!("Gaode campus target confirmed: {name}"),
                                            format!("Gaode campus target confirmed: {name}"),
                                        ),
                                        Err(error) => set_error(&ui, error),
                                    }
                                }
                                changed = result.is_ok();
                            }
                        }
                        ToolUpdate::MapBoundary(points) => {
                            let result = state.borrow_mut().apply_foundation_intent(
                                FoundationWorkflowIntent::ConfirmCampusBoundary(points),
                            );
                            if let (Err(error), Some(ui)) = (&result, weak.upgrade()) {
                                set_error(&ui, error);
                            }
                            changed = result.is_ok();
                        }
                    }
                }
                if changed {
                    if let Some(ui) = weak.upgrade() {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        }
                    }
                }
            },
        );
    }

    {
        let state = state.clone();
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_undo(move || {
            if let Some(launcher) = &launcher {
                let result = launcher.borrow_mut().undo();
                if let Some(ui) = weak.upgrade() {
                    match result {
                        Ok(()) => {
                            if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                                set_error(&ui, error);
                            }
                        }
                        Err(error) => set_error(&ui, error),
                    }
                }
            } else if state.borrow_mut().undo() {
                if let Some(ui) = weak.upgrade() {
                    if let Err(error) = save_and_sync(&ui, &state) {
                        set_error(&ui, error);
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_reset_candidate_review(move |id| {
            state.borrow_mut().mutate_project(|project| {
                project.reset_candidate_review(id.as_str());
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_review_visible_candidates(move |accepted| {
            let filter = state.borrow().candidate_filter;
            state.borrow_mut().mutate_project(|project| {
                let kind = match project.foundation_step {
                    FoundationStep::Building => Some(FeatureKind::Building),
                    FoundationStep::Road => Some(FeatureKind::Road),
                    FoundationStep::Water => Some(FeatureKind::Water),
                    FoundationStep::Vegetation => Some(FeatureKind::Vegetation),
                    FoundationStep::Sports => Some(FeatureKind::Sports),
                    _ => None,
                };
                let ids = project
                    .candidates
                    .iter()
                    .filter(|candidate| {
                        candidate.review == ReviewDecision::Pending
                            && kind.is_none_or(|kind| candidate.kind == kind)
                            && candidate_matches_filter(candidate, filter)
                    })
                    .map(|candidate| candidate.id.clone())
                    .collect::<Vec<_>>();
                for id in ids {
                    if accepted {
                        project.accept_candidate(&id);
                    } else {
                        project.reject_candidate(&id);
                    }
                }
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_redo(move || {
            if let Some(launcher) = &launcher {
                let result = launcher.borrow_mut().redo();
                if let Some(ui) = weak.upgrade() {
                    match result {
                        Ok(()) => {
                            if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                                set_error(&ui, error);
                            }
                        }
                        Err(error) => set_error(&ui, error),
                    }
                }
            } else if state.borrow_mut().redo() {
                if let Some(ui) = weak.upgrade() {
                    if let Err(error) = save_and_sync(&ui, &state) {
                        set_error(&ui, error);
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_foundation_style(move |index| {
            let preset = FoundationStylePreset::ALL
                .get(index.max(0) as usize)
                .copied()
                .unwrap_or_default();
            state
                .borrow_mut()
                .mutate_project(|project| project.apply_foundation_style(preset));
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                } else {
                    set_status(
                        &ui,
                        format!("Foundation 样式已切换：{}", preset.label()),
                        format!(
                            "Foundation style switched: {}",
                            FoundationStylePack::from_preset(preset).name
                        ),
                    );
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_preview_foundation(move || match generate_foundation_preview(&state) {
            Ok((path, title)) => {
                if let Some(ui) = weak.upgrade() {
                    if let Err(error) = autosave(&state) {
                        set_error(&ui, error);
                        return;
                    }
                    sync_ui(&ui, &state.borrow());
                }
                state.borrow_mut().active_preview_path = Some(path.clone());
                if let Err(error) = tools.launch_preview_supervised(
                    path,
                    title,
                    state.borrow().locale == DesktopLocale::En,
                ) {
                    if let Some(ui) = weak.upgrade() {
                        set_localized_error(
                            &ui,
                            "foundation-preview.launch",
                            format!("Foundation 已生成，预览启动失败：{error}"),
                            format!("Foundation generated, but preview failed to start: {error}"),
                        );
                    }
                }
            }
            Err(error) => {
                if let Some(ui) = weak.upgrade() {
                    set_localized_error(
                        &ui,
                        "foundation-preview.generate",
                        format!("Foundation 预览失败：{error}"),
                        format!("Foundation preview failed: {error}"),
                    );
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_set_foundation_metrics(move |orientation, scale| {
            let result = state.borrow_mut().apply_foundation_intent(
                FoundationWorkflowIntent::SetCampusMetrics {
                    orientation_degrees: orientation as f64,
                    blocks_per_meter: scale as f64,
                },
            );
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            set_status(&ui, "朝向与比例已应用", "Orientation and scale applied");
                        }
                    }
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_create_project(move |name, campus| {
            state
                .borrow_mut()
                .new_project(name.as_str(), campus.as_str());
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_open_project(move || {
            let Some(path) = project_file_dialog(false) else {
                return;
            };
            let result = state.borrow_mut().open(path);
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => sync_ui(&ui, &state.borrow()),
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let launcher = v11_launcher.clone();
        let weak = ui.as_weak();
        ui.on_save_project(move || {
            if let Some(launcher) = &launcher {
                let result = launcher.borrow_mut().request_save();
                if let Some(ui) = weak.upgrade() {
                    match result {
                        Ok(()) => {
                            if let Err(error) = sync_project_launcher_ui(&ui, &launcher.borrow()) {
                                set_error(&ui, error);
                            }
                        }
                        Err(error) => set_error(&ui, error),
                    }
                }
            } else {
                let result = autosave(&state);
                if let Some(ui) = weak.upgrade() {
                    match result {
                        Ok(()) => sync_ui(&ui, &state.borrow()),
                        Err(error) => set_error(&ui, error),
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_export_portable_project(move || {
            let Some(path) = project_file_dialog(true) else {
                return;
            };
            let result = state.borrow_mut().save_as_portable(path);
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => sync_ui(&ui, &state.borrow()),
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_switch_mode(move |detailed| {
            let intent = if detailed {
                ReconstructionWorkflowIntent::EnterDetailedBuilding
            } else {
                ReconstructionWorkflowIntent::EnterFoundation
            };
            let result = state.borrow_mut().apply_reconstruction_intent(intent);
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if detailed {
                            state.borrow_mut().mutate_project(|project| {
                                let ids = project
                                    .building_slots
                                    .iter()
                                    .map(|slot| slot.id.clone())
                                    .collect::<Vec<_>>();
                                for id in ids {
                                    project.refresh_detailed_plan_for_slot(&id);
                                    project.discover_external_models_for_slot(&id);
                                }
                            });
                        }
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        }
                        ui.window().request_redraw();
                    }
                    Err(error) => set_localized_error(
                        &ui,
                        "workflow.enter-detailed",
                        format!("暂时不能进入单栋精修：{error}。请先在地基审核中确认至少一栋建筑。"),
                        "Detailed Building is not available yet. Confirm at least one building in Foundation review first.",
                    ),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_step(move |index| {
            let Some(step) = FoundationStep::ALL.get(index.max(0) as usize).copied() else {
                return;
            };
            let phase = match step {
                FoundationStep::Campus | FoundationStep::Boundary | FoundationStep::Orientation => {
                    FoundationPhase::Scope
                }
                FoundationStep::Building
                | FoundationStep::Road
                | FoundationStep::Water
                | FoundationStep::Vegetation
                | FoundationStep::Sports => FoundationPhase::Review,
                FoundationStep::Export => FoundationPhase::Generate,
            };
            let result = {
                let mut borrowed = state.borrow_mut();
                borrowed.candidate_page = 0;
                borrowed.apply_foundation_intent(FoundationWorkflowIntent::EnterPhase(phase))
            };
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        }
                    }
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_confirm_step(move || {
            let result = state
                .borrow_mut()
                .apply_foundation_intent(FoundationWorkflowIntent::CompleteCurrentStep);
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        }
                    }
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_review_candidate(move |id, accepted| {
            state.borrow_mut().mutate_project(|project| {
                if accepted {
                    project.accept_candidate(id.as_str());
                } else {
                    project.reject_candidate(id.as_str());
                }
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_update_building_measurements(move |height, floors, roof| {
            let parsed_height = height
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| *value > 0.0);
            let parsed_floors = floors.trim().parse::<u32>().ok().filter(|value| *value > 0);
            let parsed_roof = match roof.trim().to_ascii_lowercase().as_str() {
                "" => None,
                "flat" | "gabled" | "hipped" | "skillion" | "pyramidal" | "dome" | "cone"
                | "onion" => Some(roof.trim().to_ascii_lowercase()),
                _ => {
                    if let Some(ui) = weak.upgrade() {
                        set_status(&ui, "屋顶形状无效", "Invalid roof shape");
                    }
                    return;
                }
            };
            state.borrow_mut().mutate_project(|project| {
                let selected_id = project.detailed.selected_slot_id.clone();
                if let Some(slot) = project.building_slots.iter_mut().find(|slot| {
                    selected_id
                        .as_deref()
                        .map(|id| slot.id == id)
                        .unwrap_or(true)
                }) {
                    slot.height_m = parsed_height;
                    slot.floors = parsed_floors;
                    slot.roof_shape = parsed_roof;
                }
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                } else {
                    set_status(
                        &ui,
                        "实测几何已保存，模板不会修改这些数值",
                        "Measurements saved; templates will not alter them",
                    );
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_slot(move |index| {
            let mut borrowed = state.borrow_mut();
            borrowed.selected_external_model = 0;
            borrowed.selected_source_conflict = 0;
            borrowed.mutate_project(|project| {
                let slot_id = project
                    .building_slots
                    .get(index.max(0) as usize)
                    .map(|slot| slot.id.clone());
                project.detailed.selected_slot_id = slot_id.clone();
                if let Some(slot_id) = slot_id {
                    project.refresh_detailed_plan_for_slot(&slot_id);
                    project.discover_external_models_for_slot(&slot_id);
                }
            });
            drop(borrowed);
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_style(move |index, density, depth| {
            let preset = ArnisStylePreset::ALL
                .get(index.max(0) as usize)
                .copied()
                .unwrap_or_default();
            state.borrow_mut().mutate_project(|project| {
                project.detailed.style_preset = preset;
                project.detailed.window_density = density.clamp(0.0, 100.0) as u8;
                project.detailed.wall_depth = depth.clamp(0.0, 100.0) as u8;
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_template_proposal(move |index| {
            let result = {
                let mut borrowed = state.borrow_mut();
                let mut selection = None;
                borrowed.mutate_project(|project| {
                    let slot_id = project
                        .detailed
                        .selected_slot_id
                        .clone()
                        .or_else(|| project.building_slots.first().map(|slot| slot.id.clone()));
                    if let Some(slot_id) = slot_id {
                        project.refresh_detailed_plan_for_slot(&slot_id);
                        let proposal_id = project
                            .template_proposals_for_slot(&slot_id)
                            .get(index.max(0) as usize)
                            .map(|proposal| proposal.template.id.clone());
                        selection = Some(
                            proposal_id
                                .as_deref()
                                .map(|template_id| {
                                    project.select_template_for_slot(&slot_id, template_id)
                                })
                                .unwrap_or_else(|| Err("没有可选的建筑模板提案".into())),
                        );
                    } else {
                        selection = Some(Err("请先选择已审核建筑".into()));
                    }
                });
                selection.unwrap_or_else(|| Err("请先选择已审核建筑".into()))
            };
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            set_status(
                                &ui,
                                "已选择模板并建立可编辑立面规则草案",
                                "Template selected and editable facade rules drafted",
                            );
                        }
                    }
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_import_local_evidence(move || {
            let sources = local_evidence_file_dialog();
            if sources.is_empty() {
                return;
            }
            let result = autosave(&state)
                .and_then(|()| state.borrow_mut().import_local_evidence_files(&sources));
            match result {
                Ok(0) => {}
                Ok(count) => {
                    if let Some(ui) = weak.upgrade() {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            set_status(
                                &ui,
                                format!("已添加 {count} 张本地照片，已作为立面规则的本地证据保存"),
                                format!(
                                    "Added {count} local photo(s) as local facade-rule evidence"
                                ),
                            );
                        }
                    }
                }
                Err(error) => {
                    if let Some(ui) = weak.upgrade() {
                        set_error(&ui, error);
                    }
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let launcher = v11_launcher.clone();
        let map_credentials = map_credentials.clone();
        let weak = ui.as_weak();
        ui.on_open_map(move || {
            let snapshot = if let Some(launcher) = &launcher {
                let borrowed = launcher.borrow();
                borrowed
                    .confirmed_campus()
                    .or_else(|| borrowed.offered_campus())
                    .map(|scope| {
                        let anchor = scope.anchor_wgs84();
                        (
                            scope.canonical_name().to_string(),
                            MapViewState {
                                center: GeoPoint {
                                    lng: anchor[0],
                                    lat: anchor[1],
                                },
                                ..MapViewState::default()
                            },
                            Vec::new(),
                            Some(FoundationMapTask::CampusSelection),
                        )
                    })
            } else {
                state.borrow().project.as_ref().map(|project| {
                    (
                        project.campus_name.clone(),
                        project.map_view.clone(),
                        project.boundary.clone(),
                        FoundationWorkflow::projection(project).map_task,
                    )
                })
            };
            let Some((campus_name, view, boundary, map_task)) = snapshot else {
                if let Some(ui) = weak.upgrade() {
                    set_error(&ui, "请先创建 Campus Reconstruction Project");
                }
                return;
            };
            let purpose = match map_task {
                Some(FoundationMapTask::CampusSelection) => MapPurpose::CampusSelection,
                Some(FoundationMapTask::CampusBoundary) => MapPurpose::CampusBoundary,
                Some(FoundationMapTask::FoundationReview) => MapPurpose::FoundationReview,
                None => {
                    if let Some(ui) = weak.upgrade() {
                        set_status(
                            &ui,
                            "当前 Foundation Workflow 任务不需要地图窗口",
                            "The current Foundation Workflow task does not use the map window",
                        );
                    }
                    return;
                }
            };
            let credentials = map_credentials.borrow().clone();
            if credentials.js_api_key.trim().is_empty() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_settings_visible(true);
                    set_status(
                        &ui,
                        "请先配置高德 Web JS API 密钥",
                        "Configure the Gaode Web JS API key first",
                    );
                }
                return;
            }
            if let Err(error) = tools.launch_map_supervised(MapLaunchRequest {
                title: campus_name,
                view,
                boundary,
                js_api_key: credentials.js_api_key,
                security_code: credentials.security_code,
                purpose,
                overlays: Vec::new(),
                feature_kind: None,
                english: state.borrow().locale == DesktopLocale::En,
            }) {
                if let Some(ui) = weak.upgrade() {
                    set_localized_error(
                        &ui,
                        "map.launch",
                        format!("地图启动失败：{error}"),
                        format!("Map failed to start: {error}"),
                    );
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let map_credentials = map_credentials.clone();
        let weak = ui.as_weak();
        ui.on_open_detailed_map(move || {
            let selected = {
                let state = state.borrow();
                state.project.as_ref().and_then(|project| {
                    let slot = project
                        .detailed
                        .selected_slot_id
                        .as_deref()
                        .and_then(|id| project.building_slots.iter().find(|slot| slot.id == id))
                        .or_else(|| project.building_slots.first())?;
                    if slot.footprint.is_empty() {
                        return None;
                    }
                    let count = slot.footprint.len() as f64;
                    let center = slot.footprint.iter().fold(
                        GeoPoint { lng: 0.0, lat: 0.0 },
                        |sum, point| GeoPoint {
                            lng: sum.lng + point.lng / count,
                            lat: sum.lat + point.lat / count,
                        },
                    );
                    Some((project.campus_name.clone(), slot.clone(), center))
                })
            };
            let Some((campus_name, slot, center)) = selected else {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
                        "请先选择具有已审核轮廓的建筑槽位",
                        "Select a Building Slot with a reviewed footprint first",
                    );
                }
                return;
            };
            let credentials = map_credentials.borrow().clone();
            if credentials.js_api_key.trim().is_empty() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_settings_visible(true);
                    set_status(
                        &ui,
                        "请先配置高德 Web JS API 密钥",
                        "Configure the Gaode Web JS API key first",
                    );
                }
                return;
            }
            let request = MapLaunchRequest {
                title: format!("{campus_name} · {}", slot.name),
                view: MapViewState {
                    center: campus_services::wgs84_to_gcj02(center),
                    zoom: 19.0,
                    pitch: 65.0,
                    rotation: 0.0,
                    capture_bounds: None,
                },
                boundary: Vec::new(),
                js_api_key: credentials.js_api_key,
                security_code: credentials.security_code,
                purpose: MapPurpose::BuildingEvidence,
                overlays: vec![MapOverlay {
                    label: slot.name,
                    points: slot
                        .footprint
                        .into_iter()
                        .map(|point| MapCoordinate {
                            lng: point.lng,
                            lat: point.lat,
                        })
                        .collect(),
                }],
                feature_kind: None,
                english: state.borrow().locale == DesktopLocale::En,
            };
            if let Err(error) = tools.launch_map_supervised(request) {
                if let Some(ui) = weak.upgrade() {
                    set_localized_error(
                        &ui,
                        "map.evidence.launch",
                        format!("建筑证据地图启动失败：{error}"),
                        format!("Building evidence map failed to start: {error}"),
                    );
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_open_preview(move || {
            let model = state
                .borrow()
                .project
                .as_ref()
                .and_then(|project| project.detailed.generated_path.clone());
            let Some(model) = model else {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
                        "请先生成精细建筑",
                        "Generate a detailed building first",
                    );
                }
                return;
            };
            let title = state
                .borrow()
                .project
                .as_ref()
                .and_then(|project| {
                    project
                        .detailed
                        .selected_slot_id
                        .as_deref()
                        .and_then(|id| project.building_slots.iter().find(|slot| slot.id == id))
                        .map(|slot| slot.name.clone())
                })
                .unwrap_or_else(|| "精细建筑".into());
            state.borrow_mut().active_preview_path = Some(model.clone());
            if let Err(error) = tools.launch_preview_supervised(
                model,
                title,
                state.borrow().locale == DesktopLocale::En,
            ) {
                if let Some(ui) = weak.upgrade() {
                    set_localized_error(
                        &ui,
                        "preview.launch",
                        format!("预览启动失败：{error}"),
                        format!("Preview failed to start: {error}"),
                    );
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_confirm_refinement(move || {
            let result = {
                let mut state = state.borrow_mut();
                let mut confirmed = None;
                state.mutate_project(|project| {
                    let slot_id = project
                        .detailed
                        .selected_slot_id
                        .clone()
                        .or_else(|| project.building_slots.first().map(|slot| slot.id.clone()));
                    if let Some(slot_id) = slot_id {
                        confirmed = project.confirm_latest_refinement(&slot_id);
                    }
                });
                confirmed.ok_or("当前建筑没有可确认的生成版本")
            };
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(version) => {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            set_status(
                                &ui,
                                format!("已确认建筑 refinement v{version}"),
                                format!("Building refinement v{version} confirmed"),
                            );
                        }
                    }
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_generate_building(move || match generate_detailed_model(&state) {
            Ok((path, title)) => {
                if let Some(ui) = weak.upgrade() {
                    if let Err(error) = autosave(&state) {
                        set_error(&ui, error);
                        return;
                    }
                    sync_ui(&ui, &state.borrow());
                }
                state.borrow_mut().active_preview_path = Some(path.clone());
                if let Err(error) = tools.launch_preview_supervised(
                    path,
                    title,
                    state.borrow().locale == DesktopLocale::En,
                ) {
                    if let Some(ui) = weak.upgrade() {
                        set_localized_error(
                            &ui,
                            "generation.preview.launch",
                            format!("生成成功，预览启动失败：{error}"),
                            format!("Generation succeeded, but preview failed to start: {error}"),
                        );
                    }
                }
            }
            Err(error) => {
                if let Some(ui) = weak.upgrade() {
                    set_localized_error(
                        &ui,
                        "generation.run",
                        format!("生成失败：{error}"),
                        format!("Generation failed: {error}"),
                    );
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_apply_semantic_feature(move |kind, side, height, strength, label, reason| {
            let kind = SemanticFeatureKind::ALL
                .get(kind.max(0) as usize)
                .copied()
                .unwrap_or(SemanticFeatureKind::EntranceEmphasis);
            let side = SemanticFeatureSide::ALL
                .get(side.max(0) as usize)
                .copied()
                .unwrap_or(SemanticFeatureSide::Center);
            let height_band = SemanticHeightBand::ALL
                .get(height.max(0) as usize)
                .copied()
                .unwrap_or(SemanticHeightBand::Lower);
            let strength = SemanticStrength::ALL
                .get(strength.max(0) as usize)
                .copied()
                .unwrap_or(SemanticStrength::Visible);
            let snapshot = {
                let state = state.borrow();
                let project = state.project.as_ref();
                project.and_then(|project| {
                    let slot_id =
                        project.detailed.selected_slot_id.clone().or_else(|| {
                            project.building_slots.first().map(|slot| slot.id.clone())
                        })?;
                    let refinement = project.latest_refinement(&slot_id)?;
                    (refinement.status == campus_state::RefinementStatus::Draft).then(|| {
                        (
                            refinement.generated_path.clone(),
                            slot_id,
                            refinement.id.clone(),
                        )
                    })
                })
            };
            let result = snapshot
                .ok_or_else(|| "请先生成尚未确认的 refinement 草稿".to_string())
                .and_then(|(path, slot_id, refinement_id)| {
                    apply_semantic_feature(
                        &path,
                        kind,
                        side,
                        height_band,
                        strength,
                        label.as_str(),
                        reason.as_str(),
                    )
                    .map(|(affected, block)| (slot_id, refinement_id, affected, block))
                });
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok((slot_id, refinement_id, affected, block)) => {
                        state.borrow_mut().mutate_project(|project| {
                            project.record_semantic_feature(
                                &slot_id,
                                &refinement_id,
                                SemanticFeatureDraft {
                                    kind,
                                    label: label.to_string(),
                                    side,
                                    height_band,
                                    strength,
                                    reason: reason.to_string(),
                                },
                                affected,
                                block.clone(),
                            );
                        });
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        } else {
                            ui.set_semantic_feature_label("".into());
                            ui.set_semantic_feature_reason("".into());
                            set_status(
                                &ui,
                                format!("已应用 {}：{} 个方块使用 {block}", kind.label(), affected),
                                format!("Semantic feature applied: {affected} blocks use {block}"),
                            );
                        }
                    }
                    Err(error) => {
                        set_localized_error(
                            &ui,
                            "semantic-feature.apply",
                            format!("语义特征应用失败：{error}"),
                            format!("Failed to apply semantic feature: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_replace_selected_block(move |target| {
            let snapshot = {
                let state = state.borrow();
                state
                    .active_preview_path
                    .clone()
                    .zip(state.selected_preview_block.clone())
            };
            let result = snapshot
                .ok_or_else(|| "请先在原生预览中选择一个方块".to_string())
                .and_then(|(path, selection)| {
                    replace_generated_block_at(
                        &path,
                        selection.x,
                        selection.y,
                        selection.z,
                        target.as_str(),
                    )
                    .map(|previous| (selection, previous))
                });
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok((selection, previous)) => {
                        let normalized = normalize_minecraft_block(target.as_str())
                            .unwrap_or_else(|_| target.to_string());
                        state.borrow_mut().selected_preview_block =
                            Some(campus_state::PreviewBlockSelection {
                                block: normalized.clone(),
                                ..selection
                            });
                        sync_ui(&ui, &state.borrow());
                        let detail = format!(
                            "({}, {}, {}): {previous} → {normalized}",
                            selection.x, selection.y, selection.z
                        );
                        set_status(&ui, format!("已编辑 {detail}"), format!("Edited {detail}"));
                    }
                    Err(error) => set_localized_error(
                        &ui,
                        "preview-block.replace",
                        format!("单点编辑失败：{error}"),
                        format!("Single-block edit failed: {error}"),
                    ),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_replace_generated_block(move |source, target| {
            let path = state
                .borrow()
                .project
                .as_ref()
                .and_then(|project| project.detailed.generated_path.clone());
            let result = path
                .as_ref()
                .ok_or_else(|| "请先生成精细建筑".to_string())
                .and_then(|path| replace_generated_block(path, source.as_str(), target.as_str()));
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(count) => {
                        sync_ui(&ui, &state.borrow());
                        set_status(
                            &ui,
                            format!("已替换 {count} 个方块，请重新打开预览"),
                            format!("Replaced {count} blocks; reopen the preview"),
                        );
                    }
                    Err(error) => {
                        set_localized_error(
                            &ui,
                            "generated-block.replace",
                            format!("替换失败：{error}"),
                            format!("Replacement failed: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_export_building(move || {
            let result = (|| -> Result<PathBuf, String> {
                let generated_path = state
                    .borrow()
                    .project
                    .as_ref()
                    .and_then(|project| project.detailed.generated_path.clone())
                    .ok_or("请先生成当前精细建筑")?;
                let generated: arnis_core::GeneratedBuilding = serde_json::from_slice(
                    &std::fs::read(&generated_path).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let default_name = format!("{}.schem", generated.report.candidate_id);
                let path = schematic_file_dialog(&default_name).ok_or("已取消导出")?;
                let model = campus_export::model_from_runs(
                    generated.width,
                    generated.height,
                    generated.length,
                    generated.palette,
                    generated
                        .block_runs
                        .into_iter()
                        .map(|run| (run.palette_index, run.run_length)),
                )?;
                campus_export::write_schematic(&path, &generated.report.candidate_id, &model)?;
                Ok(path)
            })();
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(path) => {
                        set_status(
                            &ui,
                            format!("精细建筑已导出：{}", path.display()),
                            format!("Detailed building exported: {}", path.display()),
                        );
                    }
                    Err(error) if error != "已取消导出" => {
                        set_localized_error(
                            &ui,
                            "detailed-building.export",
                            format!("导出失败：{error}"),
                            format!("Export failed: {error}"),
                        );
                    }
                    Err(_) => {}
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_export_project(move || {
            let result = (|| -> Result<PathBuf, String> {
                let project = state.borrow().project.clone().ok_or("请先创建项目")?;
                let default_name = format!("{}.schem", project.name);
                let path = schematic_file_dialog(&default_name).ok_or("已取消导出")?;
                let model = compile_foundation(&project)?;
                campus_export::write_schematic(&path, &project.name, &model)?;
                let project_path = path.with_extension("campus.json");
                std::fs::write(
                    &project_path,
                    serde_json::to_vec_pretty(&project).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                Ok(path)
            })();
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(path) => {
                        set_status(
                            &ui,
                            format!("Foundation 已导出：{}", path.display()),
                            format!("Foundation exported: {}", path.display()),
                        );
                    }
                    Err(error) if error != "已取消导出" => {
                        set_localized_error(
                            &ui,
                            "foundation.export",
                            format!("导出失败：{error}"),
                            format!("Export failed: {error}"),
                        );
                    }
                    Err(_) => {}
                }
            }
        });
    }

    let result = ui.run();
    drop(production_acquisition_client);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detailed_fixture_generates_preview_model() {
        let output = tempfile::tempdir().unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-data/v1-demo.campus.json");
        let mut loaded = DesktopApplicationState::default();
        loaded.open(fixture).unwrap();
        let state = Rc::new(RefCell::new(loaded));
        let (path, title) = generate_detailed_model_to(&state, output.path()).unwrap();
        let generated: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(title, "Fixture Library");
        assert!(generated.report.non_air_blocks > 1_000);
        assert!(generated
            .palette
            .iter()
            .any(|block| block.contains("glass")));
        let (semantic_affected, semantic_block) = apply_semantic_feature(
            &path,
            SemanticFeatureKind::WindowBand,
            SemanticFeatureSide::South,
            SemanticHeightBand::Middle,
            SemanticStrength::Visible,
            "south window band",
            "fixture evidence",
        )
        .unwrap();
        assert!(semantic_affected > 0);
        assert_eq!(semantic_block, "minecraft:glass");
        let source = generated
            .palette
            .iter()
            .find(|block| block.as_str() != "minecraft:air")
            .unwrap()
            .clone();
        let replaced =
            replace_generated_block(&path, &source, "minecraft:purple_concrete").unwrap();
        assert!(replaced > 0);
        let updated: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(updated
            .palette
            .iter()
            .any(|block| block == "minecraft:purple_concrete"));
        let _previous =
            replace_generated_block_at(&path, 0, 0, 0, "minecraft:diamond_block").unwrap();
        let point_edited: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(point_edited
            .report
            .correction_notes
            .iter()
            .any(|note| note.contains("single block edit")));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn accepted_facade_rules_change_the_generated_voxels() {
        let output = tempfile::tempdir().unwrap();
        fn glass_blocks(generated: &arnis_core::GeneratedBuilding) -> u64 {
            let glass_indices = generated
                .palette
                .iter()
                .enumerate()
                .filter(|(_, block)| block.contains("glass"))
                .map(|(index, _)| index as u16)
                .collect::<Vec<_>>();
            generated
                .block_runs
                .iter()
                .filter(|run| glass_indices.contains(&run.palette_index))
                .map(|run| run.run_length as u64)
                .sum()
        }

        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-data/v1-demo.campus.json");
        let mut loaded = DesktopApplicationState::default();
        loaded.open(fixture).unwrap();
        let slot_id = loaded
            .project
            .as_ref()
            .and_then(|project| project.detailed.selected_slot_id.clone())
            .or_else(|| {
                loaded
                    .project
                    .as_ref()
                    .and_then(|project| project.building_slots.first().map(|slot| slot.id.clone()))
            })
            .unwrap();
        let state = Rc::new(RefCell::new(loaded));
        let add_rule = |state: &Rc<RefCell<DesktopApplicationState>>, id: &str, density: u8| {
            state.borrow_mut().mutate_project(|project| {
                project
                    .detailed
                    .facade_drafts
                    .push(campus_state::FacadeReconstructionDraft {
                        id: id.into(),
                        slot_id: slot_id.clone(),
                        model_version: "test".into(),
                        confidence: 100,
                        rules: vec![campus_state::EditableFacadeRule {
                            id: format!("{id}:windows"),
                            slot_id: slot_id.clone(),
                            kind: campus_state::FacadeRuleKind::WindowPattern,
                            value: format!("density:{density}"),
                            source: campus_state::DetailedRuleSource::ManualOverride,
                            status: campus_state::DetailedRuleStatus::Accepted,
                            confidence: 100,
                            evidence_ids: Vec::new(),
                        }],
                        evidence_ids: Vec::new(),
                    });
            });
        };

        add_rule(&state, "low", 5);
        let (low_path, _) = generate_detailed_model_to(&state, output.path()).unwrap();
        let low: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&low_path).unwrap()).unwrap();
        add_rule(&state, "high", 95);
        let (high_path, _) = generate_detailed_model_to(&state, output.path()).unwrap();
        let high: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&high_path).unwrap()).unwrap();

        assert!(glass_blocks(&high) > glass_blocks(&low));
        let _ = std::fs::remove_file(low_path);
        let _ = std::fs::remove_file(high_path);
    }

    #[test]
    fn locale_changes_copy_without_changing_project_state() {
        let (title, _, action) = page_copy(FoundationStep::Boundary, DesktopLocale::En);
        assert_eq!(title, "Confirm campus boundary");
        assert_eq!(action, "Confirm boundary and continue");
        let mut state = DesktopApplicationState::default();
        state.locale = DesktopLocale::En;
        state.new_project("test", "campus");
        assert_eq!(state.locale, DesktopLocale::En);
        assert_eq!(state.project.as_ref().unwrap().campus_name, "campus");
    }

    #[test]
    fn foundation_phase_navigation_requires_completed_prerequisites() {
        let mut project = CampusProject::new("test", "campus");
        let initial = FoundationWorkflow::projection(&project);
        assert!(!initial.can_enter_review);
        assert!(!initial.can_enter_generate);

        project.campus_target = Some(CampusTargetEvidence {
            poi_id: "campus".into(),
            name: "campus".into(),
            gcj02: GeoPoint { lng: 1.0, lat: 1.0 },
            wgs84: GeoPoint { lng: 1.0, lat: 1.0 },
            acquisition: "test".into(),
        });
        project.boundary = vec![
            GeoPoint { lng: 1.0, lat: 1.0 },
            GeoPoint { lng: 2.0, lat: 1.0 },
            GeoPoint { lng: 2.0, lat: 2.0 },
        ];
        project.completed_steps.push(FoundationStep::Orientation);
        assert!(FoundationWorkflow::projection(&project).can_enter_review);
        project.completed_steps.push(FoundationStep::Sports);
        assert!(FoundationWorkflow::projection(&project).can_enter_generate);
    }

    #[test]
    fn closed_tool_event_terminates_the_ipc_read_loop() {
        assert!(tool_event_ends_stream(&ToolEvent::Closed {
            tool: ToolKind::Map
        }));
        assert!(tool_event_ends_stream(&ToolEvent::Closed {
            tool: ToolKind::Preview
        }));
        assert!(!tool_event_ends_stream(&ToolEvent::Ready {
            protocol_version: PROTOCOL_VERSION,
            tool: ToolKind::Map,
        }));
    }

    #[test]
    fn main_workflow_content_is_scrollable() {
        let ui = include_str!("../ui/app.slint");
        assert!(
            ui.contains("workflow-scroll := ScrollView"),
            "the workflow body must scroll so detailed export and long candidate lists stay reachable"
        );
        assert!(
            ui.contains("workflow-content := VerticalLayout {") && ui.contains("alignment: start;"),
            "scroll content must keep its natural height instead of compressing its last controls"
        );
        assert!(
            ui.contains("detailed-export := Button"),
            "the detailed export action must remain fixed outside the scroll viewport"
        );
        assert!(
            ui.contains("workflow-scroll := ScrollView {")
                && ui.matches("preferred-height: 0px;").count() >= 2,
            "the scroll viewport must yield space to the fixed footer action"
        );
        assert!(
            ui.contains("width: 1280px;") && ui.contains("height: 720px;"),
            "the default window must fit a common 1366x768 desktop work area"
        );
    }

    #[test]
    fn project_workbench_exposes_tasks_current_work_and_project_context() {
        let ui = include_str!("../ui/app.slint");
        assert!(
            ui.contains("PROJECT TASKS")
                && ui.contains("CURRENT TASK")
                && ui.contains("PROJECT STATUS"),
            "the selected project workbench must expose tasks, current work, and project context"
        );
        assert!(
            ui.contains("1. 选择校区")
                && ui.contains("2. 确认范围")
                && ui.contains("3. 审核地基")
                && ui.contains("4. 生成地基")
                && ui.contains("5. 单栋精修"),
            "the task rail must use user goals rather than internal workflow phases"
        );
        assert!(
            !ui.contains("for label[index] in root.step-labels"),
            "Foundation navigation must not expose all internal workflow states as sidebar buttons"
        );
        assert!(
            ui.contains("root.detailed-task == 1")
                && ui.contains("root.detailed-task == 2")
                && ui.contains("root.detailed-task == 3")
                && ui.contains("root.detailed-task == 4")
                && ui.contains("CHOOSE ONE REVIEWED BUILDING")
                && ui.contains("AUTOMATIC MATCH")
                && ui.contains("EDITABLE FACADE RULES")
                && ui.contains("REVIEW GENERATED BUILDING"),
            "Detailed Building must expose one progressive task at a time"
        );
        assert!(
            !ui.contains("visible: false;\n                        spacing: 12px;\n                        SectionLabel { text: root.english ? \"TARGET BUILDING\""),
            "the retired all-in-one detailed editor must not remain as hidden dead UI"
        );
        assert!(
            ui.contains("APPLICATION MENU") && ui.contains("root.utilities-visible = true"),
            "secondary project, settings, and diagnostic actions must use the application menu"
        );
        assert!(
            ui.contains("创建项目并选择高德校区")
                && !ui.contains("创建本地项目\"; clicked")
                && !ui.contains("在高德中选择校区\"; clicked"),
            "project creation and campus selection must be one continuous action"
        );
        assert!(
            ui.contains("root.active-step > 2 && root.active-step < 8"),
            "campus boundary must not render the candidate-review toolbar"
        );
    }

    #[test]
    fn native_launcher_orchestration_drives_real_window_and_schema2_library() {
        let directory = tempfile::tempdir().unwrap();
        let mut launcher = CampusProjectLauncher::open(
            directory.path(),
            campus_state::V11ConstructionCapability::request(true, Some("1")).unwrap(),
            campus_state::InstallationId::new("native-orchestration-test").unwrap(),
        )
        .unwrap();
        let window = AppWindow::new().expect("native launcher shell should instantiate");
        window.set_english(true);

        launcher.select_campus_candidate(
            CampusScope::new("gaode:native-test", "Native Test Campus", [121.395, 31.202]).unwrap(),
        );
        sync_project_launcher_ui(&window, &launcher).unwrap();
        assert_eq!(window.get_launcher_step(), 0);
        assert_eq!(window.get_launcher_projects().row_count(), 0);

        launcher.confirm_selected_campus().unwrap();
        sync_project_launcher_ui(&window, &launcher).unwrap();
        assert_eq!(window.get_launcher_step(), 1);

        let project_id = launcher.create_project("Native Project").unwrap();
        sync_project_launcher_ui(&window, &launcher).unwrap();
        assert_eq!(window.get_launcher_step(), 2);
        assert_eq!(
            window.get_launcher_active_project_id().as_str(),
            project_id.as_str()
        );
        assert_eq!(window.get_launcher_next_task(), "Confirm Campus Boundary");
        window.set_english(false);
        sync_project_launcher_ui(&window, &launcher).unwrap();
        assert_eq!(window.get_launcher_next_task(), "确认校园边界");
        window.set_english(true);
        sync_project_launcher_ui(&window, &launcher).unwrap();

        launcher.request_save().unwrap();

        launcher.show_project_library();
        sync_project_launcher_ui(&window, &launcher).unwrap();
        assert_eq!(window.get_launcher_step(), 1);
        let projects = window.get_launcher_projects();
        assert_eq!(projects.row_count(), 1);
        assert!(
            projects
                .row_data(0)
                .expect("project row")
                .latest_save
                .ends_with(" UTC"),
            "latest successful save must be user-readable"
        );
    }

    #[test]
    fn latest_save_time_is_a_readable_utc_timestamp() {
        assert_eq!(format_latest_save_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(
            format_latest_save_time(1_783_353_720_000),
            "2026-07-06 16:02 UTC"
        );
    }

    #[test]
    fn route_first_shell_exposes_the_two_step_campus_project_flow() {
        let source = include_str!("../ui/app.slint");

        assert!(
            !source.contains("if !root.campus-launcher-visible || root.launcher-step == 2"),
            "campus-first steps must not be hidden by their parent layout"
        );
        assert!(
            source.contains("root.campus-launcher-visible && root.launcher-step == 2")
                && source.contains("This workspace is backed only by the active project session.")
                && source.contains("此工作区只使用当前项目会话"),
            "the active project must open a dedicated localized workspace"
        );
        assert!(
            source.contains("CAMPUS TARGET")
                && source.contains("PROJECT")
                && source.contains("FOUNDATION")
                && source.contains("DETAILED")
                && source.contains("AXIOM"),
            "Variant A must expose the horizontal five-stage product route"
        );
        assert!(
            source.contains("root.launcher-step == 0")
                && source.contains("root.launcher-step == 1")
                && source.contains("for project in root.launcher-projects"),
            "Campus Target confirmation must precede the campus-scoped project table"
        );
        assert!(
            source.contains("LATEST SAVE")
                && source.contains("SAVE / RECOVERY")
                && source.contains("PROGRESS")
                && source.contains("NEXT TASK")
                && source.contains("MINECRAFT COMPATIBILITY"),
            "project rows and compact context must expose durable resume information"
        );
        assert!(
            source.contains("CURRENT TASK") && source.contains("PROJECT CONTEXT"),
            "one dominant current-task workspace must sit beside a compact project summary"
        );
        assert!(
            source.contains("@keys(Control + S)")
                && source.contains("@keys(Control + Z)")
                && source.contains("@keys(Control + Y)")
                && source.contains("@keys(Control + Shift + Z)"),
            "fixed save and history shortcuts must remain available in schema-2 mode"
        );
        assert!(
            !source.contains("PERSISTENT CAMPUS SIDEBAR")
                && !source.contains("RESUME HERO")
                && !source.contains("GLOBAL RECENT FILES")
                && !source.contains("PROJECT WORKBENCH")
        );
    }

    #[test]
    fn guidance_settings_shortcuts_and_secret_fields_are_production_surfaces() {
        let source = include_str!("../ui/app.slint");

        assert!(
            source.contains("root.guidance-step == 0")
                && source.contains("root.guidance-step == 4")
                && source.contains("root.dismiss-guidance(true)")
                && source.contains("root.dismiss-guidance(false)"),
            "first-run guidance must expose five skippable steps"
        );
        for term in [
            "校区目标",
            "校区项目库",
            "已知地物缺口",
            "已审核校园模型",
            "校园基础清单",
            "项目保存点",
            "项目恢复状态",
            "便携项目",
        ] {
            assert!(
                source.contains(term),
                "missing Chinese product term: {term}"
            );
        }
        for shortcut in [
            "@keys(Control + N)",
            "@keys(Control + O)",
            "@keys(Control + S)",
            "@keys(Control + Shift + S)",
            "@keys(Control + Z)",
            "@keys(Control + Y)",
            "@keys(Control + Shift + Z)",
            "@keys(Delete)",
            "@keys(Control + Return)",
            "@keys(Escape)",
            "@keys(F1)",
        ] {
            assert!(source.contains(shortcut), "missing shortcut: {shortcut}");
        }
        assert!(
            source.matches("input-type: password").count() >= 3,
            "Gaode and acquisition secrets must be masked by default"
        );
        assert!(
            source.contains("BUNDLED QUICK START")
                && source.contains("MINECRAFT 26.1.2 / AXIOM")
                && source.contains("@image-url(\"quick-start/01-campus-target.jpg\")")
                && source.contains("@image-url(\"quick-start/02-foundation-detailed.jpg\")")
                && source.contains("@image-url(\"quick-start/03-minecraft-axiom.jpg\")"),
            "the offline three-screenshot quick start must be bundled into the App"
        );
        for screenshot in [
            include_bytes!("../ui/quick-start/01-campus-target.jpg").as_slice(),
            include_bytes!("../ui/quick-start/02-foundation-detailed.jpg").as_slice(),
            include_bytes!("../ui/quick-start/03-minecraft-axiom.jpg").as_slice(),
        ] {
            assert!(screenshot.starts_with(b"\xff\xd8\xff"));
            assert!(
                screenshot.len() > 50_000,
                "quick-start screenshot is only a placeholder"
            );
        }
        assert!(
            source.matches("preferred-height: 0px;").count() >= 2
                && source.contains("width: min(1000px, root.width - 48px)")
                && source.contains("width: min(780px, root.width - 48px)")
                && source.contains("width: min(1100px, root.width - 48px)"),
            "core surfaces must scroll and adapt at 100%, 125%, and 150% Windows scale"
        );
        assert!(
            source.contains("callback shortcut-requested(int, bool, int, int)")
                && source.contains("changed has-focus")
                && source.contains("root.shortcut-modal")
                && source.contains("root.map-tool-state"),
            "shortcut routing must receive live text, modal, and map-tool context"
        );
        assert!(
            source.contains("APPLICATION MENU") && !source.contains("Save as"),
            "Settings and Portable Project Export must use application-menu vocabulary"
        );
        for prototype_control in [
            "PROTOTYPE STATE",
            "variant switcher",
            "debug overlay",
            "data-action=\"toggle-text\"",
        ] {
            assert!(
                !source.contains(prototype_control),
                "production UI contains prototype-only control: {prototype_control}"
            );
        }
    }
}
