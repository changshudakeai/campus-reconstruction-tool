#[test]
fn review_drawer_hides_the_expanded_candidate_detail_card() {
    let review = include_str!("../ui/review.slint");

    assert!(
        !review.contains("if root.detail-visible: Rectangle"),
        "评审抽屉不得再渲染占用大块纵向空间的候选详情卡"
    );
    assert!(
        review.contains("review-focus := FocusScope"),
        "隐藏详情卡后必须保留可滚动候选评审区"
    );
}
