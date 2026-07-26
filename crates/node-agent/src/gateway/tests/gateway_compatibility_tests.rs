//! Mixed-version Gateway management protocol fixtures.

use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn new_agent_installs_through_the_legacy_v1_gateway_wire_contract() {
    let gateway_id = Uuid::now_v7();
    let snapshot = snapshot(gateway_id, 1, None);
    let requested = serde_json::json!({
        "gateway_id": snapshot.gateway_id,
        "revision": snapshot.revision,
        "snapshot_digest": snapshot.snapshot_digest.clone(),
    });
    let applied = serde_json::json!({
        "gateway_id": snapshot.gateway_id,
        "revision": snapshot.revision,
        "expected_revision": snapshot.expected_revision,
        "snapshot_digest": snapshot.snapshot_digest.clone(),
        "issued_at": snapshot.issued_at,
        "expires_at": snapshot.expires_at,
        "applied_at": snapshot.issued_at + ChronoDuration::milliseconds(1),
    });
    let status = serde_json::json!({
        "schema": GatewayManagementProtocol::SNAPSHOT_STATUS_V1,
        "gateway_id": snapshot.gateway_id,
        "requested": requested,
        "state": "applied",
        "ready": true,
        "replayed": false,
        "applied": applied,
    })
    .to_string();
    let (base_url, requests) = serve_management_responses(vec![
        serde_json::json!({
            "name": GATEWAY_NAME,
            "version": "1.0.11",
            "api_version": "v1",
        })
        .to_string(),
        status.clone(),
        status,
    ])
    .await;
    let control = Arc::new(
        GatewayManagementClient::new(
            base_url,
            "fixture-token".into(),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("Gateway client"),
    );
    let installer = DurableGatewaySnapshotInstaller::new(gateway_id, control);

    let outcome = installer.install(&snapshot).await.expect("legacy install");
    assert_eq!(
        outcome,
        GatewaySnapshotInstallOutcome::Applied {
            protocol: GatewayManagementProtocol::legacy_v1(),
        }
    );
    let requests = requests.await.expect("legacy Gateway fixture");
    let request_lines = requests
        .iter()
        .map(|request| request.lines().next().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(request_lines[0], "GET /api/gateway/version HTTP/1.1");
    assert_eq!(
        request_lines[1],
        "POST /api/gateway/snapshots/apply HTTP/1.1"
    );
    assert!(request_lines[2].starts_with("GET /api/gateway/snapshots/status?gateway_id="));
}

async fn serve_management_responses(
    responses: Vec<String>,
) -> (url::Url, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind legacy Gateway fixture");
    let address = listener.local_addr().expect("legacy Gateway address");
    let task = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for body in responses {
            let (mut stream, _) = listener.accept().await.expect("Gateway request");
            requests.push(read_http_request(&mut stream).await);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write Gateway response");
            stream.shutdown().await.expect("close Gateway response");
        }
        requests
    });
    (
        url::Url::parse(&format!("http://{address}/api/gateway")).expect("legacy Gateway URL"),
        task,
    )
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut body_length = None;
    loop {
        let mut buffer = [0_u8; 4096];
        let read = stream
            .read(&mut buffer)
            .await
            .expect("read Gateway request");
        assert!(read > 0, "Gateway request ended before its body");
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            continue;
        };
        let body_length = *body_length.get_or_insert_with(|| {
            String::from_utf8_lossy(&request[..header_end])
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("content length"))
                })
                .unwrap_or(0)
        });
        if request.len() >= header_end + 4 + body_length {
            return String::from_utf8(request).expect("UTF-8 Gateway request");
        }
    }
}
