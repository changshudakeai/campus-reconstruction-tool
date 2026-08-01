//! B6 国际化模块：界面文本资源管理、多语言支持、占位符插值
//!
//! **依据 ADR-0005**：所有用户可见文本外置到语言资源文件，代码只引用文本键。
//! 带变量文案使用占位符插值（如`{count}`），禁止字符串拼接。
//!
//! # 快速使用
//!
//! ## 1. 初始化全局翻译器
//! ```rust,ignore
//! use localization::{Localization, Language};
//!
//! fn main() {
//!     let mut l10n = Localization::new(Language::ZhCn).expect("Failed to load zh-CN.json");
//! }
//! ```
//!
//! ## 2. 翻译文本键
//! ```rust,ignore
//! // 简单文本
//! let text = l10n.t("review.keep");
//!
//! // 带变量的文本（自动插值）
//! let text = l10n.t_with_args("export.pending_notice", serde_json::json!({ "count": 5 }));
//! // 结果："尚有 5 项待定，它们不会被导出。"
//! ```
//!
//! ## 3. Slint 集成
//!
//! .slint 文件只声明属性，不硬编码中文；Rust 绑定层用文本键解析后注入
//!（详见 SLINT_INTEGRATION.md）：
//! ```slint,ignore
//! export component MainWindow inherits Window {
//!     in property <string> review-keep-text;  // Rust 侧 set_review_keep_text(t("review.keep"))
//!
//!     Text { text: root.review-keep-text; }
//! }
//! ```
//!
//! # 文本表结构
//!
//! `resources/zh-CN.json`包含以下类别：
//! - **domain** - 共同语言章名词（校区/方案/候选/待定/保留/剔除/封账等）
//! - **app** - 应用级文本（设置页、首屏）
//! - **plan** - 方案列表与卡片三件套
//! - **review** - 评审台按钮与状态
//! - **export** - 导出确认弹窗与进度条
//! - **collection** - 数据采集相关
//! - **audit** - 覆盖体检（安静哨兵）报告与疑点问句
//! - **tutorial** - 新手教程（跟练式气泡引导）
//! - **dialog** - 弹窗标题/正文
//! - **error** - 错误消息
//!
//! # 扩展新语种
//!
//! 新增一门语言 = 新增一个 JSON 文件 + 在 Language 枚举中添加 variant + 在 ADR-0004
//! 的语言下拉菜单中增加一项，不改动任何界面代码。
//! ```

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// 支持的语种
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// 简体中文
    #[default]
    ZhCn,
}

/// 文本资源结构
#[derive(Debug, Clone, Deserialize, Serialize)]
struct ResourceBundle {
    /// 应用级文本（首屏、设置页）
    #[serde(default)]
    pub app: HashMap<String, String>,
    /// 全局设置与首次运行向导（ADR-0004，T19B-2）
    #[serde(default)]
    pub settings: HashMap<String, String>,
    /// 方案列表与卡片
    #[serde(default)]
    pub plan: HashMap<String, String>,
    /// 评审台相关
    #[serde(default)]
    pub review: HashMap<String, String>,
    /// 导出与生成
    #[serde(default)]
    pub export: HashMap<String, String>,
    /// 采集相关
    #[serde(default)]
    pub collection: HashMap<String, String>,
    /// 覆盖体检（安静哨兵，ADR-0019）
    #[serde(default)]
    pub audit: HashMap<String, String>,
    /// 新手教程（跟练式气泡引导，ADR-0020）
    #[serde(default)]
    pub tutorial: HashMap<String, String>,
    /// 弹窗与通知
    #[serde(default)]
    pub dialog: HashMap<String, String>,
    /// 错误消息
    #[serde(default)]
    pub error: HashMap<String, String>,
    /// 公告栏与通知消息
    #[serde(default)]
    pub messages: HashMap<String, String>,
    /// 相对时间表述（ADR-0018 §一第 3 条，T19B-5A）
    #[serde(default)]
    pub time: HashMap<String, String>,
    /// 当前五步工作区占位页面。
    #[serde(default)]
    pub workspace: HashMap<String, String>,
    /// 公告栏页面。
    #[serde(default)]
    pub notice: HashMap<String, String>,
    /// 回收站页面。
    #[serde(default)]
    pub trash: HashMap<String, String>,
    /// 边界绘制页（S1-05：启用既有 boundary 段，此前被 serde 忽略）
    #[serde(default)]
    pub boundary: HashMap<String, String>,
    /// 朝向设定页（S1-05：启用既有 orientation 段，此前被 serde 忽略）
    #[serde(default)]
    pub orientation: HashMap<String, String>,
    /// 地图页
    #[serde(default)]
    pub map: HashMap<String, String>,
    /// 共同语言章名词定义（用于验证覆盖完整性）
    #[serde(default)]
    pub domain: HashMap<String, String>,
    /// 校区搜索与最近使用记录（ADR-0006，S1-04 启用既有 campus 段）
    #[serde(default)]
    pub campus: HashMap<String, String>,
}

