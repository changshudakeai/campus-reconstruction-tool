//! 唯一标识符类型
//!
//! `CampusId` 和 `PlanId` 基于 Uuid v4（随机 UUID），用于跨进程、重启后依然稳定的
//! 引用。两者在类型层面区分，避免混用。

use uuid::Uuid;

impl serde::Serialize for CampusId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for CampusId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct CampusIdVisitor;

        impl<'de> serde::de::Visitor<'de> for CampusIdVisitor {
            type Value = CampusId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid UUID")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                CampusId::parse(value).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(CampusIdVisitor)
    }
}

/// 校区 ID —— 一所学校的唯一标识
///
/// 通过高德地图搜索选定校区后生成，不对外公开暴露内部结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CampusId(Uuid);

impl CampusId {
    /// 生成新的随机校区 ID
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// 从字符串解析
    pub fn parse(s: &str) -> Result<Self, ParseIdError> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|e| ParseIdError(e.to_string()))
    }

    /// 转为字符串表示
    pub fn as_hyphenated(&self) -> uuid::fmt::Hyphenated {
        self.0.into()
    }

    /// 转为简单字符串（无连字符）
    pub fn as_simple(&self) -> uuid::fmt::Simple {
        self.0.into()
    }
}

impl std::fmt::Display for CampusId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_hyphenated())
    }
}

/// 方案 ID —— 某校区下某个复刻计划的唯一标识
///
/// 同校区内方案名不可重复，但 ID 全局唯一。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlanId(Uuid);

impl PlanId {
    /// 生成新的随机方案 ID
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// 从字符串解析
    pub fn parse(s: &str) -> Result<Self, ParseIdError> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|e| ParseIdError(e.to_string()))
    }

    /// 转为字符串表示
    pub fn as_hyphenated(&self) -> uuid::fmt::Hyphenated {
        self.0.into()
    }

    /// 转为简单字符串（无连字符）
    pub fn as_simple(&self) -> uuid::fmt::Simple {
        self.0.into()
    }
}

impl std::fmt::Display for PlanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_hyphenated())
    }
}

impl serde::Serialize for PlanId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for PlanId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PlanIdVisitor;

        impl<'de> serde::de::Visitor<'de> for PlanIdVisitor {
            type Value = PlanId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid UUID")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                PlanId::parse(value).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(PlanIdVisitor)
    }
}

/// ID 解析错误
#[derive(Debug, thiserror::Error)]
#[error("无效 ID 格式：{0}")]
pub struct ParseIdError(String);
