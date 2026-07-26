//! 候选对象分类：六类别
//!
//! 每个采集回来的真实对象**恰好属于一类**，归类看它在 Minecraft 里怎么生成：
//! - 建筑（体育馆、游泳馆这类有屋顶的都算）
//! - 道路（铺地）、水域（水面）、植被（树木绿地）
//! - 体育（操场跑道等铺场地）
//! - 其他（校内铁路、雕塑、电力设施……前五类都装不下）
//!
//! **"其他"是正式类别，由标签自动归入**，没有人工"标记为其他"操作。

/// 候选对象六类别 —— 在 Minecraft 里的物理形态
#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum CandidateCategory {
    /// 建筑：有围合空间、有屋顶
    Building,
    /// 道路：铺地路径
    Road,
    /// 水域：水面、水池
    Water,
    /// 植被：树木、绿地
    Vegetation,
    /// 体育：操场、跑道等运动场地
    Sports,
    /// 其他：不属前五类的真实对象（铁路、雕塑、电力设施等）
    Other,
}

impl CandidateCategory {
    /// 返回中文显示名
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Building => "建筑",
            Self::Road => "道路",
            Self::Water => "水域",
            Self::Vegetation => "植被",
            Self::Sports => "体育",
            Self::Other => "其他",
        }
    }

    /// 返回优先级（冲突时取最高）：建筑 > 体育 > 水域 > 道路 > 植被 > 其他
    ///
    /// 供 B13 标签归类时使用（ADR-0011：体育馆不会被建筑和体育两边重复采集）。
    pub fn priority(&self) -> u8 {
        match self {
            Self::Building => 6,
            Self::Sports => 5,
            Self::Water => 4,
            Self::Road => 3,
            Self::Vegetation => 2,
            Self::Other => 1,
        }
    }
}
