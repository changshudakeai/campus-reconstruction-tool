//! 候选对象评审状态：三态
//!
//! 每个候选在评审台上只有三种状态：
//! - **待定**（刚采集回来的初始态，还没人看过）
//! - **保留**（人工确认要，唯一会被导出的状态）
//! - **剔除**（人工确认不要，不导出、无回收站）
//!
//! **状态本身就是后悔药**：点错了改点另一个状态即可，不需要撤销键。

/// 候选对象评审状态 —— 三态
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// 待定：刚采集回来，还未评审
    Pending,
    /// 保留：人工确认要，会被导出
    Keep,
    /// 剔除：人工确认不要，不导出
    Remove,
}

impl ReviewState {
    /// 返回中文显示名
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Pending => "待定",
            Self::Keep => "保留",
            Self::Remove => "剔除",
        }
    }

    /// 判断是否为保留态（会被导出）
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }

    /// 判断是否为待定态（未评审）
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// 判断是否为剔除态
    pub fn is_remove(&self) -> bool {
        matches!(self, Self::Remove)
    }

    /// 从字符串解析（用于导入/迁移）
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" | "待定" => Some(Self::Pending),
            "keep" | "保留" => Some(Self::Keep),
            "remove" | "剔除" => Some(Self::Remove),
            _ => None,
        }
    }

    /// 转为字符串表示（用于存储/传输）
    pub fn to_identifier(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Keep => "keep",
            Self::Remove => "remove",
        }
    }
}

impl serde::Serialize for ReviewState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_identifier())
    }
}

impl<'de> serde::Deserialize<'de> for ReviewState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ReviewStateVisitor;

        impl<'de> serde::de::Visitor<'de> for ReviewStateVisitor {
            type Value = ReviewState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("\"pending\"/\"keep\"/\"remove\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ReviewState::parse(value)
                    .ok_or_else(|| E::invalid_value(serde::de::Unexpected::Str(value), &self))
            }
        }

        deserializer.deserialize_str(ReviewStateVisitor)
    }
}
