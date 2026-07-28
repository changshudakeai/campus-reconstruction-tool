//! 边界绘制 UI 状态机子模块
//!
//! **核心功能**：维护边界绘制的交互状态机，处理多点触控/鼠标拖拽事件。
//! Slint UI 层只负责渲染与事件采集：指针事件转换为 [BoundaryUiEvent]
//! 送入 [BoundaryDrawer]，绘制结果（顶点表 + 状态）由壳层绑定回 Slint 属性。
//!
//! ## 状态流转
//!
//! `Idle` →（点击空白）`Drawing` →（双击/右键确认）`Determined`
//! →（点击顶点）`Editing` →（拖拽顶点）位置更新 →（确认）`Determined`

/// UI 上表示一个顶点的坐标（平面米单位，相对于边界中心）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// X 坐标（东向为正）
    pub x: f64,
    /// Y 坐标（北向为正）
    pub y: f64,
}

impl Vertex {
    /// 新建顶点
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// 边界绘制的 UI 状态
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BoundaryState {
    /// 空闲：尚未开始绘制
    #[default]
    Idle,
    /// 绘制中：正在逐点添加顶点
    Drawing,
    /// 已确认：至少三点形成闭合多边形
    Determined,
    /// 编辑模式：选中某个顶点准备拖拽
    Editing {
        /// 被选中顶点在顶点表中的下标
        selected_vertex: usize,
    },
}

/// 边界绘制事件（由 Slint UI 层把指针/触控事件转换后发送）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryUiEvent {
    /// 点击空白处（开始绘制或添加新顶点）
    ClickAt { x: f64, y: f64 },
    /// 拖动中（编辑模式下移动选中顶点）
    DragTo { x: f64, y: f64 },
    /// 双击/右键确认（结束绘制）
    Confirm,
    /// 取消（绘制中撤掉最后一点；编辑中退出选中）
    Cancel,
    /// 点击现有顶点（进入编辑模式）
    ClickVertex { index: usize },
}

/// 事件处理结果
#[derive(Debug, Clone, PartialEq)]
pub enum EventResult {
    /// 已接受，状态/顶点已更新
    Accepted,
    /// 被拒绝（附带中文原因，暂硬编码，待 T03 文本外置后换文本键）
    Rejected(String),
    /// 当前状态不处理此事件
    Ignored,
}

/// 边界绘制器 —— 维护当前绘制状态和多边形顶点
#[derive(Debug)]
pub struct BoundaryDrawer {
    /// 当前多边形的顶点列表
    vertices: Vec<Vertex>,
    /// 当前 UI 状态
    state: BoundaryState,
    /// 顶点数上限（防止过度复杂的多边形拖垮渲染）
    max_vertices: usize,
}

impl Default for BoundaryDrawer {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundaryDrawer {
    /// 默认顶点数上限
    const DEFAULT_MAX_VERTICES: usize = 50;

