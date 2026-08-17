//! 工单 04 的校区/方案/回收站生产适配器（S1-04）。
//!
//! 每个请求一次调用 F1（global-settings）/F3（project-management）的公开接口，
//! 并返回完整页面状态、操作结果、导航决定与通知事实；S1 不直接读写数据库，
//! 不持有校区/方案业务副本，数据读写失败时明确失败并允许同一入口重试。

use std::cell::RefCell;
use std::rc::Rc;

use localization::Localization;
use notification_center::Notification;
use project_management::CampusPlanSnapshot;
use shared_domain_types::{CampusId, PlanId};

use crate::presentation::{
    CampusPlanPageState, ConfirmationPresentation, InputDialogPresentation, NavigationDecision,
    NotificationFact, Presentation, PresentationAdapter, Screen, TrashPageState, TrashRequest,
};
use crate::production::campus_search::CampusSearchController;
use crate::production::startup_settings::campus_plan_page;
use crate::production::workspace_boundary::WorkspaceProductionContext;
use crate::production::{PendingConfirmation, PendingInput, ProductionEntries};
use crate::runtime::format_relative_time;
use crate::{AppWindow, CampusData, TrashItemData, ViewModelInjector};

#[cfg(test)]
use crate::production::record_entry_call;

/// 校区与方案入口的一次请求：读取页面或执行一次校区/方案操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CampusPlanRequest {
    /// 读取并显示校区选择页（搜索框 + 最近使用记录）。
    CampusSelect,
    /// 读取并显示当前校区的方案列表。
    PlanList,
    /// 用户点击"搜索"或按回车后执行高德在线校区搜索（输入期间不自动搜索）。
    SearchCampus { query: String },
    /// S1 内部轮询后台校区搜索的真实终态（D-3，镜像采集轮询模式）。
    PollSearch,
    /// 选择校区（搜索结果或最近记录行）：记住校区并进入其方案列表；
    /// 已在最近记录中时返回"该校区已添加，已为你切换"通知事实。
    SelectCampus { campus_id: String },
    /// 搜索候选详情确认窗点了"确认"（T05 显式确认；ADR-0008 建/选校区）。
    ConfirmSelectCampus { poi_id: String },
    /// 搜索候选详情确认窗点了"取消"（返回候选列表重选）。
    CancelSelectCampus,
    /// 最近记录右侧小叉：立即移除快捷记录，不弹确认窗。
    RemoveRecentCampus { campus_id: String },
    /// 请求新建方案（返回输入窗，预填建议名）。
    RequestCreatePlan,
    /// 用户确认输入窗后创建方案。
    ConfirmCreatePlan { name: String },
    /// 请求改名（返回输入窗，预填当前方案名）。
    RequestRenamePlan { plan_id: String },
    /// 用户确认输入窗后执行改名。
    ConfirmRenamePlan { plan_id: String, name: String },
    /// 复制方案（自动追加"副本"后缀）。
    DuplicatePlan { plan_id: String },
    /// 请求删除方案（先返回确认窗）。
    RequestDeletePlan { plan_id: String },
    /// 用户确认后删除方案进回收站。
    ConfirmDeletePlan { plan_id: String },
}

/// 校区与方案生产适配器：一次请求调用 F1/F3 公开接口并返回完整呈现结果。
pub(crate) struct CampusPlanProductionAdapter {
    pub(crate) injector: Rc<RefCell<ViewModelInjector>>,
    pub(crate) workspace: WorkspaceProductionContext,
    /// 校区在线搜索控制（D-3：B3 状态机 + 可注入传输）
    pub(crate) search: CampusSearchController,
}

