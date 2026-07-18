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
        let mut command = Command::new(env!("CARGO_BIN_EXE_campus-map"));
        command.arg(&pipe).arg(&token);
        if std::env::var_os("CAMPUS_TEST_GUI").is_none() {
            command.env("CAMPUS_MAP_HEADLESS", "1");
        }
        let child = command.spawn().unwrap();
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
                center_lng: 0.0,
                center_lat: 0.0,
                zoom: 17.0,
                pitch: 45.0,
                rotation: 0.0,
                js_api_key: String::new(),
                security_code: String::new(),
                boundary: Vec::new(),
                purpose: MapPurpose::CampusSelection,
                overlays: Vec::new(),
                feature_kind: None,
                english: false,
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
        loop {
            match read_message::<_, ToolEvent>(&mut server).await.unwrap() {
                ToolEvent::Closed {
                    tool: ToolKind::Map,
                } => break,
                ToolEvent::Error { .. } if std::env::var_os("CAMPUS_TEST_GUI").is_some() => {}
                event => panic!("unexpected map event before close: {event:?}"),
            }
        }
        child
    });
    assert!(child.wait().unwrap().success());
}
