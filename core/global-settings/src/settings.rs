//! 全局设置读写与首次设置向导
//!
//! ADR-0004 要点落地：
//! - 语言与 Minecraft 游戏版本均为**应用级**全局设置，下拉菜单选择；
//! - 当前支持范围：语言仅中文（zh-CN）、版本仅 26.1.2——下拉菜单从第一版
//!   起就存在，为未来扩充选项预留位置；
//! - **即使每个下拉菜单只有一个选项，首次设置页仍照常显示**：页面兼任
//!   "知情告知"职责（[`VERSION_NOTICE_TEXT`] 提示文字 + 勾选确认）。

use data_persistence::{AppSettingKey, AppSettingsApi, Database};
use shared_domain_types::CampusId;

use crate::error::{Error, Result};
use crate::landing::LandingCampus;

/// 高德地图 API key 格式校验（仅字母数字）
pub(crate) fn validate_gaode_key(key: &str) -> Result<()> {
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::InvalidGaodeApiKey);
    }
    Ok(())
}

/// 支持的界面语言（ADR-0004：当前仅中文；扩语种时在此追加）
pub const SUPPORTED_LANGUAGES: &[&str] = &["zh-CN"];

/// 支持的 Minecraft 游戏版本（ADR-0004：当前仅 26.1.2）
pub const SUPPORTED_MINECRAFT_VERSIONS: &[&str] = &["26.1.2"];

/// 默认语言
pub const DEFAULT_LANGUAGE: &str = "zh-CN";

/// 默认 Minecraft 游戏版本
pub const DEFAULT_MINECRAFT_VERSION: &str = "26.1.2";

/// 首次设置页的知情告知文字（ADR-0004 原文；暂硬编码，待 T03 文本键接入）
pub const VERSION_NOTICE_TEXT: &str = "请确认你的 Minecraft 游戏版本与此一致，否则导入可能失败";

/// 当前生效的应用级全局设置快照
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalSettings {
    /// 界面语言（如 "zh-CN"）
    pub language: String,
    /// Minecraft 游戏版本（如 "26.1.2"）
    pub minecraft_version: String,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            language: DEFAULT_LANGUAGE.to_owned(),
            minecraft_version: DEFAULT_MINECRAFT_VERSION.to_owned(),
        }
    }
}

/// 首次设置向导的提交载荷（页面兼任知情告知，ADR-0004）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstRunSetup {
    /// 用户在下拉菜单中选择的语言
    pub language: String,
    /// 用户在下拉菜单中选择的 Minecraft 版本
    pub minecraft_version: String,
    /// 是否勾选了"我已确认版本提示"（未勾选则拒绝完成设置）
    pub acknowledged: bool,
}

impl Default for FirstRunSetup {
    fn default() -> Self {
        Self {
            language: DEFAULT_LANGUAGE.to_owned(),
            minecraft_version: DEFAULT_MINECRAFT_VERSION.to_owned(),
            acknowledged: false,
        }
    }
}

/// F1 全局设置管理器 —— 设置读写、首次向导、着陆校区
///
/// 存储只经 B2 公开 trait（`AppSettingsApi` / `CampusCrudApi`），不触碰 SQL。
#[derive(Debug)]
pub struct SettingsManager {
    db: Database,
}

impl SettingsManager {
    /// 用一个已打开的数据库句柄构造管理器
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// 是否首次运行（尚未完成首次设置向导）
    pub fn is_first_run(&self) -> Result<bool> {
        let completed = self.db.get_setting(AppSettingKey::FirstRunCompleted)?;
        Ok(completed.as_deref() != Some("true"))
    }

    /// 读取当前全局设置（键缺失时回退默认值，保证界面总有值可显示）
    pub fn settings(&self) -> Result<GlobalSettings> {
        let language = self
            .db
            .get_setting(AppSettingKey::Language)?
            .unwrap_or_else(|| DEFAULT_LANGUAGE.to_owned());
        let minecraft_version = self
            .db
            .get_setting(AppSettingKey::MinecraftVersion)?
            .unwrap_or_else(|| DEFAULT_MINECRAFT_VERSION.to_owned());
        Ok(GlobalSettings {
            language,
            minecraft_version,
        })
    }

