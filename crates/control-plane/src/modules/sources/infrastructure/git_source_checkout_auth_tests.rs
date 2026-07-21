use super::tests::{assert_staging_is_empty, source_request, GitFixture};
use super::GitSourceCheckout;
use crate::modules::sources::domain::{
    GitProvider, GitRepository, ISourceCheckout, SourceCheckoutError, SourceProviderCredential,
};
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::Utc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use zeroize::Zeroizing;

#[tokio::test]
async fn authenticated_http_checkout_uses_a_transient_header_and_credential_free_replay() {
    let fixture = GitFixture::new();
    let commit = fixture.commit("private.txt", "private source\n", "private source");
    fixture.push_main();
    let token = "fixture-private-checkout-token";
    let expected_authorization = format!(
        "Basic {}",
        STANDARD.encode(format!("x-access-token:{token}"))
    );
    let state = Arc::new(GitHttpState {
        root: fixture.root.path().to_owned(),
        expected_authorization: expected_authorization.clone(),
        authorizations: Mutex::new(Vec::new()),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Git HTTP fixture listener");
    let address = listener.local_addr().expect("Git HTTP fixture address");
    let router = Router::new()
        .fallback(git_http_backend)
        .with_state(Arc::clone(&state));
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("Git HTTP fixture server")
    });
    let checkout_root = fixture.root.path().join("http-checkouts");
    let checkout = GitSourceCheckout::for_http_test(
        &checkout_root,
        Duration::from_secs(10),
        1_000,
        16 * 1024 * 1024,
        &format!("http://{address}/remote.git"),
    )
    .expect("HTTP checkout adapter");
    let request = source_request(Uuid::now_v7(), &commit);
    let issued_at = Utc::now();
    let credential = SourceProviderCredential::new(
        &request.repository,
        Zeroizing::new(token.into()),
        issued_at,
        issued_at + chrono::Duration::hours(1),
    )
    .expect("provider credential");

    let accepted = checkout
        .checkout(&request, Some(&credential))
        .await
        .expect("authenticated checkout");
    assert_eq!(
        tokio::fs::read_to_string(accepted.directory.join("private.txt"))
            .await
            .expect("checked-out private file"),
        "private source\n"
    );
    let request_count = state
        .authorizations
        .lock()
        .expect("authorization capture")
        .len();
    assert!(request_count > 0);
    assert!(state
        .authorizations
        .lock()
        .expect("authorization capture")
        .iter()
        .all(|value| value.as_deref() == Some(expected_authorization.as_str())));
    let receipt = tokio::fs::read_to_string(
        checkout_root
            .join(request.checkout_id.to_string())
            .join("receipt.json"),
    )
    .await
    .expect("checkout receipt");
    assert!(!receipt.contains(token));

    drop(credential);
    assert_eq!(
        checkout
            .checkout(&request, None)
            .await
            .expect("credential-free replay"),
        accepted
    );
    assert_eq!(
        state
            .authorizations
            .lock()
            .expect("authorization capture")
            .len(),
        request_count
    );
    server.abort();
}

#[tokio::test]
async fn checkout_rejects_a_repository_mismatched_credential_before_fetch() {
    let fixture = GitFixture::new();
    let commit = fixture.commit("message.txt", "private\n", "private");
    fixture.push_main();
    let checkout_root = fixture.root.path().join("rejected-checkouts");
    let checkout = fixture.checkout(&checkout_root, 1_000);
    let request = source_request(Uuid::now_v7(), &commit);
    let other = GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/other")
        .expect("other repository");
    let issued_at = Utc::now();
    let token = "fixture-mismatched-checkout-token";
    let credential = SourceProviderCredential::new(
        &other,
        Zeroizing::new(token.into()),
        issued_at,
        issued_at + chrono::Duration::hours(1),
    )
    .expect("provider credential");

    let error = checkout
        .checkout(&request, Some(&credential))
        .await
        .expect_err("mismatched credential");
    assert!(matches!(error, SourceCheckoutError::Unavailable(_)));
    assert!(!format!("{error:?}: {error}").contains(token));
    assert_staging_is_empty(&checkout_root);
}

struct GitHttpState {
    root: std::path::PathBuf,
    expected_authorization: String,
    authorizations: Mutex<Vec<Option<String>>>,
}

async fn git_http_backend(State(state): State<Arc<GitHttpState>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let authorization = header_value(&parts.headers, "authorization");
    state
        .authorizations
        .lock()
        .expect("authorization capture")
        .push(authorization.clone());
    if authorization.as_deref() != Some(state.expected_authorization.as_str()) {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("www-authenticate", "Basic realm=\"private-source\"")
            .body(Body::empty())
            .expect("unauthorized response");
    }
    let request_body = match to_bytes(body, 1024 * 1024).await {
        Ok(body) => body,
        Err(_) => return fixture_error(StatusCode::PAYLOAD_TOO_LARGE),
    };
    let mut command = tokio::process::Command::new("git");
    command
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", &state.root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REQUEST_METHOD", parts.method.as_str())
        .env("PATH_INFO", parts.uri.path())
        .env("QUERY_STRING", parts.uri.query().unwrap_or_default())
        .env("CONTENT_LENGTH", request_body.len().to_string())
        .env(
            "CONTENT_TYPE",
            header_value(&parts.headers, "content-type").unwrap_or_default(),
        )
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env("SERVER_NAME", "127.0.0.1")
        .env("SERVER_PORT", "80")
        .env("REMOTE_ADDR", "127.0.0.1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(protocol) = header_value(&parts.headers, "git-protocol") {
        command.env("HTTP_GIT_PROTOCOL", protocol);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return fixture_error(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let Some(mut stdin) = child.stdin.take() else {
        return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
    };
    if stdin.write_all(&request_body).await.is_err() {
        return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
    }
    drop(stdin);
    let output = match child.wait_with_output().await {
        Ok(output) if output.status.success() => output.stdout,
        _ => return fixture_error(StatusCode::INTERNAL_SERVER_ERROR),
    };
    cgi_response(output)
}

fn cgi_response(output: Vec<u8>) -> Response {
    let Some(header_end) = output.windows(4).position(|window| window == b"\r\n\r\n") else {
        return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let Ok(headers) = std::str::from_utf8(&output[..header_end]) else {
        return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let mut status = StatusCode::OK;
    let mut response_headers = Vec::new();
    for line in headers.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("status") {
            let Some(code) = value.split_whitespace().next() else {
                return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
            };
            let Ok(code) = code.parse::<u16>() else {
                return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
            };
            let Ok(parsed) = StatusCode::from_u16(code) else {
                return fixture_error(StatusCode::INTERNAL_SERVER_ERROR);
            };
            status = parsed;
        } else {
            response_headers.push((name.to_owned(), value.to_owned()));
        }
    }
    let mut response = Response::builder().status(status);
    for (name, value) in response_headers {
        response = response.header(name, value);
    }
    response
        .body(Body::from(output[(header_end + 4)..].to_vec()))
        .unwrap_or_else(|_| fixture_error(StatusCode::INTERNAL_SERVER_ERROR))
}

fn fixture_error(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .expect("fixture error response")
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
