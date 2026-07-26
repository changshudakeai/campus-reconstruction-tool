//! 校区 POI 持久化载荷
//!
//! ADR-0008 后果条：校区层持久化字段——高德地点标识、名称、地址、坐标锚点。
//! 本类型是 serde 可序列化的载荷（POI identity + coordinate lineage）；
//! 实际落库由调用方（F3 经 B2）完成，B3 不触碰存储。

use serde::{Deserialize, Serialize};

use crate::poi::SchoolPoi;

/// 校区 POI 记录 —— 校区建立时从高德数据固化的持久化字段
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampusPoiRecord {
    /// 高德地点标识（POI identity）
    pub gaode_poi_id: String,
    /// 校区名称（取自高德，规范无错别字）
    pub name: String,
    /// 所在地址（含行政区）
    pub address: String,
    /// 坐标锚点：经度（coordinate lineage——GCJ-02 原始值，不做换算）
    pub longitude: f64,
    /// 坐标锚点：纬度（同上）
    pub latitude: f64,
    /// 坐标系标识（lineage 凭据：高德使用 GCJ-02）
    pub coordinate_system: String,
    /// 数据来源标识（lineage 凭据）
    pub data_source: String,
}

impl CampusPoiRecord {
    /// 高德坐标系标识
    pub const COORDINATE_SYSTEM_GCJ02: &'static str = "GCJ-02";

    /// 高德数据来源标识
    pub const DATA_SOURCE_GAODE: &'static str = "gaode";

    /// 从选定的学校 POI 固化持久化记录
    pub fn from_poi(poi: &SchoolPoi) -> Self {
        Self {
            gaode_poi_id: poi.poi_id.clone(),
            name: poi.name.clone(),
            address: poi.address.clone(),
            longitude: poi.longitude,
            latitude: poi.latitude,
            coordinate_system: Self::COORDINATE_SYSTEM_GCJ02.to_owned(),
            data_source: Self::DATA_SOURCE_GAODE.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_serializes_round_trip() {
        let poi = SchoolPoi {
            poi_id: "B01".to_owned(),
            name: "第一中学".to_owned(),
            address: "人民路1号".to_owned(),
            longitude: 121.4,
            latitude: 31.2,
            typecode: "141202".to_owned(),
        };
        let record = CampusPoiRecord::from_poi(&poi);
        assert_eq!(record.coordinate_system, "GCJ-02");
        assert_eq!(record.data_source, "gaode");

        let json = serde_json::to_string(&record).unwrap();
        let back: CampusPoiRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, record);
    }
}