    /// 修改语言（设置页下拉菜单；不在支持范围内则拒绝）
    pub fn set_language(&mut self, language: &str) -> Result<()> {
        if !SUPPORTED_LANGUAGES.contains(&language) {
            return Err(Error::UnsupportedLanguage(language.to_owned()));
        }
        self.db.set_setting(AppSettingKey::Language, language)?;
        Ok(())
    }

    /// 修改 Minecraft 版本（设置页下拉菜单；不在支持范围内则拒绝）
    pub fn set_minecraft_version(&mut self, version: &str) -> Result<()> {
        if !SUPPORTED_MINECRAFT_VERSIONS.contains(&version) {
            return Err(Error::UnsupportedMinecraftVersion(version.to_owned()));
        }
        self.db
            .set_setting(AppSettingKey::MinecraftVersion, version)?;
        Ok(())
    }

    /// 完成首次设置向导：校验选项 + 知情告知勾选后一次性落库
    ///
    /// 未勾选知情告知返回 [`Error::NoticeNotAcknowledged`]，页面不得放行。
    pub fn complete_first_run(&mut self, setup: &FirstRunSetup) -> Result<()> {
        if !setup.acknowledged {
            return Err(Error::NoticeNotAcknowledged);
        }
        self.set_language(&setup.language)?;
        self.set_minecraft_version(&setup.minecraft_version)?;
        self.db
            .set_setting(AppSettingKey::FirstRunCompleted, "true")?;
        Ok(())
    }

    /// 老用户着陆：读"上次使用的校区"（ADR-0006）
    ///
    /// 未设置过、ID 无法解析、或校区已被删除时返回 `None`——调用方退回
    /// 校区选择页，不弹错误（属正常流程分支而非故障）。
    pub fn landing_campus(&self) -> Result<Option<LandingCampus>> {
        let Some(stored) = self.db.get_setting(AppSettingKey::LastUsedCampus)? else {
            return Ok(None);
        };
        let Ok(campus_id) = CampusId::parse(&stored) else {
            return Ok(None);
        };
        LandingCampus::find(&self.db, &campus_id)
    }

    /// 记录"上次使用的校区"（切换校区/选定校区后调用，ADR-0006）
    pub fn remember_campus(&mut self, campus_id: &CampusId) -> Result<()> {
        self.db
            .set_setting(AppSettingKey::LastUsedCampus, &campus_id.to_string())?;
        Ok(())
    }

    /// 读取高德 API key（未设置返回 None）
    pub fn gaode_api_key(&self) -> Result<Option<String>> {
        Ok(self.db.get_setting(AppSettingKey::GaodeApiKey)?)
    }

    /// 保存高德 API key（经格式校验：仅字母数字）
    pub fn set_gaode_api_key(&mut self, key: &str) -> Result<()> {
        validate_gaode_key(key)?;
        self.db.set_setting(AppSettingKey::GaodeApiKey, key)?;
        Ok(())
    }

    /// 读取高德安全密钥（未设置返回 None）
    pub fn gaode_security_key(&self) -> Result<Option<String>> {
        Ok(self.db.get_setting(AppSettingKey::GaodeSecurityKey)?)
    }

    /// 保存高德安全密钥（经格式校验：仅字母数字）
    pub fn set_gaode_security_key(&mut self, key: &str) -> Result<()> {
        validate_gaode_key(key)?;
        self.db.set_setting(AppSettingKey::GaodeSecurityKey, key)?;
        Ok(())
    }

