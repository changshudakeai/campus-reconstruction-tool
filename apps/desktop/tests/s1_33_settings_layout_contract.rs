#[test]
fn settings_page_is_scrollable_and_keeps_cards_compact() {
    let ui = include_str!("../ui/main.slint");

    assert!(
        ui.contains("settings-scroll := ScrollView"),
        "设置页必须使用独立滚动区域，避免普通窗口高度下底部操作落到窗口外"
    );
    assert!(
        ui.contains("settings-content-width: min(max(0px, root.width - 64px), 450px)"),
        "设置卡片列必须限制为 450px，并在窄窗口保留左右边距"
    );
    assert!(
        ui.contains("width: root.settings-content-width"),
        "设置页内容必须实际使用受限宽度，避免放大窗口时卡片被横向拉伸"
    );
}
