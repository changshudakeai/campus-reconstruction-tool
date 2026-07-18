#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod v11_acquisition_client;
mod v11_project_kernel;

use arnis_core::{FootprintComponent, GenerateBuildingRequest, MaterialOverrides};
use campus_state::{
    ArnisStylePreset, CampusTargetEvidence, CandidateConfidenceFilter, DesktopApplicationState,
    DesktopLocale, DesktopMode, ExternalModelDecision, FeatureKind, FoundationStep,
    FoundationStylePack, FoundationStylePreset, GeoPoint, MapCandidate, MapViewState,
    ReviewDecision, SemanticFeatureDraft, SemanticFeatureKind, SemanticFeatureSide,
    SemanticHeightBand, SemanticStrength, SourceConflictDecision,
};
use campus_tool_protocol::{
    read_message, write_message, MapCoordinate, MapOverlay, MapPurpose, ToolCommand, ToolEvent,
    ToolKind, PROTOCOL_VERSION,
};
use rand::Rng;
use slint::{ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
#[cfg(target_os = "windows")]
use tokio::net::windows::named_pipe::ServerOptions;

slint::include_modules!();

fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("CampusReconstructionTool")
}

fn default_project_path() -> PathBuf {
    app_data_dir().join("projects").join("active.campus.json")
}

fn generated_model_dir() -> PathBuf {
    app_data_dir().join("generated")
}

fn visual_capture_dir() -> PathBuf {
    app_data_dir().join("captures")
}

fn locale_path() -> PathBuf {
    app_data_dir().join("locale.txt")
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

#[derive(Clone, Default)]
struct MapCredentials {
    js_api_key: String,
    security_code: String,
}

fn credential_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("Campus Reconstruction Tool", account).map_err(|error| error.to_string())
}

fn load_map_credentials() -> MapCredentials {
    let from_store = |account: &str| {
        credential_entry(account)
            .and_then(|entry| entry.get_password().map_err(|error| error.to_string()))
            .unwrap_or_default()
    };
    let stored_key = from_store("gaode-js-api-key");
    let stored_security = from_store("gaode-security-code");
    MapCredentials {
        js_api_key: if stored_key.is_empty() {
            std::env::var("GAODE_JS_API_KEY")
                .or_else(|_| std::env::var("VITE_GAODE_JS_API_KEY"))
                .unwrap_or_default()
        } else {
            stored_key
        },
        security_code: if stored_security.is_empty() {
            std::env::var("GAODE_SECURITY_CODE")
                .or_else(|_| std::env::var("VITE_GAODE_SECURITY_CODE"))
                .unwrap_or_default()
        } else {
            stored_security
        },
    }
}

