fn action_row_contributes_preferred_height(source: &str, id: &str) -> bool {
    let Some(start) = source.find(&format!("{id} := HorizontalLayout")) else {
        return false;
    };
    source[start..]
        .lines()
        .take(5)
        .any(|line| line.contains("preferred-height: 32px;"))
}

fn card_height_follows_padded_content(source: &str, content_id: &str) -> bool {
    let height_binding = format!("height: {content_id}.preferred-height;");
    let Some(content_start) = source.find(&format!("{content_id} := VerticalLayout")) else {
        return false;
    };
    source.contains(&height_binding)
        && source[content_start..]
            .lines()
            .take(5)
            .any(|line| line.contains("padding: 24px;"))
}

#[test]
fn confirm_dialog_action_row_contributes_to_card_height() {
    let dialogs = include_str!("../ui/dialogs.slint");
    assert!(
        action_row_contributes_preferred_height(dialogs, "dialog-actions"),
        "确认弹窗操作栏必须有明确高度，否则按钮不会计入卡片的 preferred-height"
    );
    assert!(
        card_height_follows_padded_content(dialogs, "dialog-body"),
        "确认弹窗必须由带内边距的内容布局直接决定卡片高度"
    );
}

#[test]
fn input_dialog_action_row_contributes_to_card_height() {
    let dialogs = include_str!("../ui/dialogs.slint");
    assert!(
        action_row_contributes_preferred_height(dialogs, "input-actions"),
        "输入弹窗操作栏必须有明确高度，否则按钮会超出卡片"
    );
    assert!(
        card_height_follows_padded_content(dialogs, "input-body"),
        "输入弹窗必须由带内边距的内容布局直接决定卡片高度"
    );
}

#[test]
fn error_dialog_action_row_contributes_to_card_height() {
    let main = include_str!("../ui/main.slint");
    assert!(
        action_row_contributes_preferred_height(main, "error-dialog-actions"),
        "错误弹窗操作栏必须有明确高度，否则按钮会超出卡片"
    );
    assert!(
        card_height_follows_padded_content(main, "error-dialog-content"),
        "错误弹窗必须由带内边距的内容布局直接决定卡片高度"
    );
}