impl PresentationAdapter<CampusPlanRequest, CampusPlanPageState> for CampusPlanProductionAdapter {
    fn present(&mut self, request: CampusPlanRequest) -> Presentation<CampusPlanPageState> {
        #[cfg(test)]
        record_entry_call(2);
        match request {
            CampusPlanRequest::CampusSelect => {
                present_campus_select(&self.injector, &self.workspace)
            }
            CampusPlanRequest::PlanList => present_plan_list(&self.injector, &self.workspace),

            CampusPlanRequest::SearchCampus { query } => self.present_search(&query),
            CampusPlanRequest::PollSearch => self.present_poll_search(),
            CampusPlanRequest::SelectCampus { campus_id } => {
                // 最近记录行携带校区 UUID；搜索候选行携带高德 POI 标识。
                // 按 ID 形态区分两种"点选"（D-3，重复点选只切换不重复建）。
                if CampusId::parse(&campus_id).is_ok() {
                    present_select_campus(&self.injector, &self.workspace, &campus_id)
                } else {
                    self.present_select_search_candidate(&campus_id)
                }
            }
            CampusPlanRequest::ConfirmSelectCampus { poi_id } => {
                self.present_confirm_select_campus(&poi_id)
            }
            CampusPlanRequest::CancelSelectCampus => self.present_cancel_select_campus(),
            CampusPlanRequest::RemoveRecentCampus { campus_id } => {
                present_remove_recent_campus(&self.injector, &self.workspace, &campus_id)
            }
            CampusPlanRequest::RequestCreatePlan => {
                present_request_create_plan(&self.injector, &self.workspace)
            }
            CampusPlanRequest::ConfirmCreatePlan { name } => {
                present_confirm_create_plan(&self.injector, &self.workspace, &name)
            }
            CampusPlanRequest::RequestRenamePlan { plan_id } => {
                present_request_rename_plan(&self.injector, &self.workspace, &plan_id)
            }
            CampusPlanRequest::ConfirmRenamePlan { plan_id, name } => {
                present_confirm_rename_plan(&self.injector, &self.workspace, &plan_id, &name)
            }
            CampusPlanRequest::DuplicatePlan { plan_id } => {
                present_duplicate_plan(&self.injector, &self.workspace, &plan_id)
            }
            CampusPlanRequest::RequestDeletePlan { plan_id } => {
                present_request_delete_plan(&self.injector, &self.workspace, &plan_id)
            }
            CampusPlanRequest::ConfirmDeletePlan { plan_id } => {
                present_confirm_delete_plan(&self.injector, &self.workspace, &plan_id)
            }
        }
    }
}

fn present_campus_select(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
) -> Presentation<CampusPlanPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    match campus_select_page(&injector, workspace) {
        Ok(page) => Presentation::ready(page)
            .with_navigation(NavigationDecision::Show(Screen::CampusSelect)),
        Err(error) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(campus_error_fact(l10n, &error.to_string())),
    }
}