impl ResourceBundle {
    /// 把各类别拼上前缀后展平为单一映射表（如 `review.keep`、`domain.campus`）
    fn flatten(self) -> HashMap<String, String> {
        let mut resources = HashMap::new();
        let categories = [
            ("domain", self.domain),
            ("campus", self.campus),
            ("app", self.app),
            ("settings", self.settings),
            ("plan", self.plan),
            ("review", self.review),
            ("export", self.export),
            ("collection", self.collection),
            ("audit", self.audit),
            ("tutorial", self.tutorial),
            ("dialog", self.dialog),
            ("error", self.error),
            ("messages", self.messages),
            ("time", self.time),
            ("workspace", self.workspace),
            ("notice", self.notice),
            ("trash", self.trash),
            ("boundary", self.boundary),
            ("orientation", self.orientation),
            ("map", self.map),
        ];
        for (prefix, table) in categories {
            for (key, value) in table {
                resources.insert(format!("{}.{}", prefix, key), value);
            }
        }
        resources
    }
}

/// 国际化管理器
pub struct Localization {
    language: Language,
    resources: Mutex<HashMap<String, String>>,
}

impl Localization {
    /// 创建新的本地化实例
    ///
    /// 资源查找顺序：可执行文件旁 `resources/` → 当前工作目录 `resources/`
    /// → 编译期内嵌副本兑底（保证从任意目录启动都不失败）。
    /// 文件优先于内嵌：改磁盘上的 JSON 后重启即可看到新文案（验收点）。
    pub fn new(language: Language) -> Result<Self, String> {
        let resources = match language {
            Language::ZhCn => {
                let content = Self::read_resource_file("zh-CN.json")
                    .unwrap_or_else(|| EMBEDDED_ZH_CN.to_string());
                let bundle: ResourceBundle = serde_json::from_str(&content)
                    .map_err(|e| format!("Failed to parse zh-CN.json: {}", e))?;
                bundle.flatten()
            }
        };

        Ok(Self {
            language,
            resources: Mutex::new(resources),
        })
    }

