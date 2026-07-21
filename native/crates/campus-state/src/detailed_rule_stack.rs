use crate::{
    ArnisStylePreset, CampusProject, DetailedRuleSource, DetailedRuleStatus, FacadeRuleKind,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledDetailedBuildingRules {
    pub slot_id: String,
    pub template_id: Option<String>,
    pub style_preset: ArnisStylePreset,
    pub floors: Option<u32>,
    pub roof_shape: Option<String>,
    pub window_density: u8,
    pub wall_depth: u8,
    pub wall_material: Option<String>,
    pub accent_material: Option<String>,
    pub applied_rule_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub template_provisional: bool,
}

pub struct DetailedBuildingRuleStack;

impl DetailedBuildingRuleStack {
    pub fn compile(
        project: &CampusProject,
        slot_id: &str,
    ) -> Result<CompiledDetailedBuildingRules, String> {
        let slot = project
            .building_slots
            .iter()
            .find(|slot| slot.id == slot_id)
            .ok_or("Reviewed Building Slot 不存在")?;
        let selected_template = project
            .detailed
            .selected_templates
            .iter()
            .find(|selection| selection.slot_id == slot_id);
        let style_preset = selected_template
            .map(|selection| selection.template.arnis_style)
            .unwrap_or(project.detailed.style_preset);
        let mut winners =
            BTreeMap::<FacadeRuleKind, (u8, usize, usize, &crate::EditableFacadeRule)>::new();
        for (draft_index, draft) in project.detailed.facade_drafts.iter().enumerate() {
            if draft.slot_id != slot_id {
                continue;
            }
            for (rule_index, rule) in draft.rules.iter().enumerate() {
                if rule.slot_id != slot_id || rule.status != DetailedRuleStatus::Accepted {
                    continue;
                }
                let priority = source_priority(rule.source);
                let key = (priority, draft_index, rule_index);
                let replace = winners
                    .get(&rule.kind)
                    .is_none_or(|winner| key > (winner.0, winner.1, winner.2));
                if replace {
                    winners.insert(rule.kind, (priority, draft_index, rule_index, rule));
                }
            }
        }

        let mut compiled = CompiledDetailedBuildingRules {
            slot_id: slot_id.to_string(),
            template_id: selected_template.map(|selection| selection.template.id.clone()),
            style_preset,
            floors: slot.floors,
            roof_shape: slot.roof_shape.clone(),
            window_density: project.detailed.window_density,
            wall_depth: project.detailed.wall_depth,
            wall_material: project.detailed.wall_block.clone(),
            accent_material: None,
            applied_rule_ids: Vec::new(),
            evidence_ids: Vec::new(),
            template_provisional: true,
        };
        for (_, _, _, rule) in winners.values() {
            apply_rule(&mut compiled, rule)?;
            compiled.applied_rule_ids.push(rule.id.clone());
            for evidence_id in &rule.evidence_ids {
                if !compiled.evidence_ids.contains(evidence_id) {
                    compiled.evidence_ids.push(evidence_id.clone());
                }
            }
            if matches!(
                rule.source,
                DetailedRuleSource::PhotoOverride | DetailedRuleSource::ManualOverride
            ) {
                compiled.template_provisional = false;
            }
        }
        Ok(compiled)
    }
}

fn source_priority(source: DetailedRuleSource) -> u8 {
    match source {
        DetailedRuleSource::Template => 0,
        DetailedRuleSource::AutomatedDraft => 1,
        DetailedRuleSource::PhotoOverride => 2,
        DetailedRuleSource::ManualOverride => 3,
    }
}

fn apply_rule(
    compiled: &mut CompiledDetailedBuildingRules,
    rule: &crate::EditableFacadeRule,
) -> Result<(), String> {
    match rule.kind {
        FacadeRuleKind::FloorRhythm => {
            if let Some(value) = first_number(&rule.value) {
                compiled.floors = Some(value.max(1) as u32);
            }
        }
        FacadeRuleKind::WindowPattern => {
            if let Some(value) = first_number(&rule.value) {
                compiled.window_density = value.clamp(0, 100) as u8;
            }
        }
        FacadeRuleKind::BayRhythm => {
            if rule.value.to_ascii_lowercase().contains("depth") {
                if let Some(value) = first_number(&rule.value) {
                    compiled.wall_depth = value.clamp(0, 100) as u8;
                }
            }
        }
        FacadeRuleKind::Roof => {
            let value = rule.value.trim();
            if !value.is_empty() && value != "template-default" {
                compiled.roof_shape = Some(value.to_string());
            }
        }
        FacadeRuleKind::WallMaterial => {
            compiled.wall_material = Some(normalize_block(&rule.value)?);
        }
        FacadeRuleKind::AccentMaterial => {
            compiled.accent_material = Some(normalize_block(&rule.value)?);
        }
        FacadeRuleKind::Entrance | FacadeRuleKind::Cornice => {}
    }
    Ok(())
}

fn first_number(value: &str) -> Option<i32> {
    value
        .split(|character: char| !character.is_ascii_digit() && character != '-')
        .find(|part| !part.is_empty() && *part != "-")
        .and_then(|part| part.parse().ok())
}

fn normalize_block(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '_' | '-')
        })
    {
        return Err("立面规则包含无效 Minecraft 方块标识".into());
    }
    Ok(if value.contains(':') {
        value
    } else {
        format!("minecraft:{value}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildingSlot, DetailedRuleStatus, EditableFacadeRule, FacadeReconstructionDraft};

    fn project() -> CampusProject {
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
        project
    }

    fn rule(
        id: &str,
        source: DetailedRuleSource,
        status: DetailedRuleStatus,
        kind: FacadeRuleKind,
        value: &str,
    ) -> EditableFacadeRule {
        EditableFacadeRule {
            id: id.into(),
            slot_id: "library".into(),
            kind,
            value: value.into(),
            source,
            status,
            confidence: 90,
            evidence_ids: Vec::new(),
        }
    }

    #[test]
    fn accepted_manual_rules_override_photo_automation_and_template_rules() {
        let mut project = project();
        project
            .detailed
            .facade_drafts
            .push(FacadeReconstructionDraft {
                id: "rules".into(),
                slot_id: "library".into(),
                model_version: "test".into(),
                confidence: 90,
                rules: vec![
                    rule(
                        "template",
                        DetailedRuleSource::Template,
                        DetailedRuleStatus::Accepted,
                        FacadeRuleKind::WindowPattern,
                        "density:20",
                    ),
                    rule(
                        "photo-proposed",
                        DetailedRuleSource::PhotoOverride,
                        DetailedRuleStatus::Proposed,
                        FacadeRuleKind::WindowPattern,
                        "density:70",
                    ),
                    rule(
                        "manual",
                        DetailedRuleSource::ManualOverride,
                        DetailedRuleStatus::Accepted,
                        FacadeRuleKind::WindowPattern,
                        "density:88",
                    ),
                ],
                evidence_ids: Vec::new(),
            });

        let compiled = DetailedBuildingRuleStack::compile(&project, "library").unwrap();
        assert_eq!(compiled.window_density, 88);
        assert_eq!(compiled.applied_rule_ids, vec!["manual"]);
        assert!(!compiled.template_provisional);
    }

    #[test]
    fn proposed_or_rejected_rules_never_change_generation_inputs() {
        let mut project = project();
        project.detailed.window_density = 42;
        project
            .detailed
            .facade_drafts
            .push(FacadeReconstructionDraft {
                id: "rules".into(),
                slot_id: "library".into(),
                model_version: "test".into(),
                confidence: 90,
                rules: vec![rule(
                    "proposal",
                    DetailedRuleSource::PhotoOverride,
                    DetailedRuleStatus::Proposed,
                    FacadeRuleKind::WindowPattern,
                    "density:99",
                )],
                evidence_ids: Vec::new(),
            });
        let compiled = DetailedBuildingRuleStack::compile(&project, "library").unwrap();
        assert_eq!(compiled.window_density, 42);
        assert!(compiled.applied_rule_ids.is_empty());
    }
}