fn save_map_credentials(credentials: &MapCredentials) -> Result<(), String> {
    credential_entry("gaode-js-api-key")?
        .set_password(&credentials.js_api_key)
        .map_err(|error| error.to_string())?;
    credential_entry("gaode-security-code")?
        .set_password(&credentials.security_code)
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
                "Review buildings and fill gaps",
                "Accept reliable building candidates, then recover missing geometry from the map view or manual drawing.",
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
    ui.set_detailed_active(project.mode == DesktopMode::Detailed);
    ui.set_active_step(
        FoundationStep::ALL
            .iter()
            .position(|step| *step == project.foundation_step)
            .unwrap_or(0) as i32,
    );
    let (title, help, primary) = if project.mode == DesktopMode::Detailed {
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
}

fn candidate_matches_filter(candidate: &MapCandidate, filter: CandidateConfidenceFilter) -> bool {
    match filter {
        CandidateConfidenceFilter::All => candidate.review == ReviewDecision::Pending,
        CandidateConfidenceFilter::High => {
            candidate.review == ReviewDecision::Pending
                && candidate.confidence.eq_ignore_ascii_case("high")
        }
        CandidateConfidenceFilter::Medium => {
            candidate.review == ReviewDecision::Pending
                && (candidate.confidence.eq_ignore_ascii_case("medium")
                    || candidate.confidence.eq_ignore_ascii_case("manual"))
        }
        CandidateConfidenceFilter::Low => {
            candidate.review == ReviewDecision::Pending
                && candidate.confidence.eq_ignore_ascii_case("low")
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

fn set_error(ui: &AppWindow, message: impl AsRef<str>) {
    ui.set_save_status(
        if ui.get_english() {
            format!("Operation failed: {}", message.as_ref())
        } else {
            format!("保存失败：{}", message.as_ref())
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

fn generate_foundation_preview(
    state: &Rc<RefCell<DesktopApplicationState>>,
) -> Result<(PathBuf, String), String> {
    let project = state.borrow().project.clone().ok_or("请先创建项目")?;
    let model = campus_export::foundation_model(&project)?;
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
    let (slot, style, density, depth, scale, version) = {
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
        (
            slot,
            project.detailed.style_preset,
            project.detailed.window_density,
            project.detailed.wall_depth,
            project.blocks_per_meter,
            version,
        )
    };
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
        floors: slot.floors,
        roof_shape: slot.roof_shape.clone(),
        blocks_per_meter: scale,
        seed: 42,
        materials: MaterialOverrides::default(),
        correction_notes: vec![format!("V1 fixed Arnis preset: {}", style.slug())],
        parts: Vec::new(),
        style_preset: style.slug().into(),
        window_density: density,
        wall_depth: depth,
    })?;
    std::fs::create_dir_all(generated_model_dir()).map_err(|error| error.to_string())?;
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
    let path = generated_model_dir().join(format!("{safe_id}-v{version}.arnis.json"));
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
    ManualFeature {
        kind: FeatureKind,
        points: Vec<GeoPoint>,
    },
    MapCapture {
        south_west: GeoPoint,
        north_east: GeoPoint,
        candidates: Result<Vec<MapCandidate>, String>,
    },
    MapVisualCapture {
        south_west: GeoPoint,
        north_east: GeoPoint,
        png_bytes: Vec<u8>,
        candidates: Result<Vec<MapCandidate>, String>,
    },
}

#[derive(Clone)]
struct ToolSupervisor {
    children: Arc<Mutex<Vec<Child>>>,
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

impl ToolSupervisor {
    fn tool_executable(name: &str) -> Result<PathBuf, String> {
        let directory = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .parent()
            .ok_or("native executable has no parent")?
            .to_path_buf();
        let executable = directory.join(format!("{name}.exe"));
        executable
            .exists()
            .then_some(executable)
            .ok_or_else(|| format!("{name}.exe is not installed beside the main application"))
    }

    #[cfg(target_os = "windows")]
    fn launch_map(
        &self,
        ui: slint::Weak<AppWindow>,
        request: MapLaunchRequest,
    ) -> Result<(), String> {
        let executable = Self::tool_executable("campus-map")?;
        let random: u128 = rand::rng().random();
        let pipe = format!(r"\\.\pipe\campus-reconstruction-{random:032x}");
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .map_err(|error| error.to_string())?;
        let child = Command::new(executable)
            .arg(&pipe)
            .arg(&token)
            .spawn()
            .map_err(|error| error.to_string())?;
        self.children
            .lock()
            .map_err(|_| "tool child lock poisoned")?
            .push(child);
        let updates = self.updates.clone();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("map supervisor runtime");
            let error_ui = ui.clone();
            let result: Result<(), String> = runtime.block_on(async move {
                let mut server = server;
                server.connect().await.map_err(|error| error.to_string())?;
                let hello: ToolCommand = read_message(&mut server).await?;
                match hello {
                    ToolCommand::Hello {
                        protocol_version,
                        session_token,
                        tool: ToolKind::Map,
                    } if protocol_version == PROTOCOL_VERSION && session_token == token => {}
                    _ => return Err("map tool handshake rejected".into()),
                }
                let analysis_campus = request.title.clone();
                let locale = if request.english {
                    DesktopLocale::En
                } else {
                    DesktopLocale::ZhCn
                };
                write_message(
                    &mut server,
                    &ToolCommand::OpenMap {
                        campus_name: request.title,
                        center_lng: request.view.center.lng,
                        center_lat: request.view.center.lat,
                        zoom: request.view.zoom,
                        pitch: request.view.pitch,
                        rotation: request.view.rotation,
                        js_api_key: request.js_api_key,
                        security_code: request.security_code,
                        boundary: request
                            .boundary
                            .into_iter()
                            .map(campus_services::wgs84_to_gcj02)
                            .map(|point| MapCoordinate {
                                lng: point.lng,
                                lat: point.lat,
                            })
                            .collect(),
                        purpose: request.purpose,
                        overlays: request
                            .overlays
                            .into_iter()
                            .map(|overlay| MapOverlay {
                                label: overlay.label,
                                points: overlay
                                    .points
                                    .into_iter()
                                    .map(|point| {
                                        let point = campus_services::wgs84_to_gcj02(GeoPoint {
                                            lng: point.lng,
                                            lat: point.lat,
                                        });
                                        MapCoordinate {
                                            lng: point.lng,
                                            lat: point.lat,
                                        }
                                    })
                                    .collect(),
                            })
                            .collect(),
                        feature_kind: request.feature_kind,
                        english: request.english,
                    },
                )
                .await?;
                loop {
                    let event: ToolEvent = read_message(&mut server).await?;
                    let message = match event {
                        ToolEvent::Ready { .. } => {
                            tr(locale, "高德工具已连接", "Gaode tool connected").to_string()
                        }
                        ToolEvent::MapCamera {
                            center_lng,
                            center_lat,
                            zoom,
                            pitch,
                            rotation,
                        } => {
                            let _ = updates.send(ToolUpdate::MapCamera {
                                center: GeoPoint {
                                    lng: center_lng,
                                    lat: center_lat,
                                },
                                zoom,
                                pitch,
                                rotation,
                            });
                            if locale == DesktopLocale::En {
                                format!("Map view recorded · zoom {zoom:.1} · pitch {pitch:.0}")
                            } else {
                                format!("地图视角已记录 · zoom {zoom:.1} · pitch {pitch:.0}")
                            }
                        }
                        ToolEvent::MapPointSelected { lng, lat } => {
                            let _ = updates.send(ToolUpdate::MapPoint(GeoPoint { lng, lat }));
                            if locale == DesktopLocale::En {
                                format!("Selected location {lng:.6}, {lat:.6}")
                            } else {
                                format!("已选择位置 {lng:.6}, {lat:.6}")
                            }
                        }
                        ToolEvent::MapCampusSelected {
                            poi_id,
                            name,
                            lng,
                            lat,
                        } => {
                            let gcj02 = GeoPoint { lng, lat };
                            let evidence = CampusTargetEvidence {
                                poi_id,
                                name: name.clone(),
                                gcj02,
                                wgs84: campus_services::gcj02_to_wgs84(gcj02),
                                acquisition: "gaode_poi_search".into(),
                            };
                            let _ = updates.send(ToolUpdate::MapCampusTarget(evidence));
                            if locale == DesktopLocale::En {
                                format!("Campus confirmed: {name}")
                            } else {
                                format!("已确认校园：{name}")
                            }
                        }
                        ToolEvent::MapBoundaryChanged { points } => {
                            let count = points.len();
                            let _ = updates.send(ToolUpdate::MapBoundary(
                                points
                                    .into_iter()
                                    .map(|point| {
                                        campus_services::gcj02_to_wgs84(GeoPoint {
                                            lng: point.lng,
                                            lat: point.lat,
                                        })
                                    })
                                    .collect(),
                            ));
                            if locale == DesktopLocale::En {
                                format!("Campus boundary saved · {count} nodes")
                            } else {
                                format!("校区边界已保存 · {count} 个节点")
                            }
                        }
                        ToolEvent::MapFeatureDrawn { kind, points } => {
                            let kind = match kind.as_str() {
                                "building" => FeatureKind::Building,
                                "road" => FeatureKind::Road,
                                "water" => FeatureKind::Water,
                                "vegetation" => FeatureKind::Vegetation,
                                "sports" => FeatureKind::Sports,
                                _ => {
                                    let _ = updates.send(ToolUpdate::Status(
                                        tr(
                                            locale,
                                            "手绘地物类型无效",
                                            "Invalid manual feature type",
                                        )
                                        .into(),
                                    ));
                                    continue;
                                }
                            };
                            let count = points.len();
                            let _ = updates.send(ToolUpdate::ManualFeature {
                                kind,
                                points: points
                                    .into_iter()
                                    .map(|point| {
                                        campus_services::gcj02_to_wgs84(GeoPoint {
                                            lng: point.lng,
                                            lat: point.lat,
                                        })
                                    })
                                    .collect(),
                            });
                            if locale == DesktopLocale::En {
                                format!("Manual feature received · {count} nodes")
                            } else {
                                format!("已接收手绘地物 · {count} 个节点")
                            }
                        }
                        ToolEvent::MapCaptureRequested {
                            south_west_lng,
                            south_west_lat,
                            north_east_lng,
                            north_east_lat,
                        } => {
                            let south_west = campus_services::gcj02_to_wgs84(GeoPoint {
                                lng: south_west_lng,
                                lat: south_west_lat,
                            });
                            let north_east = campus_services::gcj02_to_wgs84(GeoPoint {
                                lng: north_east_lng,
                                lat: north_east_lat,
                            });
                            let _ = updates.send(ToolUpdate::Status(
                                tr(
                                    locale,
                                    "已锁定当前视野，正在识别校区地物…",
                                    "Current view locked; discovering campus features…",
                                )
                                .into(),
                            ));
                            let bounds = campus_services::GeoBounds {
                                west: south_west.lng,
                                south: south_west.lat,
                                east: north_east.lng,
                                north: north_east.lat,
                            };
                            let overture_endpoint = std::env::var("CAMPUS_DATA_SERVICE_URL")
                                .or_else(|_| std::env::var("OVERTURE_BUILDING_ENDPOINT"))
                                .ok();
                            let candidates = campus_services::query_campus_data(
                                bounds,
                                overture_endpoint.as_deref(),
                            )
                            .await;
                            let _ = updates.send(ToolUpdate::MapCapture {
                                south_west,
                                north_east,
                                candidates,
                            });
                            tr(locale, "已接收当前视野范围", "Current view bounds received")
                                .to_string()
                        }
                        ToolEvent::MapVisualCapture {
                            image_data_url,
                            south_west_lng,
                            south_west_lat,
                            north_east_lng,
                            north_east_lat,
                        } => {
                            let south_west = campus_services::gcj02_to_wgs84(GeoPoint {
                                lng: south_west_lng,
                                lat: south_west_lat,
                            });
                            let north_east = campus_services::gcj02_to_wgs84(GeoPoint {
                                lng: north_east_lng,
                                lat: north_east_lat,
                            });
                            let bounds = campus_services::GeoBounds {
                                west: south_west.lng,
                                south: south_west.lat,
                                east: north_east.lng,
                                north: north_east.lat,
                            };
                            let analysis = campus_services::analyze_visual_capture(
                                &image_data_url,
                                bounds,
                                &analysis_campus,
                            );
                            match analysis {
                                Ok((png_bytes, candidates)) => {
                                    let count = candidates.len();
                                    let _ = updates.send(ToolUpdate::MapVisualCapture {
                                        south_west,
                                        north_east,
                                        png_bytes,
                                        candidates: Ok(candidates),
                                    });
                                    if locale == DesktopLocale::En {
                                        format!("Visual recovery complete · {count} candidates")
                                    } else {
                                        format!("视觉补缺完成 · {count} 个候选")
                                    }
                                }
                                Err(error) => {
                                    let _ = updates.send(ToolUpdate::MapVisualCapture {
                                        south_west,
                                        north_east,
                                        png_bytes: Vec::new(),
                                        candidates: Err(error.clone()),
                                    });
                                    if locale == DesktopLocale::En {
                                        format!("Visual recovery failed: {error}")
                                    } else {
                                        format!("视觉补缺失败：{error}")
                                    }
                                }
                            }
                        }
                        ToolEvent::Error { message } => {
                            if locale == DesktopLocale::En {
                                format!("Map error: {message}")
                            } else {
                                format!("地图错误：{message}")
                            }
                        }
                        ToolEvent::Closed { .. } => {
                            tr(locale, "高德工具已关闭", "Gaode tool closed").to_string()
                        }
                        _ => continue,
                    };
                    let weak = ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_save_status(message.into());
                        }
                    });
                }
            });
            if let Err(error) = result {
                let weak = error_ui;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        set_status(
                            &ui,
                            format!("地图连接失败：{error}"),
                            format!("Map connection failed: {error}"),
                        );
                    }
                });
            }
        });
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    fn launch_map(
        &self,
        _ui: slint::Weak<AppWindow>,
        _request: MapLaunchRequest,
    ) -> Result<(), String> {
        Err("map tool is supported only on Windows".into())
    }

    #[cfg(target_os = "windows")]
    fn launch_preview(
        &self,
        _ui: slint::Weak<AppWindow>,
        model_path: PathBuf,
        title: String,
        english: bool,
    ) -> Result<(), String> {
        let executable = Self::tool_executable("campus-preview")?;
        let random: u128 = rand::rng().random();
        let pipe = format!(r"\\.\pipe\campus-reconstruction-preview-{random:032x}");
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .map_err(|error| error.to_string())?;
        let child = Command::new(executable)
            .arg(&pipe)
            .arg(&token)
            .spawn()
            .map_err(|error| error.to_string())?;
        self.children
            .lock()
            .map_err(|_| "tool child lock poisoned")?
            .push(child);
        let updates = self.updates.clone();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("preview supervisor runtime");
            let event_updates = updates.clone();
            let result: Result<(), String> = runtime.block_on(async move {
                let mut server = server;
                server.connect().await.map_err(|error| error.to_string())?;
                let hello: ToolCommand = read_message(&mut server).await?;
                match hello {
                    ToolCommand::Hello {
                        protocol_version,
                        session_token,
                        tool: ToolKind::Preview,
                    } if protocol_version == PROTOCOL_VERSION && session_token == token => {}
                    _ => return Err("preview tool handshake rejected".into()),
                }
                write_message(
                    &mut server,
                    &ToolCommand::OpenPreview {
                        model_path: model_path.to_string_lossy().into_owned(),
                        title,
                        english,
                    },
                )
                .await?;
                loop {
                    let event: ToolEvent = read_message(&mut server).await?;
                    let update = match event {
                        ToolEvent::Ready { .. } => ToolUpdate::Status(
                            if english {
                                "Native 3D preview connected"
                            } else {
                                "原生 3D 预览已连接"
                            }
                            .into(),
                        ),
                        ToolEvent::PreviewBlockSelected { x, y, z, block } => {
                            ToolUpdate::PreviewBlockSelected { x, y, z, block }
                        }
                        ToolEvent::Error { message } => ToolUpdate::Status(if english {
                            format!("Preview error: {message}")
                        } else {
                            format!("预览错误：{message}")
                        }),
                        ToolEvent::Closed { .. } => ToolUpdate::Status(
                            if english {
                                "Native preview closed"
                            } else {
                                "原生预览已关闭"
                            }
                            .into(),
                        ),
                        _ => continue,
                    };
                    let _ = event_updates.send(update);
                }
            });
            if let Err(error) = result {
                let _ = updates.send(ToolUpdate::Status(if english {
                    format!("Preview connection failed: {error}")
                } else {
                    format!("预览连接失败：{error}")
                }));
            }
        });
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    fn launch_preview(
        &self,
        _ui: slint::Weak<AppWindow>,
        _model_path: PathBuf,
        _title: String,
        _english: bool,
    ) -> Result<(), String> {
        Err("preview tool is supported only on Windows".into())
    }
}

