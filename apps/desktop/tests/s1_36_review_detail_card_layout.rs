#[test]
fn review_drawer_hides_detail_card_and_keeps_t51_layout_invariants() {
    let review = include_str!("../ui/review.slint");

    assert!(
        !review.contains("if root.detail-visible: Rectangle"),
        "评审抽屉不得再渲染占用大块纵向空间的候选详情卡"
    );
    assert!(
        review.contains("review-focus := FocusScope"),
        "隐藏详情卡后必须保留可滚动候选评审区"
    );
    // T51：分页行常显（含 1/1），不得再被 page-total > 1 条件整行隐藏。
    assert!(
        !review.contains("if root.page-total > 1"),
        "分页行必须常显，筛选后只剩一页也不得消失"
    );
    // T51：多选底色跟随 selected，地图联动高亮只保留独立描边样式，两者解耦。
    assert!(
        review.contains("background: card.selected ? Theme.highlight : transparent;")
            && review
                .contains("border-color: card.highlighted ? Theme.map-highlight : transparent;"),
        "选中=蓝底、地图高亮=描边，必须分别绑定 selected 与 highlighted"
    );
    // T51：固定批量行（全选复选框 + 已选数量 + 批量三态）位于候选卡片上方。
    assert!(
        review.contains("checked: root.all-page-selected;")
            && review.contains("enabled: root.batch-buttons-enabled && !root.sealed;"),
        "固定批量行必须使用当前页全选状态与批量可用状态"
    );
    // T51：删除暂停/继续评审按钮与回调。
    assert!(
        !review.contains("pause") && !review.contains("resume"),
        "暂停/继续评审按钮必须删除"
    );
}
