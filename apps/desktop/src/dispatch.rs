//! T19B-1 —— 回调错误统一出口（壳内零业务逻辑的错误处理纪律）。
//!
//! Slint 回调闭包内调用 VM 方法返回的 `Result` 错误一律递到这里，
//! 按弹窗铁律（ADR-0021）经 B7 `error()` 模态弹窗 + 公告栏留底；
//! 壳不自行判断错误轻重、不静默吞错。B7 全局单例由
//! [`crate::run_dev`] 启动时初始化；Slint Presenter（弹窗/toast 声明）
//! 随后续 T19B 工单注册，注册前消息照常留底不丢。

use localization::Localization;

/// 把回调错误分派给 B7（模态弹窗 + 公告栏留底）。
///
/// `error` 是带类型错误的显示形式；来源标签与标题走 B6 文本键
/// （`app.source_tag` / `dialog.error_title`），错误详情原样透传
/// （不隐藏、不吞异常，ADR-0025 错误码转换行约束）。
pub fn report_callback_error(l10n: &Localization, error: &dyn std::fmt::Display) {
    notification_center::error(
        l10n.t("app.source_tag"),
        l10n.t("dialog.error_title"),
        error.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use localization::Language;
    use notification_center::{NotificationCenter, PresenterRegistry};

    use super::*;

    #[test]
    fn callback_error_reaches_b7_board() {
        // init 幂等：测试进程内首次调用即建立全局一本账
        let center = NotificationCenter::init(PresenterRegistry::new());
        let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");

        let error_text = "演示错误：数据库暂不可用";
        report_callback_error(&l10n, &error_text);

        assert!(
            center.board_snapshot().iter().any(|n| n.body == error_text),
            "回调错误应留底进 B7 公告栏"
        );
    }
}