impl Drop for ToolSupervisor {
    fn drop(&mut self) {
        if Arc::strong_count(&self.children) != 1 {
            return;
        }
        if let Ok(mut children) = self.children.lock() {
            for child in children.iter_mut() {
                let _ = child.kill();
            }
        }
    }
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
            let model =
                campus_export::foundation_model(recovered.project.as_ref().expect("project"))?;
            if !model.blocks.iter().any(|block| *block != 0) {
                return Err(format!("Foundation preset {:?} produced no blocks", preset));
            }
        }
        let foundation_model =
            campus_export::foundation_model(recovered.project.as_ref().expect("project"))?;
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
    let production_acquisition_client = match v11_acquisition_client::production_client_if_configured(
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
    if let Err(error) = v11_acquisition_client::bootstrap_fixture_if_enabled(
        cfg!(debug_assertions),
        std::env::var("CAMPUS_V11_ACQUISITION_FIXTURE")
            .ok()
            .as_deref(),
    ) {
        eprintln!("V1.1 development acquisition fixture failed: {error}");
    }
    if let Err(error) = v11_project_kernel::bootstrap_if_enabled(
        &app_data_dir(),
        cfg!(debug_assertions),
        std::env::var("CAMPUS_V11_PROJECT_KERNEL").ok().as_deref(),
    ) {
        eprintln!("V1.1 development project kernel failed: {error}");
    }
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
    let map_credentials = Rc::new(RefCell::new(load_map_credentials()));
    ui.set_gaode_key(map_credentials.borrow().js_api_key.clone().into());
    ui.set_gaode_security(map_credentials.borrow().security_code.clone().into());
    let (tool_update_tx, tool_update_rx) = mpsc::channel();
    let tools = ToolSupervisor {
        children: Arc::new(Mutex::new(Vec::new())),
        updates: tool_update_tx,
    };
    let tool_update_rx = Rc::new(RefCell::new(tool_update_rx));
    if default_project_path().exists() {
        let _ = state.borrow_mut().open(default_project_path());
    }
    if state.borrow().project.is_none() {
        state
            .borrow_mut()
            .new_project("未命名项目", "华东师范大学普陀校区");
    }
    sync_ui(&ui, &state.borrow());

    let tool_timer = Timer::default();
    {
        let state = state.clone();
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
                sync_ui(&ui, &state.borrow());
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
                        set_status(
                            &ui,
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
                        set_status(
                            &ui,
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
                        set_status(
                            &ui,
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
                        set_status(
                            &ui,
                            format!("来源冲突审核失败：{error}"),
                            format!("Source conflict review failed: {error}"),
                        );
                    }
                }
            }
        });
    }
    {
        let map_credentials = map_credentials.clone();
        let weak = ui.as_weak();
        ui.on_save_map_settings(move |key, security| {
            let updated = MapCredentials {
                js_api_key: key.to_string(),
                security_code: security.to_string(),
            };
            if let Some(ui) = weak.upgrade() {
                match save_map_credentials(&updated) {
                    Ok(()) => {
                        *map_credentials.borrow_mut() = updated;
                        set_status(&ui, "地图密钥已安全保存", "Map credentials saved securely");
                    }
                    Err(error) => {
                        set_status(
                            &ui,
                            format!("地图密钥保存失败：{error}"),
                            format!("Failed to save map credentials: {error}"),
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
        tool_timer.start(
            TimerMode::Repeated,
            std::time::Duration::from_millis(120),
            move || {
                let mut changed = false;
                while let Ok(update) = receiver.borrow_mut().try_recv() {
                    match update {
                        ToolUpdate::Status(message) => {
                            state.borrow_mut().tool_status = Some(message.clone());
                            if let Some(ui) = weak.upgrade() {
                                ui.set_tool_status(message.into());
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
                            state.borrow_mut().mutate_project(|project| {
                                project.campus_name = name.clone();
                                project.map_view.center = target.gcj02;
                                project.campus_target = Some(target);
                            });
                            if let Some(ui) = weak.upgrade() {
                                set_status(
                                    &ui,
                                    format!("高德校园目标已确认：{name}"),
                                    format!("Gaode campus target confirmed: {name}"),
                                );
                            }
                            changed = true;
                        }
                        ToolUpdate::MapBoundary(points) => {
                            state.borrow_mut().mutate_project(|project| {
                                project.boundary = points;
                            });
                            changed = true;
                        }
                        ToolUpdate::ManualFeature { kind, points } => {
                            let mut result = Ok(String::new());
                            state.borrow_mut().mutate_project(|project| {
                                result = project.add_manual_feature(kind, points);
                            });
                            match result {
                                Ok(id) => {
                                    if let Some(ui) = weak.upgrade() {
                                        ui.set_tool_status(
                                            format!("手绘地物已加入审核结果 · {id}").into(),
                                        );
                                    }
                                    changed = true;
                                }
                                Err(error) => {
                                    if let Some(ui) = weak.upgrade() {
                                        ui.set_tool_status(
                                            format!("手绘地物保存失败：{error}").into(),
                                        );
                                    }
                                }
                            }
                        }
                        ToolUpdate::MapCapture {
                            south_west,
                            north_east,
                            candidates,
                        } => match candidates {
                            Ok(mut discovered) => {
                                let mut count = 0usize;
                                state.borrow_mut().mutate_project(|project| {
                                    project.map_view.capture_bounds =
                                        Some([south_west, north_east]);
                                    if project.boundary.is_empty() {
                                        project.boundary = vec![
                                            south_west,
                                            GeoPoint {
                                                lng: north_east.lng,
                                                lat: south_west.lat,
                                            },
                                            north_east,
                                            GeoPoint {
                                                lng: south_west.lng,
                                                lat: north_east.lat,
                                            },
                                        ];
                                    }
                                    for candidate in &mut discovered {
                                        if let Some(previous) = project
                                            .candidates
                                            .iter()
                                            .find(|previous| previous.id == candidate.id)
                                        {
                                            candidate.review = previous.review;
                                        }
                                    }
                                    discovered.retain(|candidate| {
                                        !project
                                            .building_suppressions
                                            .iter()
                                            .any(|record| record.source_id == candidate.id)
                                    });
                                    count = discovered.len();
                                    project.candidates.retain(|candidate| {
                                        candidate.review != ReviewDecision::Pending
                                            || candidate.source != "OpenStreetMap / Overpass"
                                    });
                                    project.candidates.extend(discovered);
                                });
                                if let Some(ui) = weak.upgrade() {
                                    set_status(
                                        &ui,
                                        format!("识别完成：发现 {count} 个校区候选"),
                                        format!("Discovery complete: {count} campus candidates"),
                                    );
                                }
                                changed = true;
                            }
                            Err(error) => {
                                if let Some(ui) = weak.upgrade() {
                                    set_status(
                                        &ui,
                                        format!("识别失败：{error}"),
                                        format!("Discovery failed: {error}"),
                                    );
                                }
                            }
                        },
                        ToolUpdate::MapVisualCapture {
                            south_west,
                            north_east,
                            png_bytes,
                            candidates,
                        } => match candidates {
                            Ok(mut discovered) => {
                                let capture_path = visual_capture_dir().join("latest.png");
                                let persisted = std::fs::create_dir_all(visual_capture_dir())
                                    .and_then(|_| std::fs::write(&capture_path, png_bytes))
                                    .map(|_| capture_path.clone())
                                    .map_err(|error| error.to_string());
                                match persisted {
                                    Ok(path) => {
                                        let mut count = 0usize;
                                        state.borrow_mut().mutate_project(|project| {
                                            project.map_view.capture_bounds =
                                                Some([south_west, north_east]);
                                            project.visual_capture_path = Some(path);
                                            for candidate in &mut discovered {
                                                if let Some(previous) = project
                                                    .candidates
                                                    .iter()
                                                    .find(|previous| previous.id == candidate.id)
                                                {
                                                    candidate.review = previous.review;
                                                }
                                            }
                                            count = discovered.len();
                                            project.candidates.retain(|candidate| {
                                                candidate.review != ReviewDecision::Pending
                                                    || candidate.source
                                                        != "截图规则分割（确定性 v2）"
                                            });
                                            project.candidates.extend(discovered);
                                        });
                                        if let Some(ui) = weak.upgrade() {
                                            set_status(
                                                &ui,
                                                format!(
                                                    "视觉补缺完成：{count} 个候选，截图已随项目记录"
                                                ),
                                                format!(
                                                    "Visual recovery complete: {count} candidates; screenshot recorded with the project"
                                                ),
                                            );
                                        }
                                        changed = true;
                                    }
                                    Err(error) => {
                                        if let Some(ui) = weak.upgrade() {
                                            set_status(
                                                &ui,
                                                format!("视觉截图保存失败：{error}"),
                                                format!("Failed to save visual capture: {error}"),
                                            );
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                if let Some(ui) = weak.upgrade() {
                                    set_status(
                                        &ui,
                                        format!("视觉补缺失败：{error}"),
                                        format!("Visual recovery failed: {error}"),
                                    );
                                }
                            }
                        },
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
        let weak = ui.as_weak();
        ui.on_undo(move || {
            if state.borrow_mut().undo() {
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
        let weak = ui.as_weak();
        ui.on_redo(move || {
            if state.borrow_mut().redo() {
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
                if let Err(error) = tools.launch_preview(
                    weak.clone(),
                    path,
                    title,
                    state.borrow().locale == DesktopLocale::En,
                ) {
                    if let Some(ui) = weak.upgrade() {
                        set_status(
                            &ui,
                            format!("Foundation 已生成，预览启动失败：{error}"),
                            format!("Foundation generated, but preview failed to start: {error}"),
                        );
                    }
                }
            }
            Err(error) => {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
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
            state.borrow_mut().mutate_project(|project| {
                project.orientation_degrees = orientation.clamp(-180.0, 180.0) as f64;
                project.blocks_per_meter = scale.clamp(0.25, 4.0) as f64;
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                } else {
                    set_status(&ui, "朝向与比例已应用", "Orientation and scale applied");
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
        let weak = ui.as_weak();
        ui.on_save_project(move || {
            let result = autosave(&state);
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
        ui.on_save_project_as(move || {
            let Some(path) = project_file_dialog(true) else {
                return;
            };
            let result = state.borrow_mut().save_to(path);
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
            let mut borrowed = state.borrow_mut();
            borrowed.set_mode(if detailed {
                DesktopMode::Detailed
            } else {
                DesktopMode::Foundation
            });
            if detailed {
                borrowed.mutate_project(|project| {
                    let ids = project
                        .building_slots
                        .iter()
                        .map(|slot| slot.id.clone())
                        .collect::<Vec<_>>();
                    for id in ids {
                        project.discover_external_models_for_slot(&id);
                    }
                });
            }
            drop(borrowed);
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
                ui.window().request_redraw();
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
            {
                let mut borrowed = state.borrow_mut();
                borrowed.candidate_page = 0;
                borrowed.mutate_project(|project| {
                    project.foundation_step = step;
                });
            }
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
        ui.on_confirm_step(move || {
            state
                .borrow_mut()
                .mutate_project(|project| project.confirm_step());
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
        let tools = tools.clone();
        let state = state.clone();
        let map_credentials = map_credentials.clone();
        let weak = ui.as_weak();
        ui.on_open_map(move || {
            let campus_name = state
                .borrow()
                .project
                .as_ref()
                .map(|project| project.campus_name.clone())
                .unwrap_or_else(|| "未命名校区".into());
            let view = state
                .borrow()
                .project
                .as_ref()
                .map(|project| project.map_view.clone())
                .unwrap_or_default();
            let boundary = state
                .borrow()
                .project
                .as_ref()
                .map(|project| project.boundary.clone())
                .unwrap_or_default();
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
            if let Err(error) = tools.launch_map(
                weak.clone(),
                MapLaunchRequest {
                    title: campus_name,
                    view,
                    boundary,
                    js_api_key: credentials.js_api_key,
                    security_code: credentials.security_code,
                    purpose: MapPurpose::CampusReview,
                    overlays: Vec::new(),
                    feature_kind: None,
                    english: state.borrow().locale == DesktopLocale::En,
                },
            ) {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
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
        ui.on_draw_foundation_feature(move || {
            let snapshot = {
                let state = state.borrow();
                state.project.as_ref().and_then(|project| {
                    let (kind, slug, label) = match project.foundation_step {
                        FoundationStep::Building => (FeatureKind::Building, "building", "建筑"),
                        FoundationStep::Road => (FeatureKind::Road, "road", "道路"),
                        FoundationStep::Water => (FeatureKind::Water, "water", "水域"),
                        FoundationStep::Vegetation => {
                            (FeatureKind::Vegetation, "vegetation", "植被")
                        }
                        FoundationStep::Sports => (FeatureKind::Sports, "sports", "体育设施"),
                        _ => return None,
                    };
                    let overlays = if project.boundary.len() >= 3 {
                        vec![MapOverlay {
                            label: "校区边界".into(),
                            points: project
                                .boundary
                                .iter()
                                .map(|point| MapCoordinate {
                                    lng: point.lng,
                                    lat: point.lat,
                                })
                                .collect(),
                        }]
                    } else {
                        Vec::new()
                    };
                    Some((
                        kind,
                        slug.to_string(),
                        label.to_string(),
                        project.campus_name.clone(),
                        project.map_view.clone(),
                        overlays,
                    ))
                })
            };
            let Some((_kind, slug, label, campus_name, view, overlays)) = snapshot else {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
                        "当前步骤不支持手绘地物",
                        "The current step does not support manual drawing",
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
                title: format!("{campus_name} · 手绘{label}"),
                view,
                boundary: Vec::new(),
                js_api_key: credentials.js_api_key,
                security_code: credentials.security_code,
                purpose: MapPurpose::FoundationFeatureDrawing,
                overlays,
                feature_kind: Some(slug),
                english: state.borrow().locale == DesktopLocale::En,
            };
            if let Err(error) = tools.launch_map(weak.clone(), request) {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
                        format!("手绘地图启动失败：{error}"),
                        format!("Drawing map failed to start: {error}"),
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
            if let Err(error) = tools.launch_map(weak.clone(), request) {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
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
            if let Err(error) = tools.launch_preview(
                weak.clone(),
                model,
                title,
                state.borrow().locale == DesktopLocale::En,
            ) {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
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
                if let Err(error) = tools.launch_preview(
                    weak.clone(),
                    path,
                    title,
                    state.borrow().locale == DesktopLocale::En,
                ) {
                    if let Some(ui) = weak.upgrade() {
                        set_status(
                            &ui,
                            format!("生成成功，预览启动失败：{error}"),
                            format!("Generation succeeded, but preview failed to start: {error}"),
                        );
                    }
                }
            }
            Err(error) => {
                if let Some(ui) = weak.upgrade() {
                    set_status(
                        &ui,
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
                        set_status(
                            &ui,
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
                    Err(error) => set_status(
                        &ui,
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
                        set_status(
                            &ui,
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
                        set_status(
                            &ui,
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
                let model = campus_export::foundation_model(&project)?;
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
                        set_status(
                            &ui,
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
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-data/v1-demo.campus.json");
        let mut loaded = DesktopApplicationState::default();
        loaded.open(fixture).unwrap();
        let state = Rc::new(RefCell::new(loaded));
        let (path, title) = generate_detailed_model(&state).unwrap();
        let generated: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(title, "验收图书馆");
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
}
