#![cfg(target_os = "windows")]

use campus_tool_protocol::{
    read_message, write_message, MapPurpose, ToolCommand, ToolEvent, ToolKind, PROTOCOL_VERSION,
};
use std::process::Command;
use tokio::net::windows::named_pipe::ServerOptions;

#[test]
fn map_process_completes_authenticated_pipe_handshake() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let random = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pipe = format!(r"\\.\pipe\campus-map-test-{random}");
    let token = format!("test-token-{random}");
    let mut child = runtime.block_on(async move {
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_campus-map"))
            .arg(&pipe)
            .arg(&token)
            .env("CAMPUS_MAP_HEADLESS", "1")
            .spawn()
            .unwrap();
        server.connect().await.unwrap();
        let hello: ToolCommand = read_message(&mut server).await.unwrap();
        assert_eq!(
            hello,
            ToolCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
                session_token: token,
                tool: ToolKind::Map,
            }
        );
        write_message(
            &mut server,
            &ToolCommand::OpenMap {
                campus_name: "IPC test".into(),
                center_lng: 121.4,
                center_lat: 31.2,
                zoom: 17.0,
                pitch: 45.0,
                rotation: 0.0,
                js_api_key: "test".into(),
                security_code: "test".into(),
                boundary: Vec::new(),
                purpose: MapPurpose::CampusReview,
                overlays: Vec::new(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            read_message::<_, ToolEvent>(&mut server).await.unwrap(),
            ToolEvent::Ready {
                protocol_version: PROTOCOL_VERSION,
                tool: ToolKind::Map,
            }
        );
        assert_eq!(
            read_message::<_, ToolEvent>(&mut server).await.unwrap(),
            ToolEvent::Closed {
                tool: ToolKind::Map,
            }
        );
        child
    });
    assert!(child.wait().unwrap().success());
}
