use campus_state::{
    CampusProjectLibrary, CampusScope, CampusTargetMatchApproval, FoundationCategory,
    FoundationResumePoint, InstallationId, PortableDestination, ProjectId, ProjectSaveStatus,
    Schema2Project, Schema2ProjectSession, V11ConstructionCapability,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(test)]
use campus_state::SaveFaultPoint;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRowSaveState {
    Saved,
    RecoveryAvailable,
    SaveFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectOpenDestination {
    Resume(FoundationResumePoint),
    CompletionAndExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherStep {
    CampusTarget,
    ProjectLibrary,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLibraryRow {
    pub project_id: ProjectId,
    pub project_name: String,
    pub latest_successful_save_unix_ms: u64,
    pub save_state: ProjectRowSaveState,
    pub completed_tasks: u8,
    pub total_tasks: u8,
    pub next_incomplete_task: String,
    pub minecraft_compatibility: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LauncherPreferences {
    last_used_campus: CampusScope,
}

pub struct CampusProjectLauncher {
    application_data: PathBuf,
    capability: V11ConstructionCapability,
    actor: InstallationId,
    selected_campus: Option<CampusScope>,
    confirmed_campus: Option<CampusScope>,
    library: Option<CampusProjectLibrary>,
    session: Schema2ProjectSession,
    step: LauncherStep,
    return_step: LauncherStep,
}

impl CampusProjectLauncher {
    pub fn open(
        application_data: impl AsRef<Path>,
        capability: V11ConstructionCapability,
        actor: InstallationId,
    ) -> Result<Self, String> {
        let application_data = application_data.as_ref().to_path_buf();
        let selected_campus = read_last_used_campus(&application_data)?;
        Ok(Self {
            application_data,
            capability,
            actor,
            selected_campus,
            confirmed_campus: None,
            library: None,
            session: Schema2ProjectSession::default(),
            step: LauncherStep::CampusTarget,
            return_step: LauncherStep::CampusTarget,
        })
    }

    pub fn select_campus_candidate(&mut self, scope: CampusScope) {
        self.selected_campus = Some(scope);
    }

    pub fn step(&self) -> LauncherStep {
        self.step
    }

    pub fn begin_campus_selection(&mut self) {
        self.return_step = self.step;
        self.step = LauncherStep::CampusTarget;
    }

    pub fn show_project_library(&mut self) {
        self.step = LauncherStep::ProjectLibrary;
    }

    pub fn confirm_selected_campus(&mut self) -> Result<(), String> {
        let scope = self
            .selected_campus
            .clone()
            .ok_or("Select a Campus Target before confirmation")?;
        if let Some(library) = self.library.as_mut() {
            if let Err(error) = self.session.prepare_context_change(library) {
                self.step = self.return_step;
                return Err(error);
            }
        }
        let library = match CampusProjectLibrary::open_for_construction(
            self.library_root(scope.target_id()),
            scope.target_id(),
            &self.capability,
        ) {
            Ok(library) => library,
            Err(error) => {
                self.step = self.return_step;
                return Err(error);
            }
        };
        if let Err(error) = persist_last_used_campus(&self.application_data, &scope) {
            self.step = self.return_step;
            return Err(error);
        }
        self.confirmed_campus = Some(scope);
        self.library = Some(library);
        self.step = LauncherStep::ProjectLibrary;
        Ok(())
    }
    pub fn confirmed_campus(&self) -> Option<&CampusScope> {
        self.confirmed_campus.as_ref()
    }

    pub fn offered_campus(&self) -> Option<&CampusScope> {
        self.selected_campus.as_ref()
    }

    pub fn active_project_id(&self) -> Option<&ProjectId> {
        self.session.active().map(Schema2Project::id)
    }
    pub fn create_project(&mut self, name: impl Into<String>) -> Result<ProjectId, String> {
        let scope = self
            .confirmed_campus
            .clone()
            .ok_or("Confirm a Campus Target before creating a project")?;
        let library = self
            .library
            .as_mut()
            .ok_or("Confirmed Campus Target has no project library")?;
        self.session.prepare_context_change(library)?;
        let project = library.create_project(scope, name, self.actor.clone())?;
        let project_id = project.id().clone();
        self.session.open_project(library, &project_id)?;
        self.step = LauncherStep::Workspace;
        Ok(project_id)
    }

    pub fn apply_active_operation<T>(
        &mut self,
        description: impl Into<String>,
        mutation: impl FnOnce(&mut Schema2Project) -> Result<T, String>,
    ) -> Result<T, String> {
        let library = self
            .library
            .as_mut()
            .ok_or("Confirm a Campus Target before editing a project")?;
        self.session
            .apply_semantic_operation(library, description, mutation)
    }

    #[cfg(test)]
    fn inject_next_save_failure(&mut self, point: SaveFaultPoint) {
        self.library
            .as_mut()
            .expect("test launcher has a confirmed campus")
            .inject_next_save_failure(point);
    }

    pub fn export_project(
        &mut self,
        project_id: &ProjectId,
        destination: impl AsRef<Path>,
        replace_existing: bool,
    ) -> Result<(), String> {
        let library = self
            .library
            .as_mut()
            .ok_or("Confirm a Campus Target before exporting a project")?;
        self.session.prepare_context_change(library)?;
        let project = library.open_project(project_id)?;
        library.export_portable_project(
            &project,
            destination,
            if replace_existing {
                PortableDestination::ReplaceConfirmed
            } else {
                PortableDestination::CreateNew
            },
        )
    }

    pub fn import_portable_project(
        &mut self,
        source: impl AsRef<Path>,
        approve_campus_switch: bool,
    ) -> Result<ProjectOpenDestination, String> {
        let source = source.as_ref();
        let scope = CampusProjectLibrary::portable_project_scope(source)?;
        let crosses_campus = self
            .confirmed_campus
            .as_ref()
            .is_some_and(|current| current.target_id() != scope.target_id());
        if crosses_campus && !approve_campus_switch {
            return Err("Cross-campus Portable Project import requires confirmation".into());
        }

        let same_campus = self
            .confirmed_campus
            .as_ref()
            .is_some_and(|current| current.target_id() == scope.target_id());
        let imported = if same_campus {
            let library = self
                .library
                .as_mut()
                .ok_or("Confirmed Campus Target has no project library")?;
            self.session.import_portable_into_current_campus(
                library,
                source,
                scope.clone(),
                CampusTargetMatchApproval::HumanConfirmed,
                self.actor.clone(),
            )?
        } else {
            let mut destination = CampusProjectLibrary::open_for_construction(
                self.library_root(scope.target_id()),
                scope.target_id(),
                &self.capability,
            )?;
            let imported = if let Some(current) = self.library.as_mut() {
                self.session.import_portable_across_campus(
                    current,
                    &mut destination,
                    source,
                    scope.clone(),
                    CampusTargetMatchApproval::HumanConfirmed,
                    self.actor.clone(),
                )?
            } else {
                let imported = destination.import_portable_project(
                    source,
                    scope.clone(),
                    CampusTargetMatchApproval::HumanConfirmed,
                    self.actor.clone(),
                )?;
                self.session.open_project(&destination, imported.id())?;
                imported
            };
            self.library = Some(destination);
            imported
        };

        persist_last_used_campus(&self.application_data, &scope)?;
        self.selected_campus = Some(scope.clone());
        self.confirmed_campus = Some(scope);
        self.step = LauncherStep::Workspace;
        Ok(open_destination(imported.resume_point()))
    }

    pub fn open_project(
        &mut self,
        project_id: &ProjectId,
    ) -> Result<ProjectOpenDestination, String> {
        let library = self
            .library
            .as_mut()
            .ok_or("Confirm a Campus Target before opening a project")?;
        self.session.switch_project(library, project_id)?;
        let resume = self
            .session
            .active()
            .ok_or("Opened project did not become active")?
            .resume_point();
        self.step = LauncherStep::Workspace;
        Ok(open_destination(resume))
    }

    pub fn rows(&self) -> Result<Vec<ProjectLibraryRow>, String> {
        let library = self
            .library
            .as_ref()
            .ok_or("Confirm a Campus Target before listing projects")?;
        library
            .records()
            .iter()
            .map(|record| {
                let project = library.open_project(record.project_id())?;
                let recovery_available = library.recovery_candidate(record.project_id())?.is_some();
                let is_active = self
                    .session
                    .active()
                    .is_some_and(|active| active.id() == record.project_id());
                let save_state = if is_active {
                    match self.session.save_status() {
                        ProjectSaveStatus::Failed { reason } => {
                            ProjectRowSaveState::SaveFailed(reason.clone())
                        }
                        _ if recovery_available => ProjectRowSaveState::RecoveryAvailable,
                        _ => ProjectRowSaveState::Saved,
                    }
                } else if recovery_available {
                    ProjectRowSaveState::RecoveryAvailable
                } else {
                    ProjectRowSaveState::Saved
                };
                let (completed_tasks, next_incomplete_task) =
                    resume_progress(project.resume_point());
                Ok(ProjectLibraryRow {
                    project_id: record.project_id().clone(),
                    project_name: record.project_name().into(),
                    latest_successful_save_unix_ms: record.latest_successful_save_unix_ms(),
                    save_state,
                    completed_tasks,
                    total_tasks: 9,
                    next_incomplete_task: next_incomplete_task.into(),
                    minecraft_compatibility: format!(
                        "Minecraft {} {}",
                        project.compatibility_profile().edition(),
                        project.compatibility_profile().minecraft_version()
                    ),
                })
            })
            .collect()
    }

    fn library_root(&self, campus_target_id: &str) -> PathBuf {
        let digest = Sha256::digest(campus_target_id.as_bytes());
        self.application_data
            .join("v1.1-project-libraries")
            .join(format!("{digest:x}"))
    }
}

fn preferences_path(application_data: &Path) -> PathBuf {
    application_data.join("v1.1-launcher-preferences.json")
}

fn read_last_used_campus(application_data: &Path) -> Result<Option<CampusScope>, String> {
    let path = preferences_path(application_data);
    if !path.exists() {
        return Ok(None);
    }
    let preferences: LauncherPreferences =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Invalid launcher preferences: {error}"))?;
    Ok(Some(preferences.last_used_campus))
}

fn persist_last_used_campus(application_data: &Path, scope: &CampusScope) -> Result<(), String> {
    fs::create_dir_all(application_data).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(&LauncherPreferences {
        last_used_campus: scope.clone(),
    })
    .map_err(|error| error.to_string())?;
    fs::write(preferences_path(application_data), bytes).map_err(|error| error.to_string())
}

fn open_destination(resume: FoundationResumePoint) -> ProjectOpenDestination {
    if resume == FoundationResumePoint::Complete {
        ProjectOpenDestination::CompletionAndExport
    } else {
        ProjectOpenDestination::Resume(resume)
    }
}

fn resume_progress(resume: FoundationResumePoint) -> (u8, &'static str) {
    match resume {
        FoundationResumePoint::BoundaryReview => (0, "Confirm Campus Boundary"),
        FoundationResumePoint::Acquisition => (1, "Acquire Foundation evidence"),
        FoundationResumePoint::Review(category) => {
            let (completed, label) = match category {
                FoundationCategory::Building => (2, "Review Buildings"),
                FoundationCategory::Circulation => (3, "Review Circulation"),
                FoundationCategory::Water => (4, "Review Water"),
                FoundationCategory::Vegetation => (5, "Review Vegetation"),
                FoundationCategory::Sports => (6, "Review Sports"),
            };
            (completed, label)
        }
        FoundationResumePoint::Generation => (7, "Generate Minecraft result"),
        FoundationResumePoint::Export => (8, "Export .schem and Foundation Manifest"),
        FoundationResumePoint::Complete => (9, "Completion and export"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(id: &str, name: &str, longitude: f64) -> CampusScope {
        CampusScope::new(id, name, [longitude, 31.2]).unwrap()
    }

    fn launcher(root: &Path) -> CampusProjectLauncher {
        CampusProjectLauncher::open(
            root,
            V11ConstructionCapability::request(true, Some("1")).unwrap(),
            InstallationId::new("launcher-test").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn confirmed_campus_shows_only_its_projects_and_names_are_campus_scoped() {
        let directory = tempfile::tempdir().unwrap();
        let mut launcher = launcher(directory.path());
        let putuo = scope("gaode:putuo", "ECNU Putuo", 121.395);
        let minhang = scope("gaode:minhang", "ECNU Minhang", 121.45);

        launcher.select_campus_candidate(putuo.clone());
        launcher.confirm_selected_campus().unwrap();
        launcher.create_project("Main rebuild").unwrap();

        launcher.select_campus_candidate(minhang);
        launcher.confirm_selected_campus().unwrap();
        launcher.create_project("Main rebuild").unwrap();
        assert_eq!(
            launcher
                .rows()
                .unwrap()
                .iter()
                .map(|row| row.project_name.as_str())
                .collect::<Vec<_>>(),
            vec!["Main rebuild"]
        );

        launcher.select_campus_candidate(putuo);
        launcher.confirm_selected_campus().unwrap();
        assert_eq!(launcher.rows().unwrap().len(), 1);
        assert_eq!(launcher.rows().unwrap()[0].project_name, "Main rebuild");
    }

    #[test]
    fn project_rows_expose_resume_and_durability_without_file_identity() {
        let directory = tempfile::tempdir().unwrap();
        let mut launcher = launcher(directory.path());
        launcher.select_campus_candidate(scope("gaode:putuo", "ECNU Putuo", 121.395));
        launcher.confirm_selected_campus().unwrap();
        let project_id = launcher.create_project("Library rebuild").unwrap();

        let row = launcher.rows().unwrap().remove(0);
        assert_eq!(row.project_id, project_id);
        assert_eq!(row.project_name, "Library rebuild");
        assert!(row.latest_successful_save_unix_ms > 0);
        assert_eq!(row.save_state, ProjectRowSaveState::Saved);
        assert_eq!((row.completed_tasks, row.total_tasks), (0, 9));
        assert_eq!(row.next_incomplete_task, "Confirm Campus Boundary");
        assert_eq!(row.minecraft_compatibility, "Minecraft Java Edition 26.1.2");
        assert_eq!(
            launcher.open_project(&project_id).unwrap(),
            ProjectOpenDestination::Resume(FoundationResumePoint::BoundaryReview)
        );
        assert!(!format!("{row:?}").contains("project.campus.json"));
    }

    #[test]
    fn failed_save_cancels_campus_switch_and_preserves_current_rows() {
        let directory = tempfile::tempdir().unwrap();
        let mut launcher = launcher(directory.path());
        let putuo = scope("gaode:putuo", "ECNU Putuo", 121.395);
        launcher.select_campus_candidate(putuo.clone());
        launcher.confirm_selected_campus().unwrap();
        launcher.create_project("Unsaved rebuild").unwrap();

        launcher.inject_next_save_failure(SaveFaultPoint::BeforeStageWrite);
        assert!(launcher
            .apply_active_operation("Change project", |project| {
                project.mark_updated(InstallationId::new("editor").unwrap())
            })
            .is_err());
        launcher.inject_next_save_failure(SaveFaultPoint::BeforeStageWrite);
        launcher.begin_campus_selection();
        launcher.select_campus_candidate(scope("gaode:minhang", "ECNU Minhang", 121.45));

        assert!(launcher.confirm_selected_campus().is_err());
        assert_eq!(launcher.confirmed_campus(), Some(&putuo));
        assert_eq!(launcher.step(), LauncherStep::Workspace);
        let rows = launcher.rows().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_name, "Unsaved rebuild");
        assert!(matches!(
            rows[0].save_state,
            ProjectRowSaveState::SaveFailed(_)
        ));
        drop(launcher);
        let mut reopened = CampusProjectLauncher::open(
            directory.path(),
            V11ConstructionCapability::request(true, Some("1")).unwrap(),
            InstallationId::new("launcher-test").unwrap(),
        )
        .unwrap();
        reopened.confirm_selected_campus().unwrap();
        assert_eq!(
            reopened.rows().unwrap()[0].save_state,
            ProjectRowSaveState::RecoveryAvailable
        );
    }

    #[test]
    fn last_used_campus_is_offered_but_never_silently_confirmed() {
        let directory = tempfile::tempdir().unwrap();
        let putuo = scope("gaode:putuo", "ECNU Putuo", 121.395);
        {
            let mut launcher = launcher(directory.path());
            launcher.select_campus_candidate(putuo.clone());
            launcher.confirm_selected_campus().unwrap();
            launcher.create_project("Resume later").unwrap();
        }

        let mut reopened = launcher(directory.path());
        assert_eq!(reopened.step(), LauncherStep::CampusTarget);
        assert_eq!(reopened.offered_campus(), Some(&putuo));
        assert_eq!(reopened.confirmed_campus(), None);
        assert!(reopened.rows().is_err());

        reopened.confirm_selected_campus().unwrap();
        assert_eq!(reopened.step(), LauncherStep::ProjectLibrary);
        assert_eq!(reopened.confirmed_campus(), Some(&putuo));
        assert_eq!(reopened.rows().unwrap()[0].project_name, "Resume later");
    }

    #[test]
    fn portable_import_requires_cross_campus_confirmation_and_commits_one_scope() {
        let source_root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("putuo-portable.json");
        let putuo = scope("gaode:putuo", "ECNU Putuo", 121.395);
        let minhang = scope("gaode:minhang", "ECNU Minhang", 121.45);

        let mut source = launcher(source_root.path());
        source.select_campus_candidate(putuo.clone());
        source.confirm_selected_campus().unwrap();
        let source_id = source.create_project("Portable rebuild").unwrap();
        source
            .export_project(&source_id, &portable_path, false)
            .unwrap();

        let target_root = tempfile::tempdir().unwrap();
        let mut target = launcher(target_root.path());
        target.select_campus_candidate(minhang.clone());
        target.confirm_selected_campus().unwrap();
        target.create_project("Keep current").unwrap();

        assert!(target
            .import_portable_project(&portable_path, false)
            .is_err());
        assert_eq!(target.confirmed_campus(), Some(&minhang));
        assert_eq!(target.rows().unwrap()[0].project_name, "Keep current");

        assert_eq!(
            target
                .import_portable_project(&portable_path, true)
                .unwrap(),
            ProjectOpenDestination::Resume(FoundationResumePoint::BoundaryReview)
        );
        assert_eq!(target.confirmed_campus(), Some(&putuo));
        let rows = target.rows().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_name, "Portable rebuild");

        let fresh_root = tempfile::tempdir().unwrap();
        let mut fresh = launcher(fresh_root.path());
        fresh
            .import_portable_project(&portable_path, false)
            .unwrap();
        assert_eq!(fresh.confirmed_campus(), Some(&putuo));
        assert_eq!(fresh.rows().unwrap()[0].project_name, "Portable rebuild");
    }
}
