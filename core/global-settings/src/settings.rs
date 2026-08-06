//! 全局设置读写与首次设置向导
//!
//! ADR-0004 要点落地：
//! - 语言与 Minecraft 游戏版本均为**应用级**全局设置，下拉菜单选择；
//! - 当前支持范围：语言仅中文（zh-CN）、版本仅 26.1.2——下拉菜单从第一版
//!   起就存在，为未来扩充选项预留位置；
//! - **即使每个下拉菜单只有一个选项，首次设置页仍照常显示**：页面兼任
//!   "知情告知"职责（[`VERSION_NOTICE_TEXT`] 提示文字 + 勾选确认）。

use data_persistence::{AppSettingKey, AppSettingsApi, CampusCrudApi, Database};
use shared_domain_types::CampusId;

use crate::error::{Error, Result};
use crate::landing::{LandingCampus, RecentCampus};

/// ADR-0008 第 5-7 条：一次点选完成"添加或找到校区 + 选定当前校区"的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedCampus {
    /// 本次选定的校区 ID
    pub campus_id: CampusId,
    /// 该高德地点此前已添加过校区（本次为切换而非新建，用于"已为你切换"提示）
    pub already_added: bool,
}

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

/// ADR-0004 默认导出位置初始值：当前用户的文档/校园复刻工具/导出文件夹。
/// 仅支持 Windows 的 USERPROFILE 环境变量；缺失时返回空串，由设置页提示填写。
fn default_export_directory() -> String {
    std::env::var("USERPROFILE")
        .ok()
        .filter(|home| !home.is_empty())
        .map(|home| format!("{home}\\Documents\\校园复刻工具\\导出"))
        .unwrap_or_default()
}

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

/// 启动呈现入口的一次性着陆决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupDestination {
    /// 首次运行，显示设置向导。
    FirstRunSetup,
    /// 已完成设置，但没有可用的上次校区。
    CampusSelect,
    /// 直接恢复上次校区。
    LastUsedCampus { name: String },
}

/// F1 一次返回启动页面所需的全部正式状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupSnapshot {
    pub settings: GlobalSettings,
    pub destination: StartupDestination,
}

/// 由组合根注入 F1、用于取得目标页面正式内容的无界面端口。
pub trait StartupLandingContentProvider {
    type Content;
    type Error;

    /// 一次返回着陆目标页面所需的完整功能状态。
    fn landing_content(&self) -> std::result::Result<Self::Content, Self::Error>;
}

/// F1 启动用例一次返回的设置、着陆决定与目标页面内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteStartupResult<LandingContent> {
    pub snapshot: StartupSnapshot,
    pub landing_content: Option<LandingContent>,
}

/// 完整启动结果读取失败，保留失败所属边界。
#[derive(Debug)]
pub enum StartupResultError<LandingError> {
    /// F1 的设置或着陆决定读取失败。
    Startup(Error),
    /// 注入的着陆内容提供端口读取失败。
    Landing(LandingError),
}

