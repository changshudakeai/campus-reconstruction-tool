use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use base64::Engine;
use campus_services::acquisition::{
    AcquisitionClient, AcquisitionClientErrorKind, BoundaryCandidateValidity,
    CampusBoundaryCandidateQuery, HttpsTransport, CONTRACT_VERSION,
};

fn read_request(mut stream: &std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = stream.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        let text = String::from_utf8_lossy(&bytes);
        let Some(header_end) = text.find("\r\n\r\n") else {
            continue;
        };
        let content_length = text[..header_end]
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        if bytes.len() >= header_end + 4 + content_length {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

fn write_json(mut stream: &std::net::TcpStream, status: &str, body: serde_json::Value) {
    let body = serde_json::to_vec(&body).unwrap();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(&body).unwrap();
}

fn write_chunk(mut stream: &std::net::TcpStream, cursor: &str, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\nX-Stable-Cursor: {cursor}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
}

#[test]
fn installed_style_client_crosses_a_live_http_contract_boundary() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json"
    ))
    .unwrap();
    let chunk = &fixture["manifest"]["chunks"][0];
    let cursor = chunk["stable_cursor"].as_str().unwrap().to_owned();
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(
            fixture["transport_chunks"][chunk["id"].as_str().unwrap()]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let supported_bundle = fixture["bundle"].clone();
    let manifest = serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "bundle": fixture["bundle"],
        "coverage_report": fixture["coverage_report"],
        "licences": fixture["candidates"].as_array().unwrap()
            .iter().map(|candidate| candidate["licence"].clone()).collect::<Vec<_>>(),
        "chunks": fixture["manifest"]["chunks"],
        "result_sha256": fixture["manifest"]["result_sha256"]
    });
    let server = thread::spawn(move || {
        let responses = [
            serde_json::json!({
                "contract_versions": [CONTRACT_VERSION],
                "supported_bundles": [supported_bundle],
                "limits": {
                    "area_square_metres": 100000000,
                    "boundary_vertices": 10000,
                    "tiles": 10000,
                    "observations": 1000000,
                    "result_bytes": 1000000000,
                    "concurrent_jobs": 2
                },
                "retention_days": 30,
                "quota_remaining": 100
            }),
            serde_json::json!({
                "job_id": "live-boundary-1",
                "contract_version": CONTRACT_VERSION,
                "bundle_id": "cn-campus-2026-06",
                "state": "queued"
            }),
            serde_json::json!({
                "job_id": "live-boundary-1",
                "contract_version": CONTRACT_VERSION,
                "bundle_id": "cn-campus-2026-06",
                "state": "complete"
            }),
            manifest,
        ];
        for response in responses {
            let (stream, _) = listener.accept().unwrap();
            sender.send(read_request(&stream)).unwrap();
            write_json(&stream, "200 OK", response);
        }
        let (stream, _) = listener.accept().unwrap();
        sender.send(read_request(&stream)).unwrap();
        write_chunk(&stream, &cursor, &compressed);
    });
    let transport =
        HttpsTransport::new(format!("http://{address}"), "installation-secret-42").unwrap();
    let client = AcquisitionClient::new(transport);
    let query = CampusBoundaryCandidateQuery::new(
        "Confirmed Campus",
        vec!["Campus Alias".into()],
        [121.4, 31.2],
        2_000.0,
        "installation-42:confirmed-campus",
    )
    .unwrap();

    let job = client.start_boundary_discovery(&query).unwrap();
    let completed = client.boundary_job(&job).unwrap();
    let snapshot = client.resume_boundary_discovery(&completed).unwrap();

    assert_eq!(job.job_id, "live-boundary-1");
    assert_eq!(snapshot.candidates.len(), 2);
    assert!(snapshot
        .validity
        .values()
        .all(|validity| validity == &BoundaryCandidateValidity::Valid));
    let capabilities_request = receiver.recv().unwrap();
    let create_request = receiver.recv().unwrap();
    let status_request = receiver.recv().unwrap();
    let manifest_request = receiver.recv().unwrap();
    let chunk_request = receiver.recv().unwrap();
    assert!(capabilities_request.starts_with("GET /v1/capabilities HTTP/1.1"));
    assert!(create_request.starts_with("POST /v1/boundary-jobs HTTP/1.1"));
    assert!(status_request.starts_with("GET /v1/boundary-jobs/live-boundary-1 HTTP/1.1"));
    assert!(manifest_request.starts_with("GET /v1/boundary-jobs/live-boundary-1/manifest HTTP/1.1"));
    assert!(chunk_request.contains("/v1/boundary-jobs/live-boundary-1/chunks/"));
    assert!(capabilities_request
        .to_ascii_lowercase()
        .contains("authorization: bearer installation-secret-42"));
    assert!(create_request.contains("\"name\":\"Confirmed Campus\""));
    assert!(!create_request.contains("gaode"));
    assert!(!create_request.contains("project"));
    assert!(!create_request.contains("minecraft"));
    server.join().unwrap();
}

#[test]
fn authentication_failure_redacts_the_installation_secret() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = read_request(&stream);
        write_json(
            &stream,
            "401 Unauthorized",
            serde_json::json!({
                "code": "credential_rejected",
                "scope": "installation-secret-42",
                "retryable": false,
                "explanation": "Rejected installation-secret-42",
                "suggested_action": "Replace installation-secret-42"
            }),
        );
    });
    let transport =
        HttpsTransport::new(format!("http://{address}"), "installation-secret-42").unwrap();
    let debug_transport = format!("{transport:?}");
    let client = AcquisitionClient::new(transport);

    let error = client.capabilities().unwrap_err();

    assert!(!debug_transport.contains("installation-secret-42"));
    assert!(!error.to_string().contains("installation-secret-42"));
    assert!(error.to_string().contains("[REDACTED]"));
    server.join().unwrap();
}

#[test]
fn production_https_rejects_an_unvalidated_tls_peer() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        }
    });
    let transport =
        HttpsTransport::new(format!("https://{address}"), "installation-secret-42").unwrap();
    let client = AcquisitionClient::new(transport);

    let error = client.capabilities().unwrap_err();

    assert_eq!(error.kind, AcquisitionClientErrorKind::TransportUnavailable);
    assert!(!error.to_string().contains("installation-secret-42"));
    server.join().unwrap();
}
