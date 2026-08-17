//! 地图页面可序列化、可验证并可在安全重建后恢复的视野值。

use serde::{Deserialize, Serialize};

/// 可跨 WebView 安全重建恢复的高德地图视野（GCJ-02 中心 + 缩放级别）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapViewport {
    pub longitude: f64,
    pub latitude: f64,
    pub zoom: f64,
}

impl MapViewport {
    pub fn new(longitude: f64, latitude: f64, zoom: f64) -> Self {
        Self {
            longitude,
            latitude,
            zoom,
        }
    }
}
