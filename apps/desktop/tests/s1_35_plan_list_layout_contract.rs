fn element_has_height(source: &str, id: &str, expected: &str) -> bool {
    let Some(start) = source.find(&format!("{id} :=")) else {
        return false;
    };
    source[start..]
        .lines()
        .take(6)
        .any(|line| line.contains(expected))
}

#[test]
fn create_plan_action_stays_button_sized_in_tall_windows() {
    let plan_list = include_str!("../ui/plan_list.slint");

    assert!(
        element_has_height(plan_list, "create-action-row", "height: 36px;"),
        "新建方案操作栏必须固定为正常按钮行高度，不能瓜分页面剩余高度"
    );
    assert!(
        element_has_height(plan_list, "create-plan-button", "height: 32px;"),
        "新建方案按钮必须有明确高度，最大化窗口时不得纵向拉伸"
    );
}