fn present_plan_list(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
) -> Presentation<CampusPlanPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    match plan_list_page(&injector, workspace) {
        Ok(page) => {
            Presentation::ready(page).with_navigation(NavigationDecision::Show(Screen::PlanList))
        }
        Err(error) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_select_campus(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    campus_id: &str,
) -> Presentation<CampusPlanPageState> {
    // 阶段 1：可变操作（记住校区），错误归一为文本
    let (remembered, already_added) = {
        let mut injector = injector.borrow_mut();
        let parsed = match CampusId::parse(campus_id) {
            Ok(id) => id,
            Err(error) => {
                let page = campus_page_fallback(&injector, workspace);
                return Presentation::failed(page)
                    .with_notification(plan_error_fact(injector.l10n(), &error.to_string()));
            }
        };
        // 已在最近记录中 → 直接切换并给出通知事实；否则记录为最近使用
        let already_added = injector
            .settings()
            .recent_campuses()
            .map(|recents| recents.iter().any(|campus| campus.id == parsed))
            .unwrap_or(false);
        let remembered = injector
            .settings_mut()
            .remember_campus(&parsed)
            .map_err(|error| error.to_string());
        (remembered, already_added)
    };
    // 阶段 2：只读构建完整方案列表页
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    match (remembered, page) {
        (Ok(()), Ok(page)) => {
            let mut result = Presentation::succeeded(page)
                .with_navigation(NavigationDecision::Show(Screen::PlanList));
            if already_added {
                result = result.with_notification(campus_info_fact(
                    l10n,
                    "campus.already_added_title",
                    "campus.already_added",
                ));
            }
            result
        }
        (Err(message), _) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &message)),
        (_, Err(error)) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_remove_recent_campus(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    campus_id: &str,
) -> Presentation<CampusPlanPageState> {
    let removed = {
        let mut injector = injector.borrow_mut();
        let parsed = match CampusId::parse(campus_id) {
            Ok(id) => id,
            Err(error) => {
                let page = campus_page_fallback(&injector, workspace);
                return Presentation::failed(page)
                    .with_notification(campus_error_fact(injector.l10n(), &error.to_string()));
            }
        };
        injector
            .settings_mut()
            .remove_recent_campus(&parsed)
            .map_err(|error| error.to_string())
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = campus_select_page(&injector, workspace);
    match (removed, page) {
        (Ok(()), Ok(page)) => Presentation::succeeded(page).with_notification(campus_info_fact(
            l10n,
            "campus.recent_removed_title",
            "campus.recent_removed",
        )),
        (Err(message), _) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(campus_error_fact(l10n, &message)),
        (_, Err(error)) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(campus_error_fact(l10n, &error.to_string())),
    }
}

fn present_request_create_plan(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
) -> Presentation<CampusPlanPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    let default_name = current_campus_id(&injector)
        .map(|campus_id| {
            injector
                .projects()
                .suggest_plan_name(&campus_id, &l10n.t("plan.default_name"))
        })
        .transpose();
    match (page, default_name) {
        (Ok(page), Ok(Some(name))) => {
            Presentation::needs_input(page, create_plan_input(l10n, name))
        }
        (Ok(page), Ok(None)) => Presentation::failed(page)
            .with_notification(plan_error_fact(l10n, &l10n.t("error.campus_not_found"))),
        (Ok(page), Err(error)) => {
            Presentation::failed(page).with_notification(plan_error_fact(l10n, &error.to_string()))
        }
        (Err(error), _) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_confirm_create_plan(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    name: &str,
) -> Presentation<CampusPlanPageState> {
    let created = {
        let mut injector = injector.borrow_mut();
        if name.trim().is_empty() {
            // 空名不提交：输入窗保持打开（既有行为）
            let l10n = injector.l10n();
            let page = plan_list_page(&injector, workspace);
            let page = match page {
                Ok(page) => page,
                Err(error) => {
                    return Presentation::failed(campus_page_fallback(&injector, workspace))
                        .with_notification(plan_error_fact(l10n, &error.to_string()))
                }
            };
            return Presentation::needs_input(page, create_plan_input(l10n, name.to_owned()));
        }
        let result: Result<(), project_management::Error> = match current_campus_id(&injector) {
            Some(campus_id) => injector
                .projects_mut()
                .create_plan(&campus_id, name)
                .map(|_| ()),
            None => Err(project_management::Error::PlanNotFound(String::new())),
        };
        match result {
            Ok(()) => Ok(()),
            Err(project_management::Error::PlanNotFound(_)) => {
                Err(injector.l10n().t("error.campus_not_found"))
            }
            Err(error) => Err(plan_error_body(injector.l10n(), &error)),
        }
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    match (created, page) {
        (Ok(()), Ok(page)) => Presentation::succeeded(page),
        (Err(message), Ok(page)) => Presentation::failed(page)
            .with_input(create_plan_input(l10n, name.to_owned()))
            .with_notification(plan_error_fact(l10n, &message)),
        (_, Err(error)) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_request_rename_plan(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    plan_id: &str,
) -> Presentation<CampusPlanPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    match page {
        Ok(page) => {
            let current_name = page
                .plans
                .iter()
                .find(|card| card.plan_id.as_str() == plan_id)
                .map(|card| card.name.to_string())
                .unwrap_or_default();
            Presentation::needs_input(page, rename_plan_input(l10n, current_name))
        }
        Err(error) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_confirm_rename_plan(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    plan_id: &str,
    name: &str,
) -> Presentation<CampusPlanPageState> {
    let renamed = {
        let mut injector = injector.borrow_mut();
        if name.trim().is_empty() {
            let l10n = injector.l10n();
            let page = plan_list_page(&injector, workspace);
            let page = match page {
                Ok(page) => page,
                Err(error) => {
                    return Presentation::failed(campus_page_fallback(&injector, workspace))
                        .with_notification(plan_error_fact(l10n, &error.to_string()))
                }
            };
            return Presentation::needs_input(page, rename_plan_input(l10n, name.to_owned()));
        }
        let result = PlanId::parse(plan_id)
            .map_err(|error| project_management::Error::InvalidId(error.to_string()))
            .and_then(|id| injector.projects_mut().rename_plan(&id, name));
        result.map_err(|error| plan_error_body(injector.l10n(), &error))
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    match (renamed, page) {
        (Ok(()), Ok(page)) => Presentation::succeeded(page),
        (Err(message), Ok(page)) => Presentation::failed(page)
            .with_input(rename_plan_input(l10n, name.to_owned()))
            .with_notification(plan_error_fact(l10n, &message)),
        (_, Err(error)) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_duplicate_plan(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    plan_id: &str,
) -> Presentation<CampusPlanPageState> {
    let duplicated = {
        let mut injector = injector.borrow_mut();
        let result = PlanId::parse(plan_id)
            .map_err(|error| project_management::Error::InvalidId(error.to_string()))
            .and_then(|id| {
                let suffix = injector.l10n().t("plan.duplicate_suffix");
                injector.projects_mut().duplicate_plan(&id, &suffix)
            });
        result
            .map(|_| ())
            .map_err(|error| plan_error_body(injector.l10n(), &error))
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    match (duplicated, page) {
        (Ok(()), Ok(page)) => Presentation::succeeded(page),
        (Err(message), Ok(page)) => {
            Presentation::failed(page).with_notification(plan_error_fact(l10n, &message))
        }
        (_, Err(error)) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_request_delete_plan(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    _plan_id: &str,
) -> Presentation<CampusPlanPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    match page {
        Ok(page) => Presentation::needs_confirmation(
            page,
            ConfirmationPresentation::new(
                l10n.t("dialog.delete_title"),
                l10n.t("plan.delete_confirm"),
                l10n.t("dialog.confirm_button"),
                l10n.t("dialog.cancel_button"),
            ),
        ),
        Err(error) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_confirm_delete_plan(
    injector: &Rc<RefCell<ViewModelInjector>>,
    workspace: &WorkspaceProductionContext,
    plan_id: &str,
) -> Presentation<CampusPlanPageState> {
    let deleted = {
        let mut injector = injector.borrow_mut();
        let result = PlanId::parse(plan_id)
            .map_err(|error| project_management::Error::InvalidId(error.to_string()))
            .and_then(|id| {
                let campus_id = current_campus_id(&injector)
                    .ok_or_else(|| project_management::Error::PlanNotFound(plan_id.to_owned()))?;
                injector.projects_mut().delete_plan(&campus_id, &id)
            });
        result
            .map(|_| ())
            .map_err(|error| plan_error_body(injector.l10n(), &error))
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = plan_list_page(&injector, workspace);
    match (deleted, page) {
        (Ok(()), Ok(page)) => Presentation::succeeded(page),
        (Err(message), Ok(page)) => {
            Presentation::failed(page).with_notification(plan_error_fact(l10n, &message))
        }
        (_, Err(error)) => Presentation::failed(campus_page_fallback(&injector, workspace))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn create_plan_input(l10n: &Localization, text: String) -> InputDialogPresentation {
    InputDialogPresentation::new(
        l10n.t("dialog.create_title"),
        l10n.t("dialog.name_label"),
        text,
        l10n.t("dialog.confirm_button"),
        l10n.t("dialog.cancel_button"),
        0,
    )
}

fn rename_plan_input(l10n: &Localization, text: String) -> InputDialogPresentation {
    InputDialogPresentation::new(
        l10n.t("dialog.rename_title"),
        l10n.t("dialog.name_label"),
        text,
        l10n.t("dialog.confirm_button"),
        l10n.t("dialog.cancel_button"),
        1,
    )
}

/// 回收站生产适配器：恢复/永久删除/清空均经 F3，S1 只呈现结果与通知。
pub(crate) struct TrashProductionAdapter {
    pub(crate) injector: Rc<RefCell<ViewModelInjector>>,
}

impl PresentationAdapter<TrashRequest, TrashPageState> for TrashProductionAdapter {
    fn present(&mut self, request: TrashRequest) -> Presentation<TrashPageState> {
        #[cfg(test)]
        record_entry_call(8);
        match request {
            TrashRequest::Show => present_trash_show(&self.injector),
            TrashRequest::Restore { trash_id } => present_trash_restore(&self.injector, &trash_id),
            TrashRequest::RequestPurge { trash_id } => {
                present_trash_request_purge(&self.injector, &trash_id)
            }
            TrashRequest::ConfirmPurge { trash_id } => {
                present_trash_confirm_purge(&self.injector, &trash_id)
            }
            TrashRequest::RequestClearAll => present_trash_request_clear(&self.injector),
            TrashRequest::ConfirmClearAll => present_trash_confirm_clear(&self.injector),
        }
    }
}

fn present_trash_show(injector: &Rc<RefCell<ViewModelInjector>>) -> Presentation<TrashPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    match trash_page(&injector) {
        Ok(page) => {
            Presentation::ready(page).with_navigation(NavigationDecision::Show(Screen::Trash))
        }
        Err(error) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_trash_restore(
    injector: &Rc<RefCell<ViewModelInjector>>,
    trash_id: &str,
) -> Presentation<TrashPageState> {
    let restored = {
        let mut injector = injector.borrow_mut();
        let template = injector
            .l10n()
            .t(project_management::RESTORE_NAME_TEMPLATE_KEY);
        current_campus_id(&injector)
            .ok_or_else(|| TrashPageError::NoCampus.to_string())
            .and_then(|campus_id| {
                injector
                    .projects_mut()
                    .restore_plan(&campus_id, trash_id, &template)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = trash_page(&injector);
    match (restored, page) {
        (Ok(()), Ok(page)) => Presentation::succeeded(page).with_notification(trash_info_fact(
            l10n,
            "trash.restored_title",
            "trash.restored_body",
        )),
        (Err(message), _) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &message)),
        (_, Err(error)) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_trash_request_purge(
    injector: &Rc<RefCell<ViewModelInjector>>,
    _trash_id: &str,
) -> Presentation<TrashPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = trash_page(&injector);
    match page {
        Ok(page) => Presentation::needs_confirmation(
            page,
            ConfirmationPresentation::new(
                l10n.t("trash.purge_confirm_title"),
                l10n.t("trash.purge_confirm_body"),
                l10n.t("dialog.confirm_button"),
                l10n.t("dialog.cancel_button"),
            ),
        ),
        Err(error) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_trash_confirm_purge(
    injector: &Rc<RefCell<ViewModelInjector>>,
    trash_id: &str,
) -> Presentation<TrashPageState> {
    let purged = {
        let mut injector = injector.borrow_mut();
        injector
            .projects_mut()
            .purge_plan_confirmed(trash_id)
            .map_err(|error| error.to_string())
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = trash_page(&injector);
    match (purged, page) {
        (Ok(()), Ok(page)) => Presentation::succeeded(page).with_notification(trash_info_fact(
            l10n,
            "trash.purged_title",
            "trash.purged_body",
        )),
        (Err(message), _) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &message)),
        (_, Err(error)) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_trash_request_clear(
    injector: &Rc<RefCell<ViewModelInjector>>,
) -> Presentation<TrashPageState> {
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = trash_page(&injector);
    match page {
        Ok(page) => Presentation::needs_confirmation(
            page,
            ConfirmationPresentation::new(
                l10n.t("trash.clear_confirm_title"),
                l10n.t("trash.clear_confirm_body"),
                l10n.t("dialog.confirm_button"),
                l10n.t("dialog.cancel_button"),
            ),
        ),
        Err(error) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

fn present_trash_confirm_clear(
    injector: &Rc<RefCell<ViewModelInjector>>,
) -> Presentation<TrashPageState> {
    let cleared = {
        let mut injector = injector.borrow_mut();
        current_campus_id(&injector)
            .ok_or_else(|| TrashPageError::NoCampus.to_string())
            .and_then(|campus_id| {
                injector
                    .projects_mut()
                    .purge_all_trash_confirmed(&campus_id)
                    .map_err(|error| error.to_string())
            })
    };
    let injector = injector.borrow();
    let l10n = injector.l10n();
    let page = trash_page(&injector);
    match (cleared, page) {
        (Ok(_), Ok(page)) => Presentation::succeeded(page).with_notification(trash_info_fact(
            l10n,
            "trash.cleared_title",
            "trash.cleared_body",
        )),
        (Err(message), _) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &message)),
        (_, Err(error)) => Presentation::failed(trash_fallback_page(l10n))
            .with_notification(plan_error_fact(l10n, &error.to_string())),
    }
}

/// 校区选择页完整状态（最近使用记录 + 搜索输入）。
pub(crate) fn campus_select_page(
    injector: &ViewModelInjector,
    workspace: &WorkspaceProductionContext,
) -> Result<CampusPlanPageState, TrashPageError> {
    let snapshot = injector
        .projects()
        .campus_plan_snapshot()
        .map_err(TrashPageError::Load)?;
    let mut page = campus_plan_page(injector, workspace, snapshot, false);
    let recents = injector
        .settings()
        .recent_campuses()
        .map_err(|error| TrashPageError::Settings(error.to_string()))?;
    page.campuses = recents
        .into_iter()
        .map(|campus| CampusData {
            id: campus.id.to_string().into(),
            name: campus.name.into(),
            address: campus.address.into(),
        })
        .collect();
    Ok(page)
}

/// 当前校区方案列表页完整状态。
pub(crate) fn plan_list_page(
    injector: &ViewModelInjector,
    workspace: &WorkspaceProductionContext,
) -> Result<CampusPlanPageState, TrashPageError> {
    let snapshot = injector
        .projects()
        .campus_plan_snapshot()
        .map_err(TrashPageError::Load)?;
    Ok(campus_plan_page(injector, workspace, snapshot, true))
}

/// 回收站页完整状态。
fn trash_page(injector: &ViewModelInjector) -> Result<TrashPageState, TrashPageError> {
    let l10n = injector.l10n();
    let campus_id = current_campus_id(injector).ok_or(TrashPageError::NoCampus)?;
    let items = injector
        .projects()
        .list_trash(&campus_id)
        .map_err(TrashPageError::Load)?;
    let items = items
        .into_iter()
        .map(|item| TrashItemData {
            trash_id: item.trash_id.into(),
            plan_id: item.plan_id.into(),
            name: item.name.into(),
            campus_name: item.campus_name.into(),
            deleted_at: format_relative_time(l10n, &item.deleted_at).into(),
            expires_in: l10n
                .t_with_array("trash.remaining_days", &[&item.expires_in_days.to_string()])
                .into(),
        })
        .collect();
    Ok(TrashPageState {
        toolbar: super::toolbar(l10n, true),
        title: l10n.t("trash.page_title"),
        empty_list_text: l10n.t("trash.empty_list"),
        restore_button_text: l10n.t("trash.restore_button"),
        purge_button_text: l10n.t("trash.purge_button"),
        retention_notice_text: l10n.t("trash.retention_notice"),
        campus_prefix: l10n.t("domain.campus").to_string() + ":",
        items,
    })
}

/// 校区页数据不可读时的失败页（不显示内存默认数据）。
pub(crate) fn campus_page_fallback(
    injector: &ViewModelInjector,
    workspace: &WorkspaceProductionContext,
) -> CampusPlanPageState {
    campus_plan_page(
        injector,
        workspace,
        CampusPlanSnapshot {
            campuses: Vec::new(),
            landing_campus: None,
            plans: Vec::new(),
        },
        false,
    )
}

/// 回收站页数据不可读时的失败页。
fn trash_fallback_page(l10n: &Localization) -> TrashPageState {
    TrashPageState {
        toolbar: super::toolbar(l10n, true),
        title: l10n.t("trash.page_title"),
        empty_list_text: l10n.t("trash.empty_list"),
        restore_button_text: l10n.t("trash.restore_button"),
        purge_button_text: l10n.t("trash.purge_button"),
        retention_notice_text: l10n.t("trash.retention_notice"),
        campus_prefix: l10n.t("domain.campus").to_string() + ":",
        items: Vec::new(),
    }
}

/// 当前校区 ID（读 F3 的"上次使用的校区"，属正式数据）
fn current_campus_id(injector: &ViewModelInjector) -> Option<CampusId> {
    injector
        .projects()
        .landing_campus()
        .ok()
        .flatten()
        .and_then(|campus| CampusId::parse(&campus.id).ok())
}

/// 方案操作错误正文：同名冲突走文本键（ADR-0005），其余原样透传（ADR-0025）。
fn plan_error_body(l10n: &Localization, error: &project_management::Error) -> String {
    if error.is_duplicate_name() {
        l10n.t("plan.duplicate_name")
    } else {
        error.to_string()
    }
}

/// 回收站页构造错误。
#[derive(Debug)]
pub(crate) enum TrashPageError {
    /// 当前没有可用校区。
    NoCampus,
    /// F3 数据读取失败。
    Load(project_management::Error),
    /// F1 最近记录读取失败。
    Settings(String),
}

impl std::fmt::Display for TrashPageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrashPageError::NoCampus => write!(formatter, "no campus selected"),
            TrashPageError::Load(error) => write!(formatter, "{error}"),
            TrashPageError::Settings(message) => write!(formatter, "{message}"),
        }
    }
}

pub(crate) fn campus_info_fact(
    l10n: &Localization,
    title_key: &str,
    body_key: &str,
) -> NotificationFact {
    NotificationFact::new(Notification::info(
        l10n.t("domain.campus"),
        l10n.t(title_key),
        l10n.t(body_key),
    ))
}

pub(crate) fn campus_error_fact(l10n: &Localization, body: &str) -> NotificationFact {
    NotificationFact::new(Notification::error(
        l10n.t("domain.campus"),
        l10n.t("dialog.error_title"),
        body.to_owned(),
    ))
}

pub(crate) fn plan_error_fact(l10n: &Localization, body: &str) -> NotificationFact {
    NotificationFact::new(Notification::error(
        l10n.t("domain.plan"),
        l10n.t("dialog.error_title"),
        body.to_owned(),
    ))
}

fn trash_info_fact(l10n: &Localization, title_key: &str, body_key: &str) -> NotificationFact {
    NotificationFact::new(Notification::info(
        l10n.t("trash.page_title"),
        l10n.t(title_key),
        l10n.t(body_key),
    ))
}

// ────────────────────────────────────────────────────────────────────────────
// 校区/方案/回收站入口方法（ProductionEntries 的第二处 impl，字段同模块可见）
// ────────────────────────────────────────────────────────────────────────────

impl ProductionEntries {
    pub(crate) fn request_campus_search(&mut self, window: &AppWindow) -> bool {
        self.supersede_diagnostic(window);
        crate::map_session::hide();
        let query = window.get_campus_search_text().to_string();
        let presentation = self.campus_plan.show(
            window,
            &self.center,
            CampusPlanRequest::SearchCampus { query },
        );
        if matches!(
            presentation.operation(),
            crate::presentation::OperationState::NeedsConfirmation
        ) {
            // 搜索失败弹窗"重试/取消"（ADR-0008 第 9 条）
            self.pending_confirmation = Some(PendingConfirmation::RetryCampusSearch {
                query: window.get_campus_search_text().to_string(),
            });
        }
        let processing = matches!(
            presentation.operation(),
            crate::presentation::OperationState::Processing { .. }
        );
        processing
    }

    pub(crate) fn select_campus(&mut self, window: &AppWindow, campus_id: String) {
        self.supersede_diagnostic(window);
        crate::map_session::hide();
        let before = window.get_active_screen();
        let presentation = self.campus_plan.show(
            window,
            &self.center,
            CampusPlanRequest::SelectCampus {
                campus_id: campus_id.clone(),
            },
        );
        // 选中校区进入方案列表：记录来源页（校区选择），使返回=校区选择。
        self.record_forward_navigation(window, before);
        if matches!(
            presentation.operation(),
            crate::presentation::OperationState::NeedsConfirmation
        ) {
            // 搜索候选详情确认窗（T05 显式确认；确认后经 F1 建/选校区）
            self.pending_confirmation =
                Some(PendingConfirmation::ConfirmCampusSelection { poi_id: campus_id });
        }
    }

    pub(crate) fn remove_recent_campus(&mut self, window: &AppWindow, campus_id: String) {
        self.supersede_diagnostic(window);
        self.campus_plan.show(
            window,
            &self.center,
            CampusPlanRequest::RemoveRecentCampus { campus_id },
        );
    }

    pub(crate) fn request_create_plan(&mut self, window: &AppWindow) {
        self.pending_input = Some(PendingInput::CreatePlan);
        self.campus_plan
            .show(window, &self.center, CampusPlanRequest::RequestCreatePlan);
    }

    pub(crate) fn request_rename_plan(&mut self, window: &AppWindow, plan_id: String) {
        self.pending_input = Some(PendingInput::RenamePlan {
            plan_id: plan_id.clone(),
        });
        self.campus_plan.show(
            window,
            &self.center,
            CampusPlanRequest::RequestRenamePlan { plan_id },
        );
    }

    pub(crate) fn duplicate_plan(&mut self, window: &AppWindow, plan_id: String) {
        self.supersede_diagnostic(window);
        self.campus_plan.show(
            window,
            &self.center,
            CampusPlanRequest::DuplicatePlan { plan_id },
        );
    }

    pub(crate) fn request_delete_plan(&mut self, window: &AppWindow, plan_id: String) {
        self.pending_confirmation = Some(PendingConfirmation::DeletePlan {
            plan_id: plan_id.clone(),
        });
        self.campus_plan.show(
            window,
            &self.center,
            CampusPlanRequest::RequestDeletePlan { plan_id },
        );
    }

    pub(crate) fn show_trash(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_session::hide();
        self.trash.show(window, &self.center, TrashRequest::Show);
    }

    pub(crate) fn restore_trash_item(&mut self, window: &AppWindow, trash_id: String) {
        self.supersede_diagnostic(window);
        self.trash
            .show(window, &self.center, TrashRequest::Restore { trash_id });
    }

    pub(crate) fn request_purge_trash_item(&mut self, window: &AppWindow, trash_id: String) {
        self.pending_confirmation = Some(PendingConfirmation::PurgePlan {
            trash_id: trash_id.clone(),
        });
        self.trash.show(
            window,
            &self.center,
            TrashRequest::RequestPurge { trash_id },
        );
    }

    pub(crate) fn request_clear_trash(&mut self, window: &AppWindow) {
        self.pending_confirmation = Some(PendingConfirmation::ClearTrash);
        self.trash
            .show(window, &self.center, TrashRequest::RequestClearAll);
    }
}