    /// 新建一个空白的边界绘制器
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            state: BoundaryState::Idle,
            max_vertices: Self::DEFAULT_MAX_VERTICES,
        }
    }

    /// 设置最大顶点数（0 会被忽略）
    pub fn set_max_vertices(&mut self, count: usize) {
        if count > 0 {
            self.max_vertices = count;
        }
    }

    /// 当前顶点列表（只读）
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// 当前状态
    pub fn state(&self) -> BoundaryState {
        self.state
    }

    /// 处理 UI 事件（主入口）
    pub fn handle_event(&mut self, event: BoundaryUiEvent) -> EventResult {
        match event {
            BoundaryUiEvent::ClickAt { x, y } => self.on_click(x, y),
            BoundaryUiEvent::DragTo { x, y } => self.on_drag(x, y),
            BoundaryUiEvent::Confirm => self.on_confirm(),
            BoundaryUiEvent::Cancel => self.on_cancel(),
            BoundaryUiEvent::ClickVertex { index } => self.on_click_vertex(index),
        }
    }

    /// 重置为初始状态（清空全部顶点）
    pub fn reset(&mut self) {
        self.vertices.clear();
        self.state = BoundaryState::Idle;
    }

    /// T24: 从外部来源（地图 WebView 确认）装载已定边界顶点
    ///
    /// 顶点须为平面米单位（相对同一参考原点），且调用前必须经
    /// [`crate::validate_polygon_closure`] 验证通过——本方法只做
    /// 状态迁移（任意状态 → `Determined`），不重复几何校验。
    /// 不受 `max_vertices` 点击上限约束（该上限仅约束手工点击 UX，
    /// OSM 自动获取的多边形顶点数可能远超 50）。
    pub fn load_determined(&mut self, vertices: Vec<Vertex>) {
        self.vertices = vertices;
        self.state = BoundaryState::Determined;
    }

    /// 点击空白处：开始绘制或追加顶点
    fn on_click(&mut self, x: f64, y: f64) -> EventResult {
        match self.state {
            BoundaryState::Idle | BoundaryState::Drawing => {
                if self.vertices.len() >= self.max_vertices {
                    return EventResult::Rejected(format!(
                        "顶点数已达上限（{} 个）",
                        self.max_vertices
                    ));
                }
                self.vertices.push(Vertex::new(x, y));
                self.state = BoundaryState::Drawing;
                EventResult::Accepted
            }
            // 已确认/编辑中不追加顶点，避免误触破坏已定边界
            BoundaryState::Determined | BoundaryState::Editing { .. } => EventResult::Ignored,
        }
    }

    /// 拖动：仅编辑模式下移动选中顶点
    fn on_drag(&mut self, x: f64, y: f64) -> EventResult {
        match self.state {
            BoundaryState::Editing { selected_vertex } => {
                match self.vertices.get_mut(selected_vertex) {
                    Some(vertex) => {
                        *vertex = Vertex::new(x, y);
                        EventResult::Accepted
                    }
                    None => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// 确认：闭合多边形（至少 3 点）
    fn on_confirm(&mut self) -> EventResult {
        if self.vertices.len() < 3 {
            return EventResult::Rejected("至少需要 3 个点才能闭合边界".to_string());
        }
        self.state = BoundaryState::Determined;
        EventResult::Accepted
    }

    /// 取消：绘制中撤掉最后一点；编辑中退出选中
    fn on_cancel(&mut self) -> EventResult {
        match self.state {
            BoundaryState::Drawing => {
                self.vertices.pop();
                if self.vertices.is_empty() {
                    self.state = BoundaryState::Idle;
                }
                EventResult::Accepted
            }
            BoundaryState::Editing { .. } => {
                self.state = BoundaryState::Determined;
                EventResult::Accepted
            }
            _ => EventResult::Ignored,
        }
    }

    /// 点击已有顶点：进入编辑模式
    fn on_click_vertex(&mut self, index: usize) -> EventResult {
        if index >= self.vertices.len() {
            return EventResult::Rejected("无效的顶点索引".to_string());
        }
        self.state = BoundaryState::Editing {
            selected_vertex: index,
        };
        EventResult::Accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawer_with_triangle() -> BoundaryDrawer {
        let mut drawer = BoundaryDrawer::new();
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 0.0, y: 0.0 });
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 10.0, y: 0.0 });
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 5.0, y: 10.0 });
        drawer
    }

    #[test]
    fn clicks_accumulate_vertices_in_drawing_state() {
        let drawer = drawer_with_triangle();
        assert_eq!(drawer.vertices().len(), 3);
        assert_eq!(drawer.state(), BoundaryState::Drawing);
    }

    #[test]
    fn confirm_needs_at_least_three_points() {
        let mut drawer = BoundaryDrawer::new();
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 0.0, y: 0.0 });
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 10.0, y: 0.0 });

        let result = drawer.handle_event(BoundaryUiEvent::Confirm);
        assert!(matches!(result, EventResult::Rejected(_)));
        assert_eq!(drawer.state(), BoundaryState::Drawing);
    }

    #[test]
    fn confirm_with_three_points_reaches_determined() {
        let mut drawer = drawer_with_triangle();
        assert_eq!(
            drawer.handle_event(BoundaryUiEvent::Confirm),
            EventResult::Accepted
        );
        assert_eq!(drawer.state(), BoundaryState::Determined);
    }

    #[test]
    fn drag_moves_selected_vertex_in_editing_mode() {
        let mut drawer = drawer_with_triangle();
        drawer.handle_event(BoundaryUiEvent::Confirm);

        assert_eq!(
            drawer.handle_event(BoundaryUiEvent::ClickVertex { index: 1 }),
            EventResult::Accepted
        );
        assert_eq!(
            drawer.handle_event(BoundaryUiEvent::DragTo { x: 12.0, y: 3.0 }),
            EventResult::Accepted
        );
        assert_eq!(drawer.vertices()[1], Vertex::new(12.0, 3.0));
    }

    #[test]
    fn invalid_vertex_index_is_rejected() {
        let mut drawer = drawer_with_triangle();
        drawer.handle_event(BoundaryUiEvent::Confirm);

        let result = drawer.handle_event(BoundaryUiEvent::ClickVertex { index: 99 });
        assert!(matches!(result, EventResult::Rejected(_)));
    }

    #[test]
    fn cancel_during_drawing_removes_last_point() {
        let mut drawer = drawer_with_triangle();
        drawer.handle_event(BoundaryUiEvent::Cancel);
        assert_eq!(drawer.vertices().len(), 2);

        drawer.handle_event(BoundaryUiEvent::Cancel);
        drawer.handle_event(BoundaryUiEvent::Cancel);
        assert!(drawer.vertices().is_empty());
        assert_eq!(drawer.state(), BoundaryState::Idle);
    }

    #[test]
    fn vertex_limit_is_enforced() {
        let mut drawer = BoundaryDrawer::new();
        drawer.set_max_vertices(3);

        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 0.0, y: 0.0 });
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 10.0, y: 0.0 });
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 5.0, y: 10.0 });

        let result = drawer.handle_event(BoundaryUiEvent::ClickAt { x: 0.0, y: 5.0 });
        assert!(matches!(result, EventResult::Rejected(_)));
        assert_eq!(drawer.vertices().len(), 3);
    }

    #[test]
    fn reset_clears_everything() {
        let mut drawer = drawer_with_triangle();
        drawer.handle_event(BoundaryUiEvent::Confirm);
        drawer.reset();

        assert!(drawer.vertices().is_empty());
        assert_eq!(drawer.state(), BoundaryState::Idle);
    }
}
