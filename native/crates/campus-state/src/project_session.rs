use crate::{
    CampusReconstructionWorkflow, DesktopApplicationState, FoundationWorkflow,
    FoundationWorkflowIntent, LocalEvidenceAsset, ReconstructionWorkflowIntent,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::path::{Path, PathBuf};

impl DesktopApplicationState {
    pub fn apply_reconstruction_intent(
        &mut self,
        intent: ReconstructionWorkflowIntent,
    ) -> Result<(), String> {
        self.transact_project(|project| {
            CampusReconstructionWorkflow::apply(project, intent).map_err(|error| error.to_string())
        })
    }

    pub fn apply_foundation_intent(
        &mut self,
        intent: FoundationWorkflowIntent,
    ) -> Result<(), String> {
        self.transact_project(|project| FoundationWorkflow::apply(project, intent))
    }

    pub fn import_local_evidence_files(&mut self, files: &[PathBuf]) -> Result<usize, String> {
        if files.is_empty() {
            return Ok(0);
        }
        let project_path = self
            .project_path
            .clone()
            .ok_or("请先保存 Campus Reconstruction Project，再导入照片证据")?;
        let project_directory = project_path.parent().ok_or("项目路径没有父目录")?;
        let slot_id = self
            .project
            .as_ref()
            .and_then(|project| project.detailed.selected_slot_id.clone())
            .or_else(|| {
                self.project
                    .as_ref()
                    .and_then(|project| project.building_slots.first().map(|slot| slot.id.clone()))
            })
            .ok_or("请先选择 Reviewed Building Slot")?;
        let safe_slot = safe_path_segment(&slot_id);
        let timestamp = now_unix_ms();
        let mut prepared = Vec::with_capacity(files.len());
        for (index, source) in files.iter().enumerate() {
            if !source.is_file() {
                return Err(format!("照片不存在：{}", source.display()));
            }
            let source_name = source
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| format!("照片文件名无效：{}", source.display()))?;
            let safe_name = safe_file_name(source_name);
            let relative = PathBuf::from("evidence")
                .join(&safe_slot)
                .join(format!("{timestamp}-{index}-{safe_name}"));
            let content_base64 =
                STANDARD.encode(std::fs::read(source).map_err(|error| error.to_string())?);
            prepared.push((
                source.clone(),
                source_name.to_string(),
                relative,
                content_base64,
            ));
        }

        for (source, _, relative, _) in &prepared {
            let destination = project_directory.join(relative);
            let parent = destination.parent().ok_or("照片目标路径无效")?;
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            std::fs::copy(source, destination).map_err(|error| error.to_string())?;
        }

        let assets = prepared
            .iter()
            .map(
                |(_, source_name, relative, content_base64)| LocalEvidenceAsset {
                    id: format!("{slot_id}:evidence:{}", relative.to_string_lossy()),
                    slot_id: slot_id.clone(),
                    relative_path: normalize_relative_path(relative),
                    source_name: source_name.clone(),
                    added_at_unix_ms: timestamp,
                    content_base64: content_base64.clone(),
                },
            )
            .collect::<Vec<_>>();
        let count = assets.len();
        self.transact_project(|project| {
            project.detailed.selected_slot_id = Some(slot_id);
            project.detailed.evidence_assets.extend(assets);
            Ok(())
        })?;
        Ok(count)
    }

    pub fn save_as_portable(&mut self, destination: impl AsRef<Path>) -> Result<(), String> {
        let destination = destination.as_ref();
        self.copy_portable_evidence(destination)?;
        self.save_to(destination)
    }

    fn transact_project<T>(
        &mut self,
        mutation: impl FnOnce(&mut crate::CampusProject) -> Result<T, String>,
    ) -> Result<T, String> {
        let previous = self.project.clone().ok_or("No project is open")?;
        let mut next = previous.clone();
        let output = mutation(&mut next)?;
        self.undo_stack.push(previous);
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.project = Some(next);
        self.dirty = true;
        Ok(output)
    }

    fn copy_portable_evidence(&self, destination: &Path) -> Result<(), String> {
        let Some(source_project_path) = self.project_path.as_ref() else {
            return Ok(());
        };
        let source_directory = source_project_path.parent().ok_or("项目路径没有父目录")?;
        let destination_directory = destination.parent().ok_or("新项目路径没有父目录")?;
        if source_directory == destination_directory {
            return Ok(());
        }
        let project = self.project.as_ref().ok_or("No project is open")?;
        for asset in &project.detailed.evidence_assets {
            let relative = Path::new(&asset.relative_path);
            if relative.is_absolute() {
                return Err("项目照片证据包含不可移植的绝对路径".into());
            }
            let source = source_directory.join(relative);
            if !source.is_file() {
                return Err(format!("项目照片证据缺失：{}", source.display()));
            }
            let target = destination_directory.join(relative);
            let parent = target.parent().ok_or("照片目标路径无效")?;
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            std::fs::copy(source, target).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn safe_path_segment(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "slot".into()
    } else {
        value
    }
}

fn safe_file_name(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_' | '.')
                || ('\u{4e00}'..='\u{9fff}').contains(&character)
            {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "photo".into()
    } else {
        value
    }
}

fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
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
    use crate::{BuildingSlot, FoundationPhase};

    #[test]
    fn rejected_foundation_intent_is_atomic() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("project.campus.json");
        let mut state = DesktopApplicationState::default();
        state.new_project("test", "campus");
        state.save_to(&path).unwrap();

        let before = state.project.clone();
        let result = state.apply_foundation_intent(FoundationWorkflowIntent::EnterPhase(
            FoundationPhase::Review,
        ));

        assert!(result.is_err());
        assert_eq!(state.project, before);
        assert!(!state.can_undo());
        assert!(!state.dirty);
    }

    #[test]
    fn evidence_import_and_save_as_are_portable_project_operations() {
        let directory = tempfile::tempdir().unwrap();
        let source_project = directory.path().join("source/project.campus.json");
        let target_project = directory.path().join("target/project.campus.json");
        let photo = directory.path().join("front photo.jpg");
        std::fs::write(&photo, b"photo").unwrap();

        let mut state = DesktopApplicationState::default();
        state.new_project("test", "campus");
        state.mutate_project(|project| {
            project.building_slots.push(BuildingSlot {
                id: "library".into(),
                name: "Library".into(),
                footprint: Vec::new(),
                height_m: None,
                floors: None,
                roof_shape: None,
                refined: false,
            });
            project.detailed.selected_slot_id = Some("library".into());
        });
        state.save_to(&source_project).unwrap();

        assert_eq!(state.import_local_evidence_files(&[photo]).unwrap(), 1);
        let relative = state.project.as_ref().unwrap().detailed.evidence_assets[0]
            .relative_path
            .clone();
        assert!(source_project.parent().unwrap().join(&relative).is_file());

        state.save_as_portable(&target_project).unwrap();
        assert!(target_project.parent().unwrap().join(relative).is_file());
    }
}