    /// 依次尝试可执行文件旁与当前目录的 resources/，都不存在则返回 None
    fn read_resource_file(file_name: &str) -> Option<String> {
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("resources").join(file_name));
            }
        }
        candidates.push(std::path::Path::new("resources").join(file_name));

        for path in candidates {
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(content) => return Some(content),
                    Err(e) => log::warn!("Failed to read {:?}: {}", path, e),
                }
            }
        }
        None
    }

    /// 获取当前语种
    pub fn language(&self) -> Language {
        self.language
    }

    /// 翻译文本键（简单情况）
    pub fn t(&self, key: &str) -> String {
        self.t_with_args(key, serde_json::Value::Null)
    }

    /// 翻译文本键并替换占位符（如 `{count}`, `{name}`）
    pub fn t_with_args(&self, key: &str, args: serde_json::Value) -> String {
        let resources = self
            .resources
            .lock()
            .expect("localization resources mutex poisoned");

        // 查找文本键
        let template = match resources.get(key) {
            Some(s) => s.clone(),
            None => {
                log::warn!("Localization key not found: {}", key);
                return key.to_string();
            }
        };

        // 如果 args 是对象，进行占位符插值
        if let serde_json::Value::Object(map) = args {
            // 先收集所有需要替换的值（避免生命周期问题）
            let replacements: Vec<(String, String)> = map
                .into_iter()
                .map(|(k, v)| {
                    let value_str = match v {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Number(n) => n.to_string(),
                        _ => v.to_string(),
                    };
                    (format!("{{{}}}", k), value_str)
                })
                .collect();

            // 依次替换
            let mut result = template;
            for (placeholder, value) in replacements {
                result = result.replace(&placeholder, &value);
            }
            result
        } else {
            template
        }
    }

    /// 批量翻译辅助方法（支持位置参数 `{0}` `{1}`）
    pub fn t_with_array(&self, key: &str, args: &[&str]) -> String {
        let resources = self
            .resources
            .lock()
            .expect("localization resources mutex poisoned");
        let template = match resources.get(key) {
            Some(s) => s.as_str(),
            None => {
                log::warn!("Localization key not found: {}", key);
                key
            }
        };

        if !args.is_empty() {
            let mut result = template.to_string();
            for (i, arg) in args.iter().enumerate() {
                let placeholder = format!("{{{}}}", i);
                result = result.replace(&placeholder, arg);
            }
            result
        } else {
            template.to_string()
        }
    }
}

/// 编译期内嵌的中文资源副本（磁盘文件缺失时的兑底，保证从任意目录启动可用）
const EMBEDDED_ZH_CN: &str = include_str!("../resources/zh-CN.json");

/// 全局翻译器单例（懒加载）
pub static GLOBAL_LOCALIZATION: Lazy<Mutex<Option<Localization>>> = Lazy::new(|| Mutex::new(None));

/// 初始化全局翻译器（必须在 UI 线程调用一次）
pub fn init_global(localization: Localization) {
    let mut global = GLOBAL_LOCALIZATION
        .lock()
        .expect("global localization mutex poisoned");
    *global = Some(localization);
}

/// 获取全局翻译器的便捷函数（必须先用 init_global 初始化）
pub fn t(key: &str) -> String {
    GLOBAL_LOCALIZATION
        .lock()
        .expect("global localization mutex poisoned")
        .as_ref()
        .expect("Global localization not initialized. Call init_global() first.")
        .t(key)
}

/// 带参数的全局翻译函数
pub fn t_with(key: &str, args: serde_json::Value) -> String {
    GLOBAL_LOCALIZATION
        .lock()
        .expect("global localization mutex poisoned")
        .as_ref()
        .expect("Global localization not initialized. Call init_global() first.")
        .t_with_args(key, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localization_creation() {
        // 注意：需要 resources/zh-CN.json 存在才能完整测试
        // 这里只是基本结构测试
        assert_eq!(Language::default(), Language::ZhCn);
    }

    #[test]
    fn test_placeholder_interpolation() {
        // 创建完整的资源映射（模拟加载后的状态）
        let mut resources = HashMap::new();

        resources.insert("domain.campus.name".to_string(), "校园名称".to_string());
        resources.insert(
            "domain.plan.status.pending".to_string(),
            "方案状态：待定".to_string(),
        );
        resources.insert("domain.review.state".to_string(), "{state}状态".to_string());

        let l10n = Localization {
            language: Language::ZhCn,
            resources: Mutex::new(resources),
        };

        // 测试简单翻译
        assert_eq!(l10n.t("domain.campus.name"), "校园名称");

        // 测试状态文本
        assert_eq!(l10n.t("domain.plan.status.pending"), "方案状态：待定");

        // 测试占位符插值
        let args = serde_json::json!({ "state": "保留" });
        assert_eq!(l10n.t_with_args("domain.review.state", args), "保留状态");
    }
}
