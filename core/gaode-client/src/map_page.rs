//! WebView 地图页生成
//!
//! 高德地图 JS SDK 必须通过**官方 CDN** 加载（决策记忆：v1.2，
//! `https://webapi.amap.com/maps?v=1.2&key=...`，无内部镜像）；
//! 地图容器最小高度 **300px**（决策记忆：适用于所有嵌入式地图场景）。
//!
//! 本模块只生成静态 HTML 文本；由壳层 WebView 加载渲染。JS 侧通过
//! `window.mcrebuildBridge.postMessage(json)` 把地点搜索结果回传宿主，
//! 宿主再交 [`crate::parse_place_search_response`] 解析。
//!
//! T21: 升级为 JS API 2.0 + securityJsCode（高德 2.0 强制要求）。

use crate::error::{Error, Result};

/// 官方 CDN 地址模板（{key}处填入高德 Web API key，T21 起 v1.2→v2.0）
pub const GAODE_CDN_URL_TEMPLATE: &str =
    "https://webapi.amap.com/maps?v=2.0&key={key}&plugin=AMap.PlaceSearch";

/// 地图容器最小高度（像素；所有嵌入式地图场景硬约束）
pub const MAP_MIN_HEIGHT_PX: u32 = 300;

/// 地图页配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPageConfig {
    /// 高德 Web API key（由部署配置注入，不入库不入 git）
    pub api_key: String,
    /// 地图容器高度（像素）；低于 300 会被钳制到 300
    pub height_px: u32,
}

impl MapPageConfig {
    /// 用 API key 构造默认配置（高度取最小值 300px）
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            height_px: MAP_MIN_HEIGHT_PX,
        }
    }

    /// 生效高度：不低于 300px
    pub fn effective_height_px(&self) -> u32 {
        self.height_px.max(MAP_MIN_HEIGHT_PX)
    }
}

/// 生成校区搜索地图页 HTML（官方 CDN + PlaceSearch 插件 + 结果桥接）
///
/// API key 含引号、尖括号、空白等字符时拒绝（防注入；高德 key 是纯十六进制）。
pub fn build_map_page_html(config: &MapPageConfig) -> Result<String> {
    if config.api_key.is_empty() || !config.api_key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::MalformedResponse(
            "高德 API key 只能是字母或数字".to_owned(),
        ));
    }
    let cdn_url = GAODE_CDN_URL_TEMPLATE.replace("{key}", &config.api_key);
    let height = config.effective_height_px();
    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>校区搜索</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; }}
  /* 决策记忆：地图最小高度 300px，自适应缩放不得低于该值 */
  #map-container {{ width: 100%; height: {height}px; min-height: {min_height}px; }}
</style>
</head>
<body>
<div id="map-container"></div>
<script src="{cdn_url}"></script>
<script>
  var map = new AMap.Map("map-container", {{ zoom: 15 }});
  // 学校名称关键词搜索：结果 JSON 经桥接回传宿主，由 Rust 侧筛选学校类目
  function searchCampus(keyword) {{
    var placeSearch = new AMap.PlaceSearch({{ city: "全国" }});
    placeSearch.search(keyword, function (status, result) {{
      var payload = {{ status: status === "complete" ? "1" : "0", info: status, pois: [] }};
      if (status === "complete" && result.poiList) {{
        payload.pois = result.poiList.pois.map(function (poi) {{
          return {{
            id: poi.id,
            name: poi.name,
            address: poi.address,
            location: poi.location ? poi.location.lng + "," + poi.location.lat : "",
            typecode: poi.type_code || poi.typecode || ""
          }};
        }});
      }}
      window.mcrebuildBridge.postMessage(JSON.stringify(payload));
    }});
  }}
  // 选定校区后地图定位到坐标锚点（此后画边界直接从锚点开始，ADR-0008）
  function centerOn(longitude, latitude) {{
    map.setCenter([longitude, latitude]);
  }}
</script>
</body>
</html>
"#,
        height = height,
        min_height = MAP_MIN_HEIGHT_PX,
        cdn_url = cdn_url,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_uses_official_cdn_v12() {
        let html = build_map_page_html(&MapPageConfig::new("abc123DEF456")).unwrap();
        assert!(html.contains("https://webapi.amap.com/maps?v=1.2&key=abc123DEF456"));
        assert!(html.contains("AMap.PlaceSearch"));
    }

    #[test]
    fn map_height_never_below_300px() {
        let mut config = MapPageConfig::new("abc123");
        config.height_px = 120;
        assert_eq!(config.effective_height_px(), 300);
        let html = build_map_page_html(&config).unwrap();
        assert!(html.contains("height: 300px"));
        assert!(html.contains("min-height: 300px"));

        config.height_px = 720;
        assert_eq!(config.effective_height_px(), 720);
        let html = build_map_page_html(&config).unwrap();
        assert!(html.contains("height: 720px"));
    }

    #[test]
    fn suspicious_api_key_is_rejected() {
        for bad in ["", "key\"onload=", "<script>", "a b c"] {
            let config = MapPageConfig::new(bad);
            assert!(build_map_page_html(&config).is_err(), "应拒绝：{bad:?}");
        }
    }
}
