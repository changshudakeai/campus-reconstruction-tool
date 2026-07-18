use campus_state::{
    v11_construction_enabled, CampusProjectLibrary, CampusScope, InstallationId, ProjectId,
    Schema2ProjectSession, V11ConstructionCapability,
};
use std::path::Path;

const DEVELOPMENT_PROJECT_NAME: &str = "V1.1 普陀校园重建";
const PUTUO_TARGET_ID: &str = "gaode:B00155J6JH";

pub fn bootstrap_if_enabled(
    application_data: &Path,
    development_build: bool,
    gate_value: Option<&str>,
) -> Result<Option<ProjectId>, String> {
    if !v11_construction_enabled(development_build, gate_value) {
        return Ok(None);
    }

    let capability = V11ConstructionCapability::request(development_build, gate_value)?;
    let mut library = CampusProjectLibrary::open_for_construction(
        application_data.join("v1.1-development-library"),
        PUTUO_TARGET_ID,
        &capability,
    )?;
    let project_id = match library.development_canonical_record() {
        Some(record) => record.project_id().clone(),
        None => library
            .create_development_canonical_project(
                CampusScope::new(PUTUO_TARGET_ID, "华东师范大学普陀校区", [121.395, 31.202])?,
                DEVELOPMENT_PROJECT_NAME,
                InstallationId::new("campus-native-development")?,
            )?
            .id()
            .clone(),
    };

    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, &project_id)?;
    let project = session
        .active()
        .cloned()
        .ok_or("Development schema-2 project did not become active")?;
    library.save_project(&project)?;
    drop(session);
    let mut reopened_session = Schema2ProjectSession::default();
    reopened_session.open_project(&library, &project_id)?;
    Ok(Some(project_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_gate_creates_and_reopens_one_canonical_project() {
        let directory = tempfile::tempdir().unwrap();

        let first = bootstrap_if_enabled(directory.path(), true, Some("1"))
            .unwrap()
            .unwrap();
        let mut library = CampusProjectLibrary::open(
            directory.path().join("v1.1-development-library"),
            PUTUO_TARGET_ID,
        )
        .unwrap();
        library
            .rename_project(
                &first,
                "renamed canonical project",
                InstallationId::new("test-installation").unwrap(),
            )
            .unwrap();
        let second = bootstrap_if_enabled(directory.path(), true, Some("1"))
            .unwrap()
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            CampusProjectLibrary::open(
                directory.path().join("v1.1-development-library"),
                PUTUO_TARGET_ID,
            )
            .unwrap()
            .open_project(&first)
            .unwrap()
            .schema_version(),
            2
        );
        assert_eq!(
            CampusProjectLibrary::open(
                directory.path().join("v1.1-development-library"),
                PUTUO_TARGET_ID,
            )
            .unwrap()
            .open_project(&first)
            .unwrap()
            .name(),
            "renamed canonical project"
        );
    }

    #[test]
    fn stable_path_does_not_create_a_v11_library() {
        let directory = tempfile::tempdir().unwrap();

        assert!(bootstrap_if_enabled(directory.path(), false, Some("1"))
            .unwrap()
            .is_none());
        assert!(!directory.path().join("v1.1-development-library").exists());
    }
}
