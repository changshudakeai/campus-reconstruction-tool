use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static SECRET_VALUES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    ZhCn,
    En,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutAction {
    OpenGuidance,
    CloseGuidance,
    CloseSettings,
    CloseQuickStart,
    CloseUtilities,
    DismissError,
    CloseAbout,
    CloseEvidence,
    CloseCandidateDetails,
    NewProject,
    OpenProject,
    SaveProject,
    ExportPortableProject,
    UndoProjectHistory,
    RedoProjectHistory,
    DeleteBoundaryVertex,
    ConfirmWorkflowTask,
    CancelMapTool,
    CancelWorkflowTask,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppPreferences {
    guidance_dismissed: bool,
}

impl AppPreferences {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes).map_err(|error| format!("Invalid App preferences: {error}"))
    }

    pub fn should_show_guidance(&self) -> bool {
        !self.guidance_dismissed
    }

    pub fn dismiss_guidance(&self, path: &Path) -> Result<(), String> {
        Self {
            guidance_dismissed: true,
        }
        .save(path)
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("App preferences path has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ModalState {
    #[default]
    None,
    Guidance,
    Settings,
    QuickStart,
    Utilities,
    Error,
    About,
    Evidence,
    CandidateDetails,
}

impl ModalState {
    fn close_action(self) -> Option<ShortcutAction> {
        match self {
            Self::None => None,
            Self::Guidance => Some(ShortcutAction::CloseGuidance),
            Self::Settings => Some(ShortcutAction::CloseSettings),
            Self::QuickStart => Some(ShortcutAction::CloseQuickStart),
            Self::Utilities => Some(ShortcutAction::CloseUtilities),
            Self::Error => Some(ShortcutAction::DismissError),
            Self::About => Some(ShortcutAction::CloseAbout),
            Self::Evidence => Some(ShortcutAction::CloseEvidence),
            Self::CandidateDetails => Some(ShortcutAction::CloseCandidateDetails),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MapToolState {
    #[default]
    None,
    BoundaryVertexSelected,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkflowTaskState {
    #[default]
    None,
    Confirmable,
    Cancellable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShortcutContext {
    pub text_input_focused: bool,
    pub modal: ModalState,
    pub map_tool: MapToolState,
    pub workflow: WorkflowTaskState,
    pub has_active_project: bool,
    pub can_create_project: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shortcut {
    NewProject,
    OpenProject,
    Save,
    ExportPortableProject,
    Undo,
    Redo,
    Delete,
    Confirm,
    Escape,
    Guidance,
}

impl Shortcut {
    pub const ALL: [Self; 10] = [
        Self::NewProject,
        Self::OpenProject,
        Self::Save,
        Self::ExportPortableProject,
        Self::Undo,
        Self::Redo,
        Self::Delete,
        Self::Confirm,
        Self::Escape,
        Self::Guidance,
    ];

    pub fn keys(self) -> &'static str {
        match self {
            Self::NewProject => "Ctrl+N",
            Self::OpenProject => "Ctrl+O",
            Self::Save => "Ctrl+S",
            Self::ExportPortableProject => "Ctrl+Shift+S",
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Y / Ctrl+Shift+Z",
            Self::Delete => "Delete",
            Self::Confirm => "Ctrl+Enter",
            Self::Escape => "Esc",
            Self::Guidance => "F1 / ?",
        }
    }

    pub fn label(self, locale: Locale) -> &'static str {
        match (self, locale) {
            (Self::NewProject, Locale::ZhCn) => "新建项目",
            (Self::NewProject, Locale::En) => "New project",
            (Self::OpenProject, Locale::ZhCn) => "打开或导入便携项目",
            (Self::OpenProject, Locale::En) => "Open or import Portable Project",
            (Self::Save, Locale::ZhCn) => "立即创建项目保存点",
            (Self::Save, Locale::En) => "Create a Project Save Point now",
            (Self::ExportPortableProject, Locale::ZhCn) => "导出便携项目",
            (Self::ExportPortableProject, Locale::En) => "Export Portable Project",
            (Self::Undo, Locale::ZhCn) => "撤销",
            (Self::Undo, Locale::En) => "Undo",
            (Self::Redo, Locale::ZhCn) => "重做",
            (Self::Redo, Locale::En) => "Redo",
            (Self::Delete, Locale::ZhCn) => "删除当前选择",
            (Self::Delete, Locale::En) => "Delete current selection",
            (Self::Confirm, Locale::ZhCn) => "确认当前任务",
            (Self::Confirm, Locale::En) => "Confirm current task",
            (Self::Escape, Locale::ZhCn) => "取消当前上下文",
            (Self::Escape, Locale::En) => "Cancel current context",
            (Self::Guidance, Locale::ZhCn) => "重新打开引导",
            (Self::Guidance, Locale::En) => "Reopen guidance",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutOutcome {
    Available {
        action: ShortcutAction,
        reason: &'static str,
    },
    Unavailable {
        reason: &'static str,
    },
    PassToTextInput,
}

impl ShortcutOutcome {
    pub fn action(self) -> Option<ShortcutAction> {
        match self {
            Self::Available { action, .. } => Some(action),
            Self::Unavailable { .. } | Self::PassToTextInput => None,
        }
    }

    pub fn is_available(self) -> bool {
        matches!(self, Self::Available { .. } | Self::PassToTextInput)
    }

    pub fn reason(self) -> &'static str {
        match self {
            Self::Available { reason, .. } | Self::Unavailable { reason } => reason,
            Self::PassToTextInput => "Handled by the focused text field.",
        }
    }
}

fn available(
    action: ShortcutAction,
    zh: &'static str,
    en: &'static str,
    locale: Locale,
) -> ShortcutOutcome {
    ShortcutOutcome::Available {
        action,
        reason: if locale == Locale::En { en } else { zh },
    }
}

fn unavailable(zh: &'static str, en: &'static str, locale: Locale) -> ShortcutOutcome {
    ShortcutOutcome::Unavailable {
        reason: if locale == Locale::En { en } else { zh },
    }
}

pub fn resolve_shortcut(
    shortcut: Shortcut,
    context: ShortcutContext,
    locale: Locale,
) -> ShortcutOutcome {
    if shortcut == Shortcut::Guidance {
        return available(
            ShortcutAction::OpenGuidance,
            "引导始终可用，且不会改变项目进度。",
            "Guidance is always available and does not change project progress.",
            locale,
        );
    }

    if context.text_input_focused
        && matches!(
            shortcut,
            Shortcut::Undo | Shortcut::Redo | Shortcut::Delete | Shortcut::Confirm
        )
    {
        return ShortcutOutcome::PassToTextInput;
    }

    if context.modal != ModalState::None {
        if shortcut == Shortcut::Escape {
            return available(
                context
                    .modal
                    .close_action()
                    .expect("non-empty modal has a close action"),
                "关闭最上层窗口。",
                "Closes the top modal.",
                locale,
            );
        }
        return unavailable(
            "请先处理或关闭最上层窗口。",
            "Finish or close the top modal first.",
            locale,
        );
    }

    match shortcut {
        Shortcut::NewProject => {
            if context.can_create_project {
                available(
                    ShortcutAction::NewProject,
                    "当前校区目标已确认，可以新建项目。",
                    "The Campus Target is confirmed; a project can be created.",
                    locale,
                )
            } else {
                unavailable(
                    "请先确认校区目标，再在校区项目库中新建项目。",
                    "Confirm a Campus Target before creating a project in its Campus Project Library.",
                    locale,
                )
            }
        }
        Shortcut::OpenProject => available(
            ShortcutAction::OpenProject,
            "打开或导入便携项目。",
            "Open or import a Portable Project.",
            locale,
        ),
        Shortcut::Save | Shortcut::ExportPortableProject if !context.has_active_project => unavailable(
            "当前没有打开的校园复刻项目。",
            "No Campus Reconstruction Project is open.",
            locale,
        ),
        Shortcut::Save => available(
            ShortcutAction::SaveProject,
            "立即创建项目保存点。",
            "Create a Project Save Point now.",
            locale,
        ),
        Shortcut::ExportPortableProject => available(
            ShortcutAction::ExportPortableProject,
            "导出便携项目不会改变当前项目身份或保存位置。",
            "Exporting a Portable Project does not change the active project's identity or save destination.",
            locale,
        ),
        Shortcut::Delete if context.map_tool == MapToolState::BoundaryVertexSelected => available(
            ShortcutAction::DeleteBoundaryVertex,
            "删除当前选择的边界顶点。",
            "Delete the selected boundary vertex.",
            locale,
        ),
        Shortcut::Escape if context.map_tool != MapToolState::None => available(
            ShortcutAction::CancelMapTool,
            "取消当前地图工具并返回审核。",
            "Cancel the active map tool and return to review.",
            locale,
        ),
        Shortcut::Confirm if context.workflow == WorkflowTaskState::Confirmable => available(
            ShortcutAction::ConfirmWorkflowTask,
            "当前任务满足确认条件。",
            "The current task is ready to confirm.",
            locale,
        ),
        Shortcut::Escape if context.workflow == WorkflowTaskState::Cancellable => available(
            ShortcutAction::CancelWorkflowTask,
            "取消当前未提交的任务操作。",
            "Cancel the current uncommitted workflow action.",
            locale,
        ),
        Shortcut::Undo if context.can_undo => available(
            ShortcutAction::UndoProjectHistory,
            "撤销最近一个项目历史操作。",
            "Undo the latest Project History Operation.",
            locale,
        ),
        Shortcut::Redo if context.can_redo => available(
            ShortcutAction::RedoProjectHistory,
            "重做最近撤销的项目历史操作。",
            "Redo the latest undone Project History Operation.",
            locale,
        ),
        Shortcut::Undo => unavailable(
            "项目历史中没有可撤销的操作。",
            "Project history has no operation to undo.",
            locale,
        ),
        Shortcut::Redo => unavailable(
            "项目历史中没有可重做的操作。",
            "Project history has no operation to redo.",
            locale,
        ),
        Shortcut::Delete => unavailable(
            "当前没有可删除的地图对象。",
            "No deletable map object is selected.",
            locale,
        ),
        Shortcut::Confirm => unavailable(
            "当前任务尚未满足确认条件。",
            "The current task is not ready to confirm.",
            locale,
        ),
        Shortcut::Escape => unavailable(
            "当前没有可取消的工具、任务或窗口。",
            "No tool, task, or modal is available to cancel.",
            locale,
        ),
        Shortcut::Guidance => unreachable!(),
    }
}

pub fn register_secret(value: impl Into<String>) {
    let value = value.into();
    if value.trim().is_empty() {
        return;
    }
    let secrets = SECRET_VALUES.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut secrets) = secrets.lock() {
        if !secrets.iter().any(|existing| existing == &value) {
            secrets.push(value);
        }
    }
}

pub fn sanitise_registered_diagnostic_value(key: &str, value: &str) -> String {
    let secrets = SECRET_VALUES
        .get()
        .and_then(|secrets| secrets.lock().ok().map(|secrets| secrets.clone()))
        .unwrap_or_default();
    let secret_refs = secrets.iter().map(String::as_str).collect::<Vec<_>>();
    sanitise_diagnostic_value(key, value, &secret_refs)
}

pub fn sanitise_diagnostic_value(key: &str, value: &str, secrets: &[&str]) -> String {
    let normalised_key = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if [
        "password",
        "secret",
        "token",
        "apikey",
        "securitycode",
        "credential",
    ]
    .iter()
    .any(|marker| normalised_key.contains(marker))
    {
        return "[REDACTED]".into();
    }

    let mut sanitised = value.to_string();
    for secret in secrets.iter().copied().filter(|secret| !secret.is_empty()) {
        sanitised = sanitised.replace(secret, "[REDACTED]");
    }
    sanitised
}

#[cfg(test)]
mod tests {
    use super::{
        register_secret, resolve_shortcut, sanitise_diagnostic_value,
        sanitise_registered_diagnostic_value, AppPreferences, Locale, MapToolState, ModalState,
        Shortcut, ShortcutAction, ShortcutContext, ShortcutOutcome, WorkflowTaskState,
    };

    #[test]
    fn first_run_guidance_is_a_persisted_app_preference_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let first_run = AppPreferences::load(&path).unwrap();
        assert!(first_run.should_show_guidance());

        first_run.dismiss_guidance(&path).unwrap();
        let returning = AppPreferences::load(&path).unwrap();
        assert!(!returning.should_show_guidance());
        returning.dismiss_guidance(&path).unwrap();

        let json = std::fs::read_to_string(path).unwrap();
        assert!(json.contains("guidance_dismissed"));
        assert!(!json.contains("project"));
        assert!(!json.contains("route"));
    }

    #[test]
    fn shortcut_priority_is_text_then_modal_then_map_then_workflow_then_history() {
        let context = ShortcutContext {
            text_input_focused: true,
            modal: ModalState::Settings,
            map_tool: MapToolState::BoundaryVertexSelected,
            workflow: WorkflowTaskState::Confirmable,
            has_active_project: true,
            can_create_project: true,
            can_undo: true,
            can_redo: true,
        };
        assert_eq!(
            resolve_shortcut(Shortcut::Undo, context, Locale::En),
            ShortcutOutcome::PassToTextInput
        );

        let context = ShortcutContext {
            text_input_focused: false,
            ..context
        };
        assert_eq!(
            resolve_shortcut(Shortcut::Escape, context, Locale::En).action(),
            Some(ShortcutAction::CloseSettings)
        );
        for (modal, expected) in [
            (ModalState::About, ShortcutAction::CloseAbout),
            (ModalState::Evidence, ShortcutAction::CloseEvidence),
            (
                ModalState::CandidateDetails,
                ShortcutAction::CloseCandidateDetails,
            ),
        ] {
            assert_eq!(
                resolve_shortcut(
                    Shortcut::Escape,
                    ShortcutContext { modal, ..context },
                    Locale::En,
                )
                .action(),
                Some(expected)
            );
        }

        let context = ShortcutContext {
            modal: ModalState::None,
            ..context
        };
        assert_eq!(
            resolve_shortcut(Shortcut::Delete, context, Locale::En).action(),
            Some(ShortcutAction::DeleteBoundaryVertex)
        );

        let context = ShortcutContext {
            map_tool: MapToolState::None,
            ..context
        };
        assert_eq!(
            resolve_shortcut(Shortcut::Confirm, context, Locale::En).action(),
            Some(ShortcutAction::ConfirmWorkflowTask)
        );
        assert_eq!(
            resolve_shortcut(Shortcut::Undo, context, Locale::En).action(),
            Some(ShortcutAction::UndoProjectHistory)
        );
    }

    #[test]
    fn unavailable_shortcuts_explain_the_exact_current_reason() {
        let context = ShortcutContext::default();
        let save = resolve_shortcut(Shortcut::Save, context, Locale::En);
        assert!(!save.is_available());
        assert_eq!(save.reason(), "No Campus Reconstruction Project is open.");

        let delete = resolve_shortcut(Shortcut::Delete, context, Locale::ZhCn);
        assert!(!delete.is_available());
        assert_eq!(delete.reason(), "当前没有可删除的地图对象。");
    }

    #[test]
    fn diagnostic_sanitisation_blocks_secret_names_and_values() {
        let secrets = ["gaode-real-key", "acquisition-bearer-token"];
        assert_eq!(
            sanitise_diagnostic_value("api_key", "gaode-real-key", &secrets),
            "[REDACTED]"
        );
        assert_eq!(
            sanitise_diagnostic_value(
                "message",
                "request failed with acquisition-bearer-token",
                &secrets,
            ),
            "request failed with [REDACTED]"
        );
        assert_eq!(
            sanitise_diagnostic_value("campus", "华东师范大学普陀校区", &secrets),
            "华东师范大学普陀校区"
        );

        let registered = format!("registered-secret-{}", std::process::id());
        register_secret(registered.clone());
        assert_eq!(
            sanitise_registered_diagnostic_value(
                "message",
                &format!("service echoed {registered}"),
            ),
            "service echoed [REDACTED]"
        );
    }
}
