use super::*;
use atomicwrites::DisallowOverwrite;

const PORTABLE_FORMAT_VERSION: u32 = 1;
const PORTABLE_IMPORT_ROLLBACK_FILE: &str = ".portable-import-rollback.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PortableProjectEnvelope {
    portable_format_version: u32,
    exported_at_unix_ms: u64,
    project: Schema2Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortableImportRollback {
    managed_relative_path: String,
    previous_index_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortableDestination {
    CreateNew,
    ReplaceConfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampusTargetMatchApproval {
    AutomaticOnly,
    HumanConfirmed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CampusTargetMatchRequirement {
    AutomaticGaodePoiMatch,
    HumanConfirmationRequired {
        portable_name: String,
        portable_anchor_wgs84: [f64; 2],
        selected_name: String,
        selected_anchor_wgs84: [f64; 2],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PortableImportLineage {
    source_project_id: ProjectId,
    source_schema_version: u32,
    imported_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct ImportedProjectMetadata {
    project_id: ProjectId,
    name: String,
    campus_scope: CampusScope,
    audit: ProjectAudit,
    lineage: Value,
}

impl PortableImportLineage {
    pub fn source_project_id(&self) -> &ProjectId {
        &self.source_project_id
    }

    pub fn source_schema_version(&self) -> u32 {
        self.source_schema_version
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortableTransferFaultPoint {
    ExportAfterStageWrite,
    ExportAfterStageValidation,
    ImportAfterTemporaryCopy,
    ImportAfterMigration,
    ImportAfterProjectWrite,
    ImportAfterIndexWrite,
}

impl CampusProjectLibrary {
    pub fn inject_next_portable_failure(&mut self, point: PortableTransferFaultPoint) {
        self.next_portable_fault = Some(point);
    }

    pub fn export_portable_project(
        &mut self,
        project: &Schema2Project,
        destination: impl AsRef<Path>,
        destination_policy: PortableDestination,
    ) -> Result<(), String> {
        let destination = destination.as_ref();
        ensure_external_portable_destination(&self.root, destination)?;
        if destination.exists() && destination_policy != PortableDestination::ReplaceConfirmed {
            return Err("Replacing an existing Portable Project requires confirmation".into());
        }
        if destination.is_dir() {
            return Err("Portable Project destination must be a file".into());
        }
        validate_schema2_project(project)?;
        let portable_project = portable_project_copy(project)?;
        let envelope = PortableProjectEnvelope {
            portable_format_version: PORTABLE_FORMAT_VERSION,
            exported_at_unix_ms: now_unix_ms(),
            project: portable_project,
        };
        let bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| error.to_string())?;
        let parent = destination
            .parent()
            .ok_or("Portable Project destination has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let stage_path = parent.join(format!(
            ".portable-export-stage-{}.json",
            uuid::Uuid::new_v4()
        ));
        let result = (|| {
            write_and_sync(&stage_path, &bytes)?;
            self.fail_portable_at(PortableTransferFaultPoint::ExportAfterStageWrite)?;
            let staged = decode_portable_envelope(
                &fs::read(&stage_path).map_err(|error| error.to_string())?,
            )?;
            if staged != envelope {
                return Err("Staged Portable Project changed during validation".into());
            }
            self.fail_portable_at(PortableTransferFaultPoint::ExportAfterStageValidation)?;
            match destination_policy {
                PortableDestination::CreateNew => atomic_write_new_bytes(destination, &bytes),
                PortableDestination::ReplaceConfirmed => atomic_write_bytes(destination, &bytes),
            }
        })();
        remove_file_if_present(&stage_path);
        result
    }

    pub fn inspect_portable_project(
        source: impl AsRef<Path>,
        selected_scope: &CampusScope,
    ) -> Result<CampusTargetMatchRequirement, String> {
        let bytes = fs::read(source.as_ref()).map_err(|error| error.to_string())?;
        let embedded_scope = match decode_portable_envelope(&bytes) {
            Ok(envelope) => envelope.project.campus_scope().clone(),
            Err(portable_error) => portable_schema1_scope(&bytes).map_err(|migration_error| {
                format!(
                    "Unsupported Portable Project; schema-2 validation: {portable_error}; schema-1 validation: {migration_error}"
                )
            })?,
        };
        Ok(campus_target_match_requirement(
            &embedded_scope,
            selected_scope,
        ))
    }

    pub fn import_portable_project(
        &mut self,
        source: impl AsRef<Path>,
        selected_scope: CampusScope,
        approval: CampusTargetMatchApproval,
        actor: InstallationId,
    ) -> Result<Schema2Project, String> {
        if selected_scope.target_id() != self.campus_target_id {
            return Err("Selected Campus Target does not match the destination library".into());
        }
        let source = source.as_ref();
        if !source.is_file() {
            return Err("Portable Project source must be a readable file".into());
        }
        let source_bytes = fs::read(source).map_err(|error| error.to_string())?;
        let temporary_copy = self.root.join(format!(
            ".portable-import-copy-{}.json",
            uuid::Uuid::new_v4()
        ));
        let result = (|| {
            write_and_sync(&temporary_copy, &source_bytes)?;
            self.fail_portable_at(PortableTransferFaultPoint::ImportAfterTemporaryCopy)?;
            let copied_bytes = fs::read(&temporary_copy).map_err(|error| error.to_string())?;
            if copied_bytes != source_bytes {
                return Err("Temporary Portable Project copy changed before validation".into());
            }
            let (source_project, embedded_scope, source_schema_version) =
                match decode_portable_envelope(&copied_bytes) {
                    Ok(envelope) => {
                        let embedded_scope = envelope.project.campus_scope().clone();
                        let source_schema_version = envelope.project.schema_version();
                        (envelope.project, embedded_scope, source_schema_version)
                    }
                    Err(portable_error) => {
                        let embedded_scope =
                            portable_schema1_scope(&copied_bytes).map_err(|migration_error| {
                                format!(
                                    "Unsupported Portable Project; schema-2 validation: {portable_error}; schema-1 validation: {migration_error}"
                                )
                            })?;
                        let migrated = migrate_portable_schema1_copy(
                            &copied_bytes,
                            selected_scope.clone(),
                            actor.clone(),
                        )?;
                        (migrated, embedded_scope, 1)
                    }
                };
            self.fail_portable_at(PortableTransferFaultPoint::ImportAfterMigration)?;
            let requirement = campus_target_match_requirement(&embedded_scope, &selected_scope);
            if matches!(
                requirement,
                CampusTargetMatchRequirement::HumanConfirmationRequired { .. }
            ) && approval != CampusTargetMatchApproval::HumanConfirmed
            {
                return Err(
                    "Portable Project Campus Target Match requires human confirmation of names and map locations"
                        .into(),
                );
            }
            let candidate = self.prepare_import_candidate(
                source_project,
                source_schema_version,
                selected_scope,
                actor,
            )?;
            self.commit_import_candidate(candidate)
        })();
        remove_file_if_present(&temporary_copy);
        result
    }

    fn fail_portable_at(&mut self, point: PortableTransferFaultPoint) -> Result<(), String> {
        if self.next_portable_fault == Some(point) {
            self.next_portable_fault = None;
            Err(format!("injected portable transfer failure at {point:?}"))
        } else {
            Ok(())
        }
    }

    fn stage_portable_confirmation_copy(&self, source: &Path) -> Result<PathBuf, String> {
        if !source.is_file() {
            return Err("Portable Project source must be a readable file".into());
        }
        let bytes = fs::read(source).map_err(|error| error.to_string())?;
        let staged = self.root.join(format!(
            ".portable-confirmation-copy-{}.json",
            uuid::Uuid::new_v4()
        ));
        atomic_write_new_bytes(&staged, &bytes)?;
        Ok(staged)
    }

    fn prepare_import_candidate(
        &self,
        source: Schema2Project,
        source_schema_version: u32,
        selected_scope: CampusScope,
        actor: InstallationId,
    ) -> Result<Schema2Project, String> {
        let imported_name = self.next_import_name(source.name());
        source.into_imported_copy(imported_name, source_schema_version, selected_scope, actor)
    }

    fn next_import_name(&self, base: &str) -> String {
        if !self.index.projects.iter().any(|record| record.name == base) {
            return base.into();
        }
        let mut sequence = 1_u64;
        loop {
            let candidate = format!("{base}（导入 {sequence}）");
            if !self
                .index
                .projects
                .iter()
                .any(|record| record.name == candidate)
            {
                return candidate;
            }
            sequence = sequence.saturating_add(1);
        }
    }

    fn commit_import_candidate(
        &mut self,
        candidate: Schema2Project,
    ) -> Result<Schema2Project, String> {
        let relative = format!("projects/{}/{PROJECT_FILE_NAME}", candidate.id().as_str());
        let project_path = self.root.join(&relative);
        if project_path.exists() {
            return Err("Portable Project import collided with an existing managed path".into());
        }
        let project_directory = project_path
            .parent()
            .ok_or("Imported managed project path has no parent")?
            .to_path_buf();
        let index_path = self.root.join(LIBRARY_INDEX_FILE);
        let rollback_path = self.root.join(PORTABLE_IMPORT_ROLLBACK_FILE);
        if rollback_path.exists() {
            return Err("A previous Portable Project import requires recovery".into());
        }
        let old_index_bytes = read_optional_file(&index_path)?;
        let mut next_index = self.index.clone();
        next_index
            .projects
            .push(record_from_project(&candidate, relative.clone(), None));
        let candidate_bytes =
            serde_json::to_vec_pretty(&candidate).map_err(|error| error.to_string())?;
        atomic_write_json(
            &rollback_path,
            &PortableImportRollback {
                managed_relative_path: relative,
                previous_index_bytes: old_index_bytes.clone(),
            },
        )?;
        let commit = (|| {
            atomic_write_new_bytes(&project_path, &candidate_bytes)?;
            let written = decode_schema2_project(
                &fs::read(&project_path).map_err(|error| error.to_string())?,
            )?;
            if written != candidate {
                return Err("Imported managed project changed during validation".into());
            }
            self.fail_portable_at(PortableTransferFaultPoint::ImportAfterProjectWrite)?;
            atomic_write_json(&index_path, &next_index)?;
            self.fail_portable_at(PortableTransferFaultPoint::ImportAfterIndexWrite)?;
            let reloaded_index = read_portable_library_rows(&self.root, &self.campus_target_id)?;
            fs::remove_file(&rollback_path).map_err(|error| error.to_string())?;
            Ok(reloaded_index)
        })();
        let reloaded_index = match commit {
            Ok(index) => index,
            Err(error) => {
                let rollback = rollback_portable_import(
                    &project_directory,
                    &index_path,
                    old_index_bytes.as_deref(),
                    &rollback_path,
                );
                return match rollback {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(format!(
                        "{error}; failed to roll back Portable Project import: {rollback_error}"
                    )),
                };
            }
        };
        self.index = reloaded_index;
        Ok(candidate)
    }
}

impl Schema2Project {
    pub fn portable_import_lineage(&self) -> Result<Option<PortableImportLineage>, String> {
        self.optional_state
            .get("portableImportLineage")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| format!("Invalid Portable Project import lineage: {error}"))
    }

    fn into_imported_copy(
        mut self,
        imported_name: String,
        source_schema_version: u32,
        selected_scope: CampusScope,
        actor: InstallationId,
    ) -> Result<Self, String> {
        let imported_at_unix_ms = now_unix_ms();
        let imported_audit = ProjectAudit {
            created_at_unix_ms: imported_at_unix_ms,
            created_by: actor.clone(),
            updated_at_unix_ms: imported_at_unix_ms,
            updated_by: actor,
            optional_state: Map::new(),
        };
        let lineage = PortableImportLineage {
            source_project_id: self.id().clone(),
            source_schema_version,
            imported_at_unix_ms,
        };
        let lineage_value = serde_json::to_value(&lineage).map_err(|error| error.to_string())?;
        let metadata = ImportedProjectMetadata {
            project_id: ProjectId::generate(),
            name: imported_name,
            campus_scope: selected_scope,
            audit: imported_audit,
            lineage: lineage_value,
        };

        self.project_id = metadata.project_id.clone();
        self.name = metadata.name.clone();
        self.campus_scope = metadata.campus_scope.clone();
        self.audit = metadata.audit.clone();
        self.optional_state
            .insert("portableImportLineage".into(), metadata.lineage.clone());
        self.durability.last_confirmed_save_unix_ms = Some(imported_at_unix_ms);
        for operation in self
            .durability
            .undo
            .iter_mut()
            .chain(self.durability.redo.iter_mut())
        {
            rewrite_imported_snapshot(&mut operation.before, &metadata)?;
            rewrite_imported_snapshot(&mut operation.after, &metadata)?;
        }
        validate_schema2_project(&self)?;
        Ok(self)
    }
}

impl Schema2ProjectSession {
    pub fn import_portable_into_current_campus(
        &mut self,
        library: &mut CampusProjectLibrary,
        source: impl AsRef<Path>,
        selected_scope: CampusScope,
        approval: CampusTargetMatchApproval,
        actor: InstallationId,
    ) -> Result<Schema2Project, String> {
        let confirmed_copy = library.stage_portable_confirmation_copy(source.as_ref())?;
        let result = (|| {
            confirm_portable_match_for_context_change(&confirmed_copy, &selected_scope, approval)?;
            self.prepare_context_change(library)?;
            let imported = library.import_portable_project(
                &confirmed_copy,
                selected_scope,
                approval,
                actor,
            )?;
            self.activate_imported_project(imported)
        })();
        remove_file_if_present(&confirmed_copy);
        result
    }

    pub fn import_portable_across_campus(
        &mut self,
        current_library: &mut CampusProjectLibrary,
        destination_library: &mut CampusProjectLibrary,
        source: impl AsRef<Path>,
        selected_scope: CampusScope,
        approval: CampusTargetMatchApproval,
        actor: InstallationId,
    ) -> Result<Schema2Project, String> {
        let confirmed_copy =
            destination_library.stage_portable_confirmation_copy(source.as_ref())?;
        let result = (|| {
            confirm_portable_match_for_context_change(&confirmed_copy, &selected_scope, approval)?;
            self.prepare_context_change(current_library)?;
            let imported = destination_library.import_portable_project(
                &confirmed_copy,
                selected_scope,
                approval,
                actor,
            )?;
            self.activate_imported_project(imported)
        })();
        remove_file_if_present(&confirmed_copy);
        result
    }

    fn activate_imported_project(
        &mut self,
        imported: Schema2Project,
    ) -> Result<Schema2Project, String> {
        let saved_at = imported
            .durability
            .last_confirmed_save_unix_ms
            .unwrap_or(imported.audit.updated_at_unix_ms);
        self.active = Some(imported.clone());
        self.save_status = ProjectSaveStatus::Saved {
            completed_at_unix_ms: saved_at,
        };
        self.maintenance_warning = None;
        self.dirty = false;
        Ok(imported)
    }
}

fn confirm_portable_match_for_context_change(
    source: &Path,
    selected_scope: &CampusScope,
    approval: CampusTargetMatchApproval,
) -> Result<(), String> {
    if matches!(
        CampusProjectLibrary::inspect_portable_project(source, selected_scope)?,
        CampusTargetMatchRequirement::HumanConfirmationRequired { .. }
    ) && approval != CampusTargetMatchApproval::HumanConfirmed
    {
        return Err(
            "Portable Project Campus Target Match requires human confirmation before saving the active project"
                .into(),
        );
    }
    Ok(())
}

fn portable_project_copy(project: &Schema2Project) -> Result<Schema2Project, String> {
    let mut value = serde_json::to_value(project).map_err(|error| error.to_string())?;
    strip_non_portable_state(&mut value);
    let portable: Schema2Project =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    validate_schema2_project(&portable)?;
    Ok(portable)
}

pub(super) fn strip_non_portable_state(value: &mut Value) {
    visit_json_objects(value, &mut |object| {
        object.retain(|key, value| !is_non_portable_entry(key, value));
        false
    });
}

fn is_non_portable_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if normalized == "previewprofileid" {
        return false;
    }
    normalized.ends_with("absolutepath")
        || normalized.ends_with("machinepath")
        || normalized.ends_with("previewpath")
        || normalized.ends_with("schematicpath")
        || normalized.contains("credential")
        || normalized.ends_with("apikey")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("clientsecret")
        || normalized.contains("privatekey")
        || normalized.contains("secretaccesskey")
        || normalized.contains("accesstoken")
        || normalized.contains("refreshtoken")
        || normalized.contains("sessiontoken")
        || normalized.contains("bearertoken")
        || normalized.ends_with("accesskey")
        || normalized.contains("authorization")
        || normalized.contains("cookie")
        || normalized.contains("appsettings")
        || normalized.contains("applicationsettings")
        || normalized.contains("cache")
        || normalized.starts_with("logs")
        || normalized.ends_with("logs")
        || normalized.contains("runtimelog")
        || normalized.contains("diagnosticlog")
        || normalized.contains("previewimage")
        || normalized.contains("previewdata")
        || normalized.contains("previewfile")
        || normalized.contains("schematicfile")
        || normalized.contains("schematicdata")
        || matches!(
            normalized.as_str(),
            "credentials"
                | "applicationsettings"
                | "logs"
                | "caches"
                | "previews"
                | "schematics"
                | "apikey"
                | "auth"
                | "secret"
                | "token"
                | "password"
                | "session"
        )
}

fn decode_portable_envelope(bytes: &[u8]) -> Result<PortableProjectEnvelope, String> {
    let mut raw: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("Invalid Portable Project: {error}"))?;
    if contains_non_portable_state(&mut raw) {
        return Err("Portable Project contains credentials, machine paths, logs, caches, previews, or schematics".into());
    }
    let envelope: PortableProjectEnvelope = serde_json::from_value(raw)
        .map_err(|error| format!("Invalid Portable Project: {error}"))?;
    if envelope.portable_format_version != PORTABLE_FORMAT_VERSION {
        return Err(format!(
            "Portable Project format {} is not supported format {}",
            envelope.portable_format_version, PORTABLE_FORMAT_VERSION
        ));
    }
    validate_schema2_project(&envelope.project)?;
    Ok(envelope)
}

fn contains_non_portable_state(value: &mut Value) -> bool {
    visit_json_objects(value, &mut |object| {
        object
            .iter()
            .any(|(key, value)| is_non_portable_entry(key, value))
    })
}

fn visit_json_objects(
    value: &mut Value,
    visitor: &mut impl FnMut(&mut Map<String, Value>) -> bool,
) -> bool {
    match value {
        Value::Object(object) => {
            visitor(object)
                || object
                    .values_mut()
                    .any(|child| visit_json_objects(child, visitor))
        }
        Value::Array(values) => values
            .iter_mut()
            .any(|child| visit_json_objects(child, visitor)),
        _ => false,
    }
}

fn is_non_portable_entry(key: &str, value: &Value) -> bool {
    is_non_portable_key(key)
        || contains_unsafe_path_value(key, value)
        || value
            .as_str()
            .is_some_and(|value| value.to_ascii_lowercase().ends_with(".schem"))
}

fn contains_unsafe_path_value(key: &str, value: &Value) -> bool {
    let Some(value) = value.as_str() else {
        return false;
    };
    let path = Path::new(value);
    path.is_absolute()
        || (key.to_ascii_lowercase().contains("path")
            && path
                .components()
                .any(|component| !matches!(component, Component::Normal(_))))
}

fn campus_target_match_requirement(
    portable: &CampusScope,
    selected: &CampusScope,
) -> CampusTargetMatchRequirement {
    if portable.gaode_poi_id().is_some() && portable.gaode_poi_id() == selected.gaode_poi_id() {
        CampusTargetMatchRequirement::AutomaticGaodePoiMatch
    } else {
        CampusTargetMatchRequirement::HumanConfirmationRequired {
            portable_name: portable.canonical_name().into(),
            portable_anchor_wgs84: portable.anchor_wgs84(),
            selected_name: selected.canonical_name().into(),
            selected_anchor_wgs84: selected.anchor_wgs84(),
        }
    }
}

fn rewrite_imported_snapshot(
    snapshot: &mut Value,
    metadata: &ImportedProjectMetadata,
) -> Result<(), String> {
    let object = snapshot
        .as_object_mut()
        .ok_or("Portable Project history snapshot must be an object")?;
    object.insert(
        "projectId".into(),
        serde_json::to_value(&metadata.project_id).map_err(|error| error.to_string())?,
    );
    object.insert("name".into(), Value::String(metadata.name.clone()));
    object.insert(
        "campusScope".into(),
        serde_json::to_value(&metadata.campus_scope).map_err(|error| error.to_string())?,
    );
    object.insert(
        "audit".into(),
        serde_json::to_value(&metadata.audit).map_err(|error| error.to_string())?,
    );
    object.insert("portableImportLineage".into(), metadata.lineage.clone());
    Ok(())
}

fn atomic_write_new_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Portable destination has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    AtomicFile::new(path, DisallowOverwrite)
        .write(|file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .map_err(|error| error.to_string())
}

fn ensure_external_portable_destination(
    managed_root: &Path,
    destination: &Path,
) -> Result<(), String> {
    let managed_root = fs::canonicalize(managed_root).map_err(|error| error.to_string())?;
    let mut existing_ancestor = if destination.exists() {
        destination
    } else {
        destination
            .parent()
            .ok_or("Portable Project destination has no parent")?
    };
    while !existing_ancestor.exists() {
        existing_ancestor = existing_ancestor
            .parent()
            .ok_or("Portable Project destination has no existing ancestor")?;
    }
    let canonical_ancestor =
        fs::canonicalize(existing_ancestor).map_err(|error| error.to_string())?;
    if canonical_ancestor.starts_with(&managed_root) {
        return Err(
            "Portable Project export destination must be outside the managed project library"
                .into(),
        );
    }
    Ok(())
}

fn rollback_portable_import(
    project_directory: &Path,
    index_path: &Path,
    previous_index_bytes: Option<&[u8]>,
    rollback_path: &Path,
) -> Result<(), String> {
    if project_directory.exists() {
        fs::remove_dir_all(project_directory).map_err(|error| error.to_string())?;
    }
    restore_optional_file(index_path, previous_index_bytes)?;
    remove_file_if_present(rollback_path);
    Ok(())
}

fn read_portable_library_rows(root: &Path, campus_target_id: &str) -> Result<LibraryIndex, String> {
    let index_path = root.join(LIBRARY_INDEX_FILE);
    let index = if index_path.exists() {
        serde_json::from_slice(&fs::read(index_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Invalid campus project library: {error}"))?
    } else {
        LibraryIndex::default()
    };
    for record in &index.projects {
        if record.campus_target_id != campus_target_id {
            return Err("Campus Project Library contains a record from another campus".into());
        }
        validate_portable_managed_path(&record.managed_relative_path)?;
    }
    Ok(index)
}

fn validate_portable_managed_path(relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    let components = path.components().collect::<Vec<_>>();
    if path.is_absolute()
        || components.len() != 3
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
        || components[0].as_os_str() != "projects"
        || components[2].as_os_str() != PROJECT_FILE_NAME
    {
        return Err("Portable import rollback contains an unsafe managed path".into());
    }
    Ok(path.to_path_buf())
}

pub(super) fn recover_interrupted_portable_import(
    root: &Path,
    index: &mut LibraryIndex,
) -> Result<(), String> {
    let rollback_path = root.join(PORTABLE_IMPORT_ROLLBACK_FILE);
    if !rollback_path.exists() {
        return Ok(());
    }
    let rollback: PortableImportRollback =
        serde_json::from_slice(&fs::read(&rollback_path).map_err(|error| error.to_string())?)
            .map_err(|error| {
                format!("Invalid Portable Project import rollback journal: {error}")
            })?;
    let relative = validate_portable_managed_path(&rollback.managed_relative_path)?;
    let project_directory = root
        .join(relative)
        .parent()
        .ok_or("Portable import rollback path has no parent")?
        .to_path_buf();
    rollback_portable_import(
        &project_directory,
        &root.join(LIBRARY_INDEX_FILE),
        rollback.previous_index_bytes.as_deref(),
        &rollback_path,
    )?;
    *index = match rollback.previous_index_bytes {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid rolled-back campus project library: {error}"))?,
        None => LibraryIndex::default(),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability() -> V11ConstructionCapability {
        V11ConstructionCapability::request(true, Some("1")).unwrap()
    }

    fn scope(id: &str, name: &str, poi: &str) -> CampusScope {
        CampusScope::new(id, name, [121.4, 31.2])
            .unwrap()
            .with_gaode_poi_id(poi)
            .unwrap()
    }

    #[test]
    fn export_requires_replacement_confirmation_and_preserves_project_identity() {
        let root = tempfile::tempdir().unwrap();
        let destination_root = tempfile::tempdir().unwrap();
        let destination = destination_root.path().join("portable.campus-project.json");
        fs::write(&destination, b"existing").unwrap();
        let mut library =
            CampusProjectLibrary::open_for_construction(root.path(), "putuo", &capability())
                .unwrap();
        let project = library
            .create_project(
                scope("putuo", "Putuo Campus", "B001"),
                "Library",
                InstallationId::new("installation-a").unwrap(),
            )
            .unwrap();
        let id = project.id().clone();
        let managed_path = library
            .record(&id)
            .unwrap()
            .managed_relative_path()
            .to_owned();

        let error = library
            .export_portable_project(&project, &destination, PortableDestination::CreateNew)
            .unwrap_err();

        assert!(error.contains("confirmation"));
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        assert_eq!(project.id(), &id);
        assert_eq!(
            library.record(&id).unwrap().managed_relative_path(),
            managed_path
        );
    }

    #[test]
    fn import_uses_gaode_identity_creates_new_identity_and_resolves_name_deterministically() {
        let source_root = tempfile::tempdir().unwrap();
        let target_root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("library.campus-project.json");
        let actor = InstallationId::new("installation-a").unwrap();
        let campus = scope("putuo", "Putuo Campus", "B001");
        let mut source_library =
            CampusProjectLibrary::open_for_construction(source_root.path(), "putuo", &capability())
                .unwrap();
        let source_project = source_library
            .create_project(campus.clone(), "Library", actor.clone())
            .unwrap();
        source_library
            .export_portable_project(
                &source_project,
                &portable_path,
                PortableDestination::CreateNew,
            )
            .unwrap();
        let source_bytes = fs::read(&portable_path).unwrap();

        let mut target_library =
            CampusProjectLibrary::open_for_construction(target_root.path(), "putuo", &capability())
                .unwrap();
        target_library
            .create_project(campus.clone(), "Library", actor.clone())
            .unwrap();
        let imported = target_library
            .import_portable_project(
                &portable_path,
                campus,
                CampusTargetMatchApproval::AutomaticOnly,
                actor,
            )
            .unwrap();

        assert_ne!(imported.id(), source_project.id());
        assert_eq!(imported.name(), "Library（导入 1）");
        assert_eq!(
            imported
                .portable_import_lineage()
                .unwrap()
                .unwrap()
                .source_project_id(),
            source_project.id()
        );
        assert_eq!(fs::read(&portable_path).unwrap(), source_bytes);
        assert!(target_library.record(imported.id()).is_some());
    }

    #[test]
    fn matching_names_and_locations_still_require_human_confirmation_without_gaode_identity() {
        let source_root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("portable.json");
        let mut library =
            CampusProjectLibrary::open_for_construction(source_root.path(), "putuo", &capability())
                .unwrap();
        let project = library
            .create_project(
                CampusScope::new("putuo", "Same Campus", [121.4, 31.2]).unwrap(),
                "Project",
                InstallationId::new("installation-a").unwrap(),
            )
            .unwrap();
        library
            .export_portable_project(&project, &portable_path, PortableDestination::CreateNew)
            .unwrap();

        let selected = CampusScope::new("putuo", "Same Campus", [121.4, 31.2]).unwrap();
        assert!(matches!(
            CampusProjectLibrary::inspect_portable_project(&portable_path, &selected).unwrap(),
            CampusTargetMatchRequirement::HumanConfirmationRequired { .. }
        ));
        let error = library
            .import_portable_project(
                &portable_path,
                selected,
                CampusTargetMatchApproval::AutomaticOnly,
                InstallationId::new("installation-b").unwrap(),
            )
            .unwrap_err();
        assert!(error.contains("human confirmation"));
    }

    #[test]
    fn transfer_faults_leave_no_partial_destination_or_project() {
        let source_root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("portable.json");
        let campus = scope("putuo", "Putuo Campus", "B001");
        let actor = InstallationId::new("installation-a").unwrap();
        let mut source_library =
            CampusProjectLibrary::open_for_construction(source_root.path(), "putuo", &capability())
                .unwrap();
        let project = source_library
            .create_project(campus.clone(), "Project", actor.clone())
            .unwrap();
        fs::write(&portable_path, b"existing").unwrap();
        for point in [
            PortableTransferFaultPoint::ExportAfterStageWrite,
            PortableTransferFaultPoint::ExportAfterStageValidation,
        ] {
            source_library.inject_next_portable_failure(point);
            assert!(source_library
                .export_portable_project(
                    &project,
                    &portable_path,
                    PortableDestination::ReplaceConfirmed,
                )
                .is_err());
            assert_eq!(fs::read(&portable_path).unwrap(), b"existing");
        }
        source_library
            .export_portable_project(
                &project,
                &portable_path,
                PortableDestination::ReplaceConfirmed,
            )
            .unwrap();

        for point in [
            PortableTransferFaultPoint::ImportAfterTemporaryCopy,
            PortableTransferFaultPoint::ImportAfterMigration,
            PortableTransferFaultPoint::ImportAfterProjectWrite,
            PortableTransferFaultPoint::ImportAfterIndexWrite,
        ] {
            let target_root = tempfile::tempdir().unwrap();
            let mut target = CampusProjectLibrary::open_for_construction(
                target_root.path(),
                "putuo",
                &capability(),
            )
            .unwrap();
            target.inject_next_portable_failure(point);
            assert!(target
                .import_portable_project(
                    &portable_path,
                    campus.clone(),
                    CampusTargetMatchApproval::AutomaticOnly,
                    actor.clone(),
                )
                .is_err());
            assert!(target.index.projects.is_empty());
            let reopened = CampusProjectLibrary::open(target_root.path(), "putuo").unwrap();
            assert!(reopened.index.projects.is_empty());
        }
    }

    #[test]
    fn cross_campus_import_saves_active_project_before_switching() {
        let current_root = tempfile::tempdir().unwrap();
        let destination_root = tempfile::tempdir().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("portable.json");
        let actor = InstallationId::new("installation-a").unwrap();
        let current_scope = scope("current", "Current Campus", "CURRENT");
        let destination_scope = scope("destination", "Destination Campus", "DEST");

        let mut current_library = CampusProjectLibrary::open_for_construction(
            current_root.path(),
            "current",
            &capability(),
        )
        .unwrap();
        let current = current_library
            .create_project(current_scope, "Current Project", actor.clone())
            .unwrap();
        let mut session = Schema2ProjectSession::default();
        session
            .open_project(&current_library, current.id())
            .unwrap();
        session.dirty = true;

        let mut source_library = CampusProjectLibrary::open_for_construction(
            source_root.path(),
            "destination",
            &capability(),
        )
        .unwrap();
        let source = source_library
            .create_project(destination_scope.clone(), "Imported Project", actor.clone())
            .unwrap();
        source_library
            .export_portable_project(&source, &portable_path, PortableDestination::CreateNew)
            .unwrap();
        let mut destination_library = CampusProjectLibrary::open_for_construction(
            destination_root.path(),
            "destination",
            &capability(),
        )
        .unwrap();

        current_library.inject_next_save_failure(SaveFaultPoint::BeforeStageWrite);
        assert!(session
            .import_portable_across_campus(
                &mut current_library,
                &mut destination_library,
                &portable_path,
                destination_scope.clone(),
                CampusTargetMatchApproval::HumanConfirmed,
                actor.clone(),
            )
            .is_err());
        assert_eq!(session.active().unwrap().id(), current.id());
        assert!(destination_library.index.projects.is_empty());

        let imported = session
            .import_portable_across_campus(
                &mut current_library,
                &mut destination_library,
                &portable_path,
                destination_scope,
                CampusTargetMatchApproval::HumanConfirmed,
                actor,
            )
            .unwrap();
        assert_eq!(session.active().unwrap().id(), imported.id());
        assert_eq!(destination_library.index.projects.len(), 1);
        assert_eq!(current_library.index.projects.len(), 1);
    }

    #[test]
    fn export_keeps_editable_state_and_removes_non_portable_state() {
        let root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("portable.json");
        let mut library =
            CampusProjectLibrary::open_for_construction(root.path(), "putuo", &capability())
                .unwrap();
        let mut project = library
            .create_project(
                scope("putuo", "Putuo Campus", "B001"),
                "Project",
                InstallationId::new("installation-a").unwrap(),
            )
            .unwrap();
        project.optional_state.insert(
            "credentials".into(),
            serde_json::json!({"apiKey": "must-not-leave-machine"}),
        );
        project.optional_state.insert(
            "integrationPassword".into(),
            Value::String("also-secret".into()),
        );
        project.optional_state.insert(
            "accessTokenValue".into(),
            Value::String("also-secret".into()),
        );
        project
            .optional_state
            .insert("sessionCookie".into(), Value::String("also-secret".into()));
        project
            .optional_state
            .insert("runtimeLogs".into(), serde_json::json!(["private"]));
        project
            .optional_state
            .insert("tileCache".into(), serde_json::json!({"private": true}));
        project
            .optional_state
            .insert("previewImage".into(), Value::String("private".into()));
        project
            .optional_state
            .insert("appSettings".into(), serde_json::json!({"private": true}));
        project.optional_state.insert(
            "generatedArtifact".into(),
            Value::String("latest.schem".into()),
        );
        project.optional_state.insert(
            "machinePath".into(),
            Value::String(r"C:\private\project".into()),
        );
        project
            .optional_state
            .insert("authoritativeEvidenceMarker".into(), Value::Bool(true));

        library
            .export_portable_project(&project, &portable_path, PortableDestination::CreateNew)
            .unwrap();
        let raw: Value = serde_json::from_slice(&fs::read(&portable_path).unwrap()).unwrap();
        let exported = raw.get("project").unwrap();

        assert_eq!(
            exported.get("authoritativeEvidenceMarker"),
            Some(&Value::Bool(true))
        );
        assert!(exported.get("credentials").is_none());
        assert!(exported.get("integrationPassword").is_none());
        assert!(exported.get("accessTokenValue").is_none());
        assert!(exported.get("sessionCookie").is_none());
        assert!(exported.get("runtimeLogs").is_none());
        assert!(exported.get("tileCache").is_none());
        assert!(exported.get("previewImage").is_none());
        assert!(exported.get("appSettings").is_none());
        assert!(exported.get("generatedArtifact").is_none());
        assert!(exported.get("machinePath").is_none());
        for required in [
            "schemaVersion",
            "compatibilityProfile",
            "workflow",
            "foundation",
            "durability",
        ] {
            assert!(exported.get(required).is_some(), "missing {required}");
        }
    }

    #[test]
    fn schema1_import_migrates_the_temporary_copy_and_preserves_the_source() {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-data/v1-demo.campus.json");
        let source_bytes = fs::read(&source).unwrap();
        let target_root = tempfile::tempdir().unwrap();
        let selected_scope = scope("putuo", "ECNU Putuo Campus", "fixture-poi-putuo");
        let mut library =
            CampusProjectLibrary::open_for_construction(target_root.path(), "putuo", &capability())
                .unwrap();

        let imported = library
            .import_portable_project(
                &source,
                selected_scope,
                CampusTargetMatchApproval::AutomaticOnly,
                InstallationId::new("installation-b").unwrap(),
            )
            .unwrap();

        assert_eq!(imported.schema_version(), SCHEMA_2_VERSION);
        assert_eq!(
            imported
                .portable_import_lineage()
                .unwrap()
                .unwrap()
                .source_schema_version(),
            1
        );
        assert!(imported.legacy_migration().unwrap().is_some());
        assert_eq!(fs::read(source).unwrap(), source_bytes);
    }

    #[test]
    fn corrupt_and_unsafe_input_creates_no_partial_project() {
        let source_root = tempfile::tempdir().unwrap();
        let target_root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("portable.json");
        let campus = scope("putuo", "Putuo Campus", "B001");
        let actor = InstallationId::new("installation-a").unwrap();
        let mut source_library =
            CampusProjectLibrary::open_for_construction(source_root.path(), "putuo", &capability())
                .unwrap();
        let project = source_library
            .create_project(campus.clone(), "Project", actor.clone())
            .unwrap();
        source_library
            .export_portable_project(&project, &portable_path, PortableDestination::CreateNew)
            .unwrap();
        let mut raw: Value = serde_json::from_slice(&fs::read(&portable_path).unwrap()).unwrap();
        raw.pointer_mut("/project")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("outputPath".into(), Value::String("../escape".into()));
        fs::write(&portable_path, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
        let mut target =
            CampusProjectLibrary::open_for_construction(target_root.path(), "putuo", &capability())
                .unwrap();

        assert!(target
            .import_portable_project(
                &portable_path,
                campus.clone(),
                CampusTargetMatchApproval::AutomaticOnly,
                actor.clone(),
            )
            .is_err());
        assert!(target.index.projects.is_empty());
        fs::write(&portable_path, b"{not json").unwrap();
        assert!(target
            .import_portable_project(
                &portable_path,
                campus,
                CampusTargetMatchApproval::AutomaticOnly,
                actor,
            )
            .is_err());
        assert!(target.index.projects.is_empty());
    }

    #[test]
    fn create_new_commit_never_overwrites_a_destination_collision() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("portable.json");
        fs::write(&destination, b"racing-writer").unwrap();

        assert!(atomic_write_new_bytes(&destination, b"portable").is_err());
        assert_eq!(fs::read(destination).unwrap(), b"racing-writer");
    }

    #[test]
    fn library_open_rolls_back_an_interrupted_import_journal() {
        let target_root = tempfile::tempdir().unwrap();
        let actor = InstallationId::new("installation-a").unwrap();
        let campus = scope("putuo", "Putuo Campus", "B001");
        let candidate = Schema2Project::new(campus, "Interrupted".into(), actor).unwrap();
        let relative = format!("projects/{}/{PROJECT_FILE_NAME}", candidate.id().as_str());
        let project_path = target_root.path().join(&relative);
        atomic_write_json(&project_path, &candidate).unwrap();
        let partial_index = LibraryIndex {
            projects: vec![record_from_project(&candidate, relative.clone(), None)],
        };
        atomic_write_json(&target_root.path().join(LIBRARY_INDEX_FILE), &partial_index).unwrap();
        atomic_write_json(
            &target_root.path().join(PORTABLE_IMPORT_ROLLBACK_FILE),
            &PortableImportRollback {
                managed_relative_path: relative,
                previous_index_bytes: None,
            },
        )
        .unwrap();

        let recovered = CampusProjectLibrary::open(target_root.path(), "putuo").unwrap();

        assert!(recovered.index.projects.is_empty());
        assert!(!project_path.exists());
        assert!(!target_root.path().join(LIBRARY_INDEX_FILE).exists());
        assert!(!target_root
            .path()
            .join(PORTABLE_IMPORT_ROLLBACK_FILE)
            .exists());
    }

    #[test]
    fn export_rejects_every_destination_inside_the_managed_library() {
        let root = tempfile::tempdir().unwrap();
        let mut library =
            CampusProjectLibrary::open_for_construction(root.path(), "putuo", &capability())
                .unwrap();
        let project = library
            .create_project(
                scope("putuo", "Putuo Campus", "B001"),
                "Project",
                InstallationId::new("installation-a").unwrap(),
            )
            .unwrap();
        let managed_destination = root.path().join("exports/portable.json");

        let error = library
            .export_portable_project(
                &project,
                managed_destination,
                PortableDestination::ReplaceConfirmed,
            )
            .unwrap_err();

        assert!(error.contains("outside the managed project library"));
        assert_eq!(library.open_project(project.id()).unwrap(), project);
    }

    #[test]
    fn campus_target_confirmation_copy_is_immutable_from_source_replacement() {
        let source_root = tempfile::tempdir().unwrap();
        let target_root = tempfile::tempdir().unwrap();
        let transfer_root = tempfile::tempdir().unwrap();
        let portable_path = transfer_root.path().join("portable.json");
        let campus = scope("putuo", "Putuo Campus", "B001");
        let actor = InstallationId::new("installation-a").unwrap();
        let mut source_library =
            CampusProjectLibrary::open_for_construction(source_root.path(), "putuo", &capability())
                .unwrap();
        let project = source_library
            .create_project(campus.clone(), "Project", actor)
            .unwrap();
        source_library
            .export_portable_project(&project, &portable_path, PortableDestination::CreateNew)
            .unwrap();
        let target =
            CampusProjectLibrary::open_for_construction(target_root.path(), "putuo", &capability())
                .unwrap();
        let confirmed_copy = target
            .stage_portable_confirmation_copy(&portable_path)
            .unwrap();
        fs::write(&portable_path, b"{replaced").unwrap();

        assert!(matches!(
            CampusProjectLibrary::inspect_portable_project(&confirmed_copy, &campus).unwrap(),
            CampusTargetMatchRequirement::AutomaticGaodePoiMatch
        ));
        assert!(CampusProjectLibrary::inspect_portable_project(&portable_path, &campus).is_err());
    }
}