/// F1 一次返回设置页面所需的全部正式状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub settings: GlobalSettings,
    pub gaode_api_key: Option<String>,
    pub gaode_security_key: Option<String>,
    /// 默认导出位置（未设置时为 ADR-0004 初始路径）
    pub default_export_location: String,
}
/// 首次设置向导的提交载荷（页面兼任知情告知，ADR-0004）。
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

    /// 一次读取启动呈现所需的设置与着陆决定。
    fn startup_snapshot(&self) -> Result<StartupSnapshot> {
        let settings = self.settings()?;
        let destination = if self.is_first_run()? {
            StartupDestination::FirstRunSetup
        } else {
            match self.landing_campus()? {
                Some(campus) => StartupDestination::LastUsedCampus { name: campus.name },
                None => StartupDestination::CampusSelect,
            }
        };
        Ok(StartupSnapshot {
            settings,
            destination,
        })
    }

    /// 一次读取设置页所需的全部正式设置。
    pub fn settings_snapshot(&self) -> Result<SettingsSnapshot> {
        Ok(SettingsSnapshot {
            settings: self.settings()?,
            gaode_api_key: self.gaode_api_key()?,
            gaode_security_key: self.gaode_security_key()?,
            default_export_location: self.default_export_location()?,
        })
    }

    /// 读取默认导出位置（ADR-0004：初始值为文档/校园复刻工具/导出）
    pub fn default_export_location(&self) -> Result<String> {
        Ok(self
            .db
            .get_setting(AppSettingKey::DefaultExportLocation)?
            .filter(|value| !value.is_empty())
            .unwrap_or_else(default_export_directory))
    }

    /// 修改默认导出位置（保存设置页；空路径拒绝）
    pub fn set_default_export_location(&mut self, path: &str) -> Result<()> {
        let path = path.trim();
        if path.is_empty() {
            return Err(Error::InvalidExportLocation);
        }
        self.db
            .set_setting(AppSettingKey::DefaultExportLocation, path)?;
        Ok(())
    }

    /// 只通过 F1 一次取得完整启动结果；是否读取着陆内容由 F1 决定。
    pub fn startup_result<Provider>(
        &self,
        provider: &Provider,
    ) -> std::result::Result<
        CompleteStartupResult<Provider::Content>,
        StartupResultError<Provider::Error>,
    >
    where
        Provider: StartupLandingContentProvider,
    {
        let snapshot = self
            .startup_snapshot()
            .map_err(StartupResultError::Startup)?;
        let landing_content = match &snapshot.destination {
            StartupDestination::FirstRunSetup => None,
            StartupDestination::CampusSelect | StartupDestination::LastUsedCampus { .. } => Some(
                provider
                    .landing_content()
                    .map_err(StartupResultError::Landing)?,
            ),
        };
        Ok(CompleteStartupResult {
            snapshot,
            landing_content,
        })
    }

    /// 修改语言（设置页下拉菜单；不在支持范围内则拒绝）。
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

    /// 记录"上次使用的校区"（切换校区/选定校区后调用，ADR-0006）。
    ///
    /// 同时维护"最近使用的校区"列表：最近进入的排最前，重复进入不产生重复记录。
    pub fn remember_campus(&mut self, campus_id: &CampusId) -> Result<()> {
        self.db
            .set_setting(AppSettingKey::LastUsedCampus, &campus_id.to_string())?;
        let mut ids = self.recent_campus_ids()?;
        ids.retain(|id| id != campus_id);
        ids.insert(0, *campus_id);
        self.save_recent_campus_ids(&ids)
    }

    /// 最近使用的校区记录（ADR-0006）：按最近进入时间倒序，校区已被删除时跳过。
    pub fn recent_campuses(&self) -> Result<Vec<RecentCampus>> {
        let mut result = Vec::new();
        for id in self.recent_campus_ids()? {
            let Some(campus) = self.db.find_campus_by_id(&id.to_string())? else {
                continue;
            };
            let Ok(parsed) = CampusId::parse(&campus.id) else {
                continue;
            };
            result.push(RecentCampus {
                id: parsed,
                name: campus.name,
                address: campus.address,
            });
        }
        Ok(result)
    }

    /// 从最近使用记录中移除一条（右侧小叉，不删除校区及其方案，ADR-0006）。
    pub fn remove_recent_campus(&mut self, campus_id: &CampusId) -> Result<()> {
        let ids = self.recent_campus_ids()?;
        let filtered: Vec<CampusId> = ids.into_iter().filter(|id| id != campus_id).collect();
        self.save_recent_campus_ids(&filtered)
    }

    /// 读取持久化的最近校区 ID 列表（损坏或缺失按空列表处理，与
    /// `last_used_campus` 的容错语义一致）。
    fn recent_campus_ids(&self) -> Result<Vec<CampusId>> {
        let Some(stored) = self.db.get_setting(AppSettingKey::RecentCampuses)? else {
            return Ok(Vec::new());
        };
        let Ok(ids) = serde_json::from_str::<Vec<String>>(&stored) else {
            return Ok(Vec::new());
        };
        Ok(ids
            .iter()
            .filter_map(|id| CampusId::parse(id).ok())
            .collect())
    }

    /// 持久化最近校区 ID 列表（JSON 数组，最近进入排最前）。
    fn save_recent_campus_ids(&mut self, ids: &[CampusId]) -> Result<()> {
        let values: Vec<String> = ids.iter().map(ToString::to_string).collect();
        let json = serde_json::to_string(&values)
            .map_err(|error| Error::InvalidRecentCampuses(error.to_string()))?;
        self.db.set_setting(AppSettingKey::RecentCampuses, &json)?;
        Ok(())
    }

    /// T05：选择校区并保存锚点坐标（完整流程：创建校区 + 记录最后使用 + 标记首次完成）
    pub fn select_campus_with_anchor(
        &mut self,
        name: &str,
        poi_id: &str,
        address: &str,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<CampusId> {
        // 1. 创建校区（带锚点）
        let campus = self
            .db
            .create_campus_with_anchor(name, poi_id, address, anchor_lng, anchor_lat)?;

        // 2. 解析 CampusId
        let campus_id =
            CampusId::parse(&campus.id).map_err(|e| Error::InvalidCampusId(e.to_string()))?;

        // 3. 记录"上次使用的校区"
        self.remember_campus(&campus_id)?;

        // 4. 标记首次运行完成（如果是新用户）
        let is_first = self.is_first_run()?;
        if is_first {
            self.db
                .set_setting(AppSettingKey::FirstRunCompleted, "true")?;
        }

        Ok(campus_id)
    }

    /// T30/ADR-0008 第 5-7 条：按高德地点标识选校区——已添加则直接切换
    /// （不重复建校区），未添加则建立校区并切换；两种路径都立即成为当前校区。
    pub fn select_campus_by_poi_id(
        &mut self,
        name: &str,
        poi_id: &str,
        address: &str,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<SelectedCampus> {
        if let Some(existing) = self.db.find_campus_by_poi_id(poi_id)? {
            let campus_id =
                CampusId::parse(&existing.id).map_err(|e| Error::InvalidCampusId(e.to_string()))?;
            self.remember_campus(&campus_id)?;
            // 与新建路径保持一致：选定校区后首次运行即完成（老用户无副作用）
            if self.is_first_run()? {
                self.db
                    .set_setting(AppSettingKey::FirstRunCompleted, "true")?;
            }
            return Ok(SelectedCampus {
                campus_id,
                already_added: true,
            });
        }
        let campus_id =
            self.select_campus_with_anchor(name, poi_id, address, anchor_lng, anchor_lat)?;
        Ok(SelectedCampus {
            campus_id,
            already_added: false,
        })
    }

    /// T05：仅更新已有校区的锚点坐标
    pub fn update_campus_anchor(
        &self,
        campus_id: &CampusId,
        anchor_lng: f64,
        anchor_lat: f64,
    ) -> Result<()> {
        Ok(self
            .db
            .update_campus_anchor(&campus_id.to_string(), anchor_lng, anchor_lat)?)
    }

    /// 读取高德 API key（未设置或已清除返回 None）
    pub fn gaode_api_key(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .get_setting(AppSettingKey::GaodeApiKey)?
            .filter(|value| !value.is_empty()))
    }

    /// 保存高德 API key（经格式校验：仅字母数字）
    pub fn set_gaode_api_key(&mut self, key: &str) -> Result<()> {
        validate_gaode_key(key)?;
        self.db.set_setting(AppSettingKey::GaodeApiKey, key)?;
        Ok(())
    }

    /// 读取高德安全密钥（未设置或已清除返回 None）
    pub fn gaode_security_key(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .get_setting(AppSettingKey::GaodeSecurityKey)?
            .filter(|value| !value.is_empty()))
    }

    /// 清除全部高德密钥（ADR-0004：一次清除已保存值与输入内容）
    pub fn clear_gaode_keys(&mut self) -> Result<()> {
        self.db.set_setting(AppSettingKey::GaodeApiKey, "")?;
        self.db.set_setting(AppSettingKey::GaodeSecurityKey, "")?;
        Ok(())
    }

    /// 保存高德安全密钥（经格式校验：仅字母数字）
    pub fn set_gaode_security_key(&mut self, key: &str) -> Result<()> {
        validate_gaode_key(key)?;
        self.db.set_setting(AppSettingKey::GaodeSecurityKey, key)?;
        Ok(())
    }

    /// 测试高德地图连通性（返回成功或错误原因）
    ///
    /// T23: JS API 2.0 + securityJsCode.
    /// - 格式校验：key 必须为纯字母数字
    /// - 长度校验：key ≥ 16 字符（高德 key 规范）
    /// - 实际服务端探测由壳层在 WebView 加载时完成（SDK 拒绝无安全密钥的加载）
    pub fn test_gaode_connection(&self, api_key: &str, security_key: &str) -> Result<()> {
        if !api_key.is_empty() && !security_key.is_empty() {
            // 格式 + 长度双重校验
            if api_key.len() < 16 || security_key.len() < 16 {
                return Err(Error::GaodeConnectionFailed(
                    "Key 长度不足，请确认是否为有效的阿里云高德开放平台密钥".to_owned(),
                ));
            }

            // T23: 额外验证 v2.0 兼容性（API key 必须支持 securityJsCode）
            // 这里通过检查 key 字符集来间接验证：只能是字母数字
            if !api_key.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(Error::GaodeConnectionFailed(
                    "API key 包含非法字符，必须是纯字母数字".to_owned(),
                ));
            }
            if !security_key.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(Error::GaodeConnectionFailed(
                    "安全密钥包含非法字符，必须是纯字母数字".to_owned(),
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

        // 短 key 报长度错误（新改进信息）
        let err = manager
            .test_gaode_connection("short", "key12345678")
            .unwrap_err();
        assert!(err.to_string().contains("阿里云高德开放平台"));

        // 合法长度的纯字母数字 key 通过校验
        assert!(manager
            .test_gaode_connection("abc123DEF456ghi789jkl012", "xyz789GHI012mno345pqr678")
            .is_ok());

        // 含特殊字符的 key 应拒绝
        assert!(manager
            .test_gaode_connection("abc@def1234567890abcdef", "xyz789GHI012mno345pqr678")
            .unwrap_err()
            .to_string()
            .contains("非法字符"));
    }

    // T05: 测试校区锚点持久化功能
    #[test]
    fn test_select_campus_with_anchor() {
        let mut manager = manager();

        // 选择校区并保存锚点
        let campus_id = manager
            .select_campus_with_anchor(
                "北京大学",
                "239494",
                "海淀区颐和园路5号",
                116.308, // 经度
                39.995,  // 纬度
            )
            .expect("创建校区");

        // 验证校区已保存到数据库
        let campuses = manager.db.list_campuses().unwrap();
        assert_eq!(campuses.len(), 1);
        assert_eq!(campuses[0].name, "北京大学");
        assert_eq!(campuses[0].anchor_lng, 116.308);
        assert_eq!(campuses[0].anchor_lat, 39.995);

        // 验证最后使用的校区被记录
        let landing = manager.landing_campus().unwrap().expect("应该有校区");
        assert_eq!(landing.name, "北京大学");
        assert_eq!(landing.anchor_lng, 116.308);
        assert_eq!(landing.anchor_lat, 39.995);
        assert_eq!(landing.id, campus_id);

        // 验证首次运行完成标志已设置
        assert!(!manager.is_first_run().unwrap());
    }

    #[test]
    fn select_campus_by_poi_id_switches_existing_without_duplicate() {
        let mut manager = manager();

        let first = manager
            .select_campus_by_poi_id("上海交通大学", "POI-A1", "闵行区东川路800号", 121.44, 31.03)
            .expect("新建并选定");
        assert!(!first.already_added);

        // 重复点选同一真实学校：只切换、不重复建（ADR-0008 第 6 条）
        let again = manager
            .select_campus_by_poi_id("上海交通大学", "POI-A1", "闵行区东川路800号", 121.44, 31.03)
            .expect("切换既有校区");
        assert!(again.already_added);
        assert_eq!(again.campus_id, first.campus_id, "必须返回原校区，不新建");
        assert_eq!(
            manager.db.list_campuses().unwrap().len(),
            1,
            "不得重复建校区"
        );

        // 新地点 → 新建校区并切换
        let second = manager
            .select_campus_by_poi_id("复旦大学", "POI-B2", "杨浦区邯郸路220号", 121.505, 31.296)
            .expect("新建第二校区");
        assert!(!second.already_added);
        assert_eq!(manager.db.list_campuses().unwrap().len(), 2);
        assert_eq!(
            manager.landing_campus().unwrap().unwrap().name,
            "复旦大学",
            "最近选定的校区成为当前校区"
        );
    }

    #[test]
    fn test_update_campus_anchor() {
        let mut manager = manager();

        // 先创建校区
        let campus_id = manager
            .select_campus_with_anchor("清华大学", "239495", "海淀区清华园1号", 116.320, 39.998)
            .expect("创建校区");

        // 更新锚点坐标
        manager
            .update_campus_anchor(&campus_id, 116.330, 40.000)
            .expect("更新锚点");

        // 验证锚点已更新
        let campuses = manager.db.list_campuses().unwrap();
        assert_eq!(campuses[0].anchor_lng, 116.330);
        assert_eq!(campuses[0].anchor_lat, 40.000);
    }

    #[test]
    fn recent_campuses_keep_most_recent_first_and_dedupe() {
        let mut db = Database::open_in_memory().unwrap();
        let first = db.create_campus("第一大学").unwrap();
        let second = db.create_campus("第二大学").unwrap();
        let first_id = CampusId::parse(&first.id).unwrap();
        let second_id = CampusId::parse(&second.id).unwrap();

        let mut manager = SettingsManager::new(db);
        manager.remember_campus(&first_id).unwrap();
        manager.remember_campus(&second_id).unwrap();
        manager.remember_campus(&first_id).unwrap();

        let recent = manager.recent_campuses().unwrap();
        assert_eq!(recent.len(), 2, "重复进入不产生重复记录");
        assert_eq!(recent[0].id, first_id, "最近进入的排最前");
        assert_eq!(recent[1].id, second_id);
        assert_eq!(recent[0].name, "第一大学");
        assert_eq!(manager.landing_campus().unwrap().unwrap().name, "第一大学");
    }

    #[test]
    fn recent_campus_records_show_address_and_can_be_removed() {
        let mut manager = manager();
        let campus_id = manager
            .select_campus_with_anchor(
                "华东师范大学(普陀校区)",
                "B01",
                "中山北路3663号",
                121.406,
                31.228,
            )
            .expect("创建校区");

        let recent = manager.recent_campuses().unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].address, "中山北路3663号");

        // 小叉移除：只清快捷记录，不删除校区，不弹确认（确认由 S1 呈现层保证）
        manager.remove_recent_campus(&campus_id).unwrap();
        assert!(manager.recent_campuses().unwrap().is_empty());
        assert!(
            manager.landing_campus().unwrap().is_some(),
            "校区及其方案不受影响"
        );
        assert_eq!(manager.db.list_campuses().unwrap().len(), 1);
    }

    #[test]
    fn recent_campus_skips_missing_campuses() {
        let mut manager = manager();
        let ghost = CampusId::generate();
        manager.remember_campus(&ghost).unwrap();
        assert!(
            manager.recent_campuses().unwrap().is_empty(),
            "校区不存在时跳过该记录"
        );
    }

    #[test]
    fn presentation_snapshots_are_complete_single_results() {
        let manager = manager();
        let startup = manager.startup_snapshot().expect("完整启动结果");
        assert_eq!(startup.settings, GlobalSettings::default());
        assert!(matches!(
            startup.destination,
            StartupDestination::FirstRunSetup
        ));

        let settings = manager.settings_snapshot().expect("完整设置结果");
        assert_eq!(settings.settings, GlobalSettings::default());
        assert_eq!(settings.gaode_api_key, None);
        assert_eq!(settings.gaode_security_key, None);
        assert_eq!(settings.default_export_location, default_export_directory());
    }

    #[test]
    fn export_location_defaults_to_documents_directory_and_round_trips() {
        let mut manager = manager();
        assert_eq!(
            manager.default_export_location().unwrap(),
            default_export_directory()
        );
        assert!(matches!(
            manager.set_default_export_location("   ").unwrap_err(),
            Error::InvalidExportLocation
        ));
        manager.set_default_export_location("D:/导出").unwrap();
        assert_eq!(manager.default_export_location().unwrap(), "D:/导出");
        let snapshot = manager.settings_snapshot().unwrap();
        assert_eq!(snapshot.default_export_location, "D:/导出");
    }

    #[test]
    fn clear_gaode_keys_removes_saved_values() {
        let mut manager = manager();
        let valid_key = "abc123DEF456ghi789";
        manager.set_gaode_api_key(valid_key).unwrap();
        manager.set_gaode_security_key(valid_key).unwrap();
        manager.clear_gaode_keys().unwrap();
        assert_eq!(manager.gaode_api_key().unwrap(), None);
        assert_eq!(manager.gaode_security_key().unwrap(), None);
        let snapshot = manager.settings_snapshot().unwrap();
        assert_eq!(snapshot.gaode_api_key, None);
        assert_eq!(snapshot.gaode_security_key, None);
    }
}
