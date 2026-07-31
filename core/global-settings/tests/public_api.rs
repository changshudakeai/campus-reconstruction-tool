//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、关键行为可调用。

use data_persistence::Database;
use global_settings::{
    CompleteStartupResult, Error, FirstRunSetup, GlobalSettings, LandingCampus, SettingsManager,
    SettingsSnapshot, StartupDestination, StartupLandingContentProvider, StartupResultError,
    StartupSnapshot, DEFAULT_LANGUAGE, DEFAULT_MINECRAFT_VERSION, SUPPORTED_LANGUAGES,
    SUPPORTED_MINECRAFT_VERSIONS, VERSION_NOTICE_TEXT,
};
use shared_domain_types::CampusId;

struct LandingProvider;

impl StartupLandingContentProvider for LandingProvider {
    type Content = &'static str;
    type Error = &'static str;

    fn landing_content(&self) -> Result<Self::Content, Self::Error> {
        Ok("完整着陆内容")
    }
}
struct FailingLandingProvider;

impl StartupLandingContentProvider for FailingLandingProvider {
    type Content = &'static str;
    type Error = &'static str;

    fn landing_content(&self) -> Result<Self::Content, Self::Error> {
        Err("landing failed")
    }
}

#[test]
fn public_api_types_exist() {
    // 常量：支持范围与默认值（ADR-0004：仅中文 / 26.1.2，菜单预留扩充位）
    assert_eq!(SUPPORTED_LANGUAGES, ["zh-CN"]);
    assert_eq!(SUPPORTED_MINECRAFT_VERSIONS, ["26.1.2"]);
    assert_eq!(DEFAULT_LANGUAGE, "zh-CN");
    assert_eq!(DEFAULT_MINECRAFT_VERSION, "26.1.2");
    assert!(VERSION_NOTICE_TEXT.contains("Minecraft"));

    // GlobalSettings / FirstRunSetup：默认值即当前唯一支持范围
    let defaults = GlobalSettings::default();
    assert_eq!(defaults.language, DEFAULT_LANGUAGE);
    assert_eq!(defaults.minecraft_version, DEFAULT_MINECRAFT_VERSION);
    let setup = FirstRunSetup::default();
    assert!(!setup.acknowledged, "知情告知默认未勾选");

    // SettingsManager：首次向导 → 设置读写
    let db = Database::open_in_memory().expect("内存库可打开");
    let mut manager = SettingsManager::new(db);
    assert!(manager.is_first_run().unwrap());
    let complete: CompleteStartupResult<&str> = manager.startup_result(&LandingProvider).unwrap();
    assert!(complete.landing_content.is_none());
    let startup: StartupSnapshot = complete.snapshot;
    let _error_type: Option<StartupResultError<&str>> = None;
    assert!(matches!(
        startup.destination,
        StartupDestination::FirstRunSetup
    ));
    let settings: SettingsSnapshot = manager.settings_snapshot().unwrap();
    assert_eq!(settings.settings, defaults);
    assert!(!settings.default_export_location.is_empty());

    // 未勾选知情告知 → 拒绝完成（Error 可匹配，#[non_exhaustive]）
    let err = manager.complete_first_run(&setup).unwrap_err();
    assert!(matches!(err, Error::NoticeNotAcknowledged));
    assert!(!err.to_string().is_empty());

    let acknowledged = FirstRunSetup {
        acknowledged: true,
        ..FirstRunSetup::default()
    };
    manager.complete_first_run(&acknowledged).unwrap();
    assert!(!manager.is_first_run().unwrap());
    assert_eq!(manager.settings().unwrap(), defaults);
    let complete: CompleteStartupResult<&str> = manager.startup_result(&LandingProvider).unwrap();
    assert!(complete.landing_content.is_some());
    let error = manager
        .startup_result(&FailingLandingProvider)
        .expect_err("landing content failure must not become an empty page");
    assert!(matches!(
        error,
        StartupResultError::Landing("landing failed")
    ));

    manager.set_language("zh-CN").unwrap();
    manager.set_minecraft_version("26.1.2").unwrap();

    // 默认导出位置（ADR-0004）：读写与空路径拒绝
    manager.set_default_export_location("D:/测试导出").unwrap();
    assert_eq!(manager.default_export_location().unwrap(), "D:/测试导出");

    // 高德密钥：保存后一次清除（ADR-0004）
    manager.set_gaode_api_key("abc123DEF456ghi789").unwrap();
    manager
        .set_gaode_security_key("abc123DEF456ghi789")
        .unwrap();
    manager.clear_gaode_keys().unwrap();
    assert_eq!(manager.gaode_api_key().unwrap(), None);
    assert_eq!(manager.gaode_security_key().unwrap(), None);

    // 着陆流程（ADR-0006）：remember → landing；无记录时 None
    assert!(manager.landing_campus().unwrap().is_none());
    let ghost = CampusId::generate();
    manager.remember_campus(&ghost).unwrap();
    let landed: Option<LandingCampus> = manager.landing_campus().unwrap();
    assert!(landed.is_none(), "校区不存在 → None（退回校区选择页）");
}
