//! 边界与朝向
//!
//! - **边界**：在地图上圈画的方案范围（将来扩展为几何多边形）
//! - **朝向**：在高德地图上画两点参考线确定，没有默认值、不可跳过

use serde::{Deserialize, Serialize};

/// 方案边界 —— 暂用 JSON 字符串承载 GeoJSON Polygon/MultiPolygon
///
/// 将来可扩展为专用几何类型，但目前先用标准 GeoJSON 格式存储。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Boundary {
    /// GeoJSON type: "Polygon" or "MultiPolygon"
    pub r#type: String,
    /// Coordinates array
    pub coordinates: serde_json::Value,
}

impl Boundary {
    /// 新建一个空边界（待用户绘制）
    pub fn empty() -> Self {
        Self {
            r#type: "Polygon".to_string(),
            coordinates: serde_json::json!([[]]),
        }
    }

    /// 判断是否为空（未绘制）
    pub fn is_empty(&self) -> bool {
        self.coordinates
            .as_array()
            .map(|arr| {
                arr.is_empty()
                    || arr
                        .iter()
                        .all(|a| a.as_array().map(|r| r.is_empty()).unwrap_or(true))
            })
            .unwrap_or(true)
    }
}

impl Default for Boundary {
    fn default() -> Self {
        Self::empty()
    }
}

/// 朝向：由高德地图两点参考线计算出的方位角（0~360 度）
///
/// 单位：度，北为 0°/360°，顺时针增加。**无默认值**，必须由用户在地图上明确设定。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
pub struct Orientation(f32);

impl Orientation {
    /// 新建朝向（超出 0~360 返回 None）
    pub fn new(degree: f32) -> Option<Self> {
        if !(0.0..=360.0).contains(&degree) {
            return None;
        }
        Some(Self(degree))
    }

    /// 从字符串解析（导入/迁移）
    pub fn parse(s: &str) -> Option<Self> {
        s.parse::<f32>().ok().and_then(Self::new)
    }

    /// 返回方位角（0~360）
    pub fn degree(&self) -> f32 {
        self.0
    }

    /// 返回简写方向名（N/E/S/W）
    pub fn cardinal_direction(&self) -> &'static str {
        let deg = self.0 % 360.0;
        if !(22.5..337.5).contains(&deg) {
            "N"
        } else if deg < 67.5 {
            "NE"
        } else if deg < 112.5 {
            "E"
        } else if deg < 157.5 {
            "SE"
        } else if deg < 202.5 {
            "S"
        } else if deg < 247.5 {
            "SW"
        } else if deg < 292.5 {
            "W"
        } else {
            "NW"
        }
    }
}

impl std::fmt::Display for Orientation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}° ({})", self.0, self.cardinal_direction())
    }
}

impl<'de> Deserialize<'de> for Orientation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| {
            serde::de::Error::custom(format!("Orientation out of range 0..=360: {}", value))
        })
    }
}