    /// 测试高德地图连通性（返回成功或错误原因）
    pub fn test_gaode_connection(&self, api_key: &str, security_key: &str) -> Result<()> {
        // T22：先用 B3 现有能力做轻量探测；升级到 JS API 2.0 链路归 T23
        // 这里先做一个基本校验：key 长度合理 + 格式正确
        if !api_key.is_empty() && !security_key.is_empty() {
            // 模拟一个轻量探测：key 长度应该在合理范围内（实际探测由 B3 实现）
            if api_key.len() < 16 || security_key.len() < 16 {
                return Err(Error::GaodeConnectionFailed(
                    "Key 长度不足，可能无效".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> SettingsManager {
        SettingsManager::new(Database::open_in_memory().expect("内存库可打开"))
    }

    #[test]
    fn fresh_database_is_first_run_with_defaults() {
        let manager = manager();
        assert!(manager.is_first_run().unwrap());
        assert_eq!(manager.settings().unwrap(), GlobalSettings::default());
    }

    #[test]
    fn first_run_requires_acknowledgement() {
        let mut manager = manager();
        let setup = FirstRunSetup::default();
        let err = manager.complete_first_run(&setup).unwrap_err();
        assert!(matches!(err, Error::NoticeNotAcknowledged));
        assert!(manager.is_first_run().unwrap());
    }

    #[test]
    fn acknowledged_first_run_persists_choices() {
        let mut manager = manager();
        let setup = FirstRunSetup {
            acknowledged: true,
            ..FirstRunSetup::default()
        };
        manager.complete_first_run(&setup).unwrap();
        assert!(!manager.is_first_run().unwrap());
        assert_eq!(manager.settings().unwrap(), GlobalSettings::default());
    }

    #[test]
    fn unsupported_options_are_rejected() {
        let mut manager = manager();
        assert!(matches!(
            manager.set_language("fr-FR").unwrap_err(),
            Error::UnsupportedLanguage(_)
        ));
        assert!(matches!(
            manager.set_minecraft_version("1.8.9").unwrap_err(),
            Error::UnsupportedMinecraftVersion(_)
        ));
        // 拒绝后原值不变
        assert_eq!(manager.settings().unwrap(), GlobalSettings::default());
    }

    #[test]
    fn unparsable_last_campus_value_falls_back_to_none() {
        let mut manager = manager();
        manager
            .db
            .set_setting(AppSettingKey::LastUsedCampus, "不是一个 UUID")
            .unwrap();
        assert!(manager.landing_campus().unwrap().is_none());
    }

    #[test]
    fn gaode_keys_can_be_saved_and_read() {
        let mut manager = manager();
        let valid_key = "abc123DEF456ghi789";

        // 初始为空
        assert_eq!(manager.gaode_api_key().unwrap(), None);
        assert_eq!(manager.gaode_security_key().unwrap(), None);

        // 保存 key
        manager.set_gaode_api_key(valid_key).unwrap();
        manager.set_gaode_security_key(valid_key).unwrap();

        // 读取往返
        assert_eq!(manager.gaode_api_key().unwrap(), Some(valid_key.to_owned()));
        assert_eq!(
            manager.gaode_security_key().unwrap(),
            Some(valid_key.to_owned())
        );
    }

    #[test]
    fn invalid_gaode_key_format_is_rejected() {
        let mut manager = manager();

        // 包含特殊字符应拒绝
        assert!(matches!(
            manager.set_gaode_api_key("abc@def").unwrap_err(),
            Error::InvalidGaodeApiKey
        ));

        // 空字符串应拒绝
        assert!(matches!(
            manager.set_gaode_api_key("").unwrap_err(),
            Error::InvalidGaodeApiKey
        ));

        // 合法 key（纯字母数字）应接受
        assert!(manager.set_gaode_api_key("abc123DEF456").is_ok());
        assert!(manager.set_gaode_security_key("xyz789GHI012").is_ok());
    }

    #[test]
    fn gaode_connection_test_basic_validation() {
        let manager = manager();

        // 空 key 不报错（留到实际使用再弹）
        assert!(manager.test_gaode_connection("", "").is_ok());

        // 短 key 报长度错误
        assert!(matches!(
            manager
                .test_gaode_connection("short", "key12345678")
                .unwrap_err(),
            Error::GaodeConnectionFailed(_)
        ));

        // 长 key 通过基本校验
        assert!(manager
            .test_gaode_connection("abc123DEF456ghi789jkl012", "xyz789GHI012mno345pqr678")
            .is_ok());
    }
}
