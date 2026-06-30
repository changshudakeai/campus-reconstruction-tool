#![cfg(target_os = "windows")]

use campus_tool_protocol::{
    read_message, write_message, ToolCommand, ToolEvent, ToolKind, PROTOCOL_VERSION,
};
use std::process::Command;
use tokio::net::windows::named_pipe::ServerOptions;

#[test]
fn preview_process_completes_authenticated_pipe_handshake() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let random = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pipe = format!(r"\\.\pipe\campus-preview-test-{random}");
    let token = format!("test-token-{random}");
    let model_path = std::env::temp_dir().join(format!("campus-preview-{random}.json"));
    std::fs::write(
        &model_path,
        r#"{"width":2,"height":1,"length":2,"palette":["minecraft:air","minecraft:bricks"],"blockRuns":[{"paletteIndex":1,"runLength":4}],"report":{}}"#,
    )
    .unwrap();
    let mut child = runtime.block_on(async move {
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_campus-preview"))
            .arg(&pipe)
            .arg(&token)
            .env("CAMPUS_PREVIEW_HEADLESS", "1")
            .spawn()
            .unwrap();
        server.connect().await.unwrap();
        let hello: ToolCommand = read_message(&mut server).await.unwrap();
        assert_eq!(
            hello,
            ToolCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
                session_token: token,
                tool: ToolKind::Preview,
            }
        );
        write_message(
            &mut server,
            &ToolCommand::OpenPreview {
                model_path: model_path.to_string_lossy().into_owned(),
                title: "IPC test".into(),
                english: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            read_message::<_, ToolEvent>(&mut server).await.unwrap(),
            ToolEvent::Ready {
                protocol_version: PROTOCOL_VERSION,
                tool: ToolKind::Preview,
            }
        );
        assert_eq!(
            read_message::<_, ToolEvent>(&mut server).await.unwrap(),
            ToolEvent::Closed {
                tool: ToolKind::Preview,
            }
        );
        let _ = std::fs::remove_file(model_path);
        child
    });
    assert!(child.wait().unwrap().success());
}
