//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在此快照中，PR diff 可见。
//!
//! 简单方式：只检查所有公开项都可调用且行为符合快照描述。

use serde_json::json;

#[test]
fn public_api_surface_exists() {
    // Language 枚举与默认值
    let lang = localization::Language::default();
    assert_eq!(lang, localization::Language::ZhCn);

    // Localization 实例方法：t / t_with_args / t_with_array / language
    // （用不存在的 key 验证回退行为，不依赖 resources 文件的加载路径）
    // Localization::new 需要 resources/zh-CN.json；集成环境从 crate 根运行时可用
    if let Ok(l10n) = localization::Localization::new(localization::Language::ZhCn) {
        assert_eq!(l10n.language(), localization::Language::ZhCn);

        // 共同语言章名词（外置铁律：校区/方案/候选/待定/保留/剔除/封账）
        assert_eq!(l10n.t("domain.campus"), "校区");
        assert_eq!(l10n.t("domain.plan"), "方案");
        assert_eq!(l10n.t("domain.candidate"), "候选");
        assert_eq!(l10n.t("domain.pending"), "待定");
        assert_eq!(l10n.t("domain.keep"), "保留");
        assert_eq!(l10n.t("domain.reject"), "剔除");
        assert_eq!(l10n.t("domain.seal"), "封账");
        assert_eq!(l10n.t("domain.foundation"), "地基");

        // 工单验收示例键：t("review.keep")
        assert_eq!(l10n.t("review.keep"), "保留");

        // 占位符插值（ADR-0005：禁止字符串拼接）
        let text = l10n.t_with_args("export.pending_notice", json!({ "count": 3 }));
        assert_eq!(text, "尚有 3 项待定，它们不会被导出。");

        // 未定义 key 回退为 key 本身（调试标识）
        assert_eq!(l10n.t("no.such.key"), "no.such.key");
    } else {
        panic!("Localization::new 应能从 resources/zh-CN.json 加载成功");
    }

    // 全局函数存在性（init_global / t / t_with 由编译器验证签名）
    let _: fn(localization::Localization) = localization::init_global;
    let _: fn(&str) -> String = localization::t;
    let _: fn(&str, serde_json::Value) -> String = localization::t_with;
}
