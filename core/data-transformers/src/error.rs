//! 带类型错误（窗口契约章：错误是带类型的值一路向上传递）。
//!
//! 错误消息为开发者诊断文本；用户可见文案由功能模块按文本键另行处理
//! （ADR-0005，B13 自身不新增任何文本键）。

/// B13 数据转换器错误
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum TransformError {
    /// 标签映射表 JSON 解析失败
    #[error("标签映射表 JSON 解析失败: {0}")]
    InvalidConfigJson(String),
    /// 映射表引用了六类别之外的未知类别名（禁止静默丢弃规则）
    #[error("标签映射表引用了未知类别 '{0}'——规则不得被静默丢弃，请修正类别名")]
    UnknownCategory(String),
    /// 映射表引用了未知的类别文本键（国际化迁移后）
    #[error("标签映射表引用了未知的类别文本键 '{0}'，请补充到 zh-CN.json")]
    UnknownCategoryKey(String),
    /// 映射表没有任何规则
    #[error("标签映射表为空：所有对象都将归入'其他'，请先定义规则")]
    EmptyRuleSet,
    /// 某条规则没有任何标签
    #[error("类别 '{0}' 的规则条目没有任何标签——空规则等同静默丢弃")]
    EmptyTagList(String),
    /// 某个类别缺少规则（五大类每类至少一条）
    #[error("类别 '{0}' 没有任何标签规则——该类对象将全部落入'其他'")]
    MissingCategoryRules(String),
}
