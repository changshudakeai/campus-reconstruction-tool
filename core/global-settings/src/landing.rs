//! "上次使用的校区"视图
//!
//! ADR-0006 着陆流程：老用户打开应用直接落在上次使用的校区的方案列表页，
//! 页面顶部显著展示校区名称 + "切换校区"入口。本模块提供着陆所需的最小
//! 校区视图；校区已被删除时返回 `None`（调用方退回校区选择页）。

use data_persistence::{CampusCrudApi, Database};
use shared_domain_types::CampusId;

use crate::error::Result;

/// 着陆页所需的校区视图（方案列表页顶部展示）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandingCampus {
    /// 校区 ID（B1 共享类型，T02 复用）
    pub id: CampusId,
    /// 校区名称（页面顶部显著展示）
    pub name: String,
}

impl LandingCampus {
    /// 按 ID 查找校区并组装着陆视图；校区不存在（已被删除）返回 `None`
    pub(crate) fn find(db: &Database, campus_id: &CampusId) -> Result<Option<Self>> {
        let Some(entity) = db.find_campus_by_id(&campus_id.to_string())? else {
            return Ok(None);
        };
        // 数据库行 ID 由 B2 生成，必为合法 UUID；解析失败按"校区不存在"处理
        let Ok(id) = CampusId::parse(&entity.id) else {
            return Ok(None);
        };
        Ok(Some(Self {
            id,
            name: entity.name,
        }))
    }
}
