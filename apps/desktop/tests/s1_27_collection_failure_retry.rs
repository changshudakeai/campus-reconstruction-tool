//! T36 验收：采集任何失败必须弹错误对话框并给“重试”，重试重新发起同一开始意图。
//!
//! 注意：Slint 事件循环一个进程只能初始化一次，本文件只含一个建窗测试。

use data_persistence::CampusCrudApi;
use std::sync::Arc;
use std::time::{Duration, Instant};

use data_acquisition::{DataSource, OverpassDataSource};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::StdExportFileSystem;
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;

fn pump_until(
    window: &AppWindow,
    deadline: Duration,
    condition: impl Fn(&AppWindow) -> bool + 'static,
) {
    let deadline_at = Instant::now() + deadline;
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if condition(&window) || Instant::now() >= deadline_at {
                slint::quit_event_loop().expect("停止事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行事件循环");
}

fn failing_source() -> Arc<dyn DataSource + Send + Sync> {
    Arc::new(OverpassDataSource::new(Box::new(|_| {
        Err("端点全部不可达".to_owned())
    })))
}

#[test]
fn s1_27_collection_failure_error_dialog_retry() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-27.db");
    let mut injector = ViewModelInjector::new_with_collection_source(
        ShellDatabases::open(&database_path).expect("连接数据库"),
        Arc::new(StdExportFileSystem),
        failing_source(),
    )
    .expect("创建注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首次设置");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("验收校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "验收方案")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    window.invoke_workspace_step_clicked(2);

    window.invoke_collection_start_clicked();
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_error_dialog_visible()
    });
    assert!(window.get_error_dialog_visible(), "失败必须弹错误对话框");
    assert!(
        window.get_error_dialog_retry_visible(),
        "错误对话框必须提供“重试”"
    );
    assert_eq!(
        window.get_error_dialog_retry_label().as_str(),
        l10n.t("collection.retry_button")
    );

    // 点“重试”→ 重新发起同一开始意图（再次失败也再次弹窗，证明可用）
    window.invoke_error_dialog_retry_clicked();
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_error_dialog_visible() && window.get_error_dialog_retry_visible()
    });
    assert!(
        window.get_error_dialog_visible() && window.get_error_dialog_retry_visible(),
        "重试后再次失败仍应弹错误对话框 + 重试"
    );
    window.invoke_error_dialog_dismissed();
}
