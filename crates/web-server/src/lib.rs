use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{
    header::{ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    HeaderName, HeaderValue, Method, StatusCode,
};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const INDEX_CACHE_CONTROL: &str = "no-cache, no-store, must-revalidate";
const ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const FILE_CACHE_CONTROL: &str = "public, max-age=3600";
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; base-uri 'self'; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'";

#[derive(Debug, thiserror::Error)]
pub enum SpaServerError {
    #[error("SPA root {path} is unavailable: {source}")]
    RootUnavailable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SPA root {0} is not a directory")]
    RootNotDirectory(PathBuf),
    #[error("SPA entrypoint {path} is unavailable: {source}")]
    EntrypointUnavailable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SPA entrypoint {0} is not a file inside the configured root")]
    InvalidEntrypoint(PathBuf),
}

#[derive(Clone)]
struct SpaState {
    root: PathBuf,
    index: PathBuf,
}

pub async fn build_spa_router(root: impl AsRef<Path>) -> Result<Router, SpaServerError> {
    let requested_root = root.as_ref().to_path_buf();
    let root = tokio::fs::canonicalize(&requested_root)
        .await
        .map_err(|source| SpaServerError::RootUnavailable {
            path: requested_root,
            source,
        })?;
    let root_metadata =
        tokio::fs::metadata(&root)
            .await
            .map_err(|source| SpaServerError::RootUnavailable {
                path: root.clone(),
                source,
            })?;
    if !root_metadata.is_dir() {
        return Err(SpaServerError::RootNotDirectory(root));
    }

    let requested_index = root.join("index.html");
    let index = tokio::fs::canonicalize(&requested_index)
        .await
        .map_err(|source| SpaServerError::EntrypointUnavailable {
            path: requested_index,
            source,
        })?;
    let index_metadata = tokio::fs::metadata(&index).await.map_err(|source| {
        SpaServerError::EntrypointUnavailable {
            path: index.clone(),
            source,
        }
    })?;
    if !index_metadata.is_file() || !index.starts_with(&root) {
        return Err(SpaServerError::InvalidEntrypoint(index));
    }

    let state = Arc::new(SpaState { root, index });
    Ok(Router::new()
        .route("/healthz", get(health))
        .fallback(serve_spa)
        .with_state(state))
}

async fn health(request: Request) -> Response {
    let head_only = request.method() == Method::HEAD;
    response(
        StatusCode::OK,
        "text/plain; charset=utf-8",
        INDEX_CACHE_CONTROL,
        b"ok\n".to_vec(),
        head_only,
    )
}

async fn serve_spa(State(state): State<Arc<SpaState>>, request: Request) -> Response {
    let method = request.method().clone();
    if method != Method::GET && method != Method::HEAD {
        let mut response = error_response(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n");
        response
            .headers_mut()
            .insert(ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }

    let segments = match decode_path(request.uri().path()) {
        Ok(segments) => segments,
        Err(()) => return error_response(StatusCode::BAD_REQUEST, "invalid request path\n"),
    };
    if segments.first().is_some_and(|segment| segment == "api") {
        return error_response(StatusCode::NOT_FOUND, "not found\n");
    }

    let head_only = method == Method::HEAD;
    if segments.is_empty() {
        return serve_index(&state, head_only).await;
    }

    let requested = segments
        .iter()
        .fold(state.root.clone(), |path, segment| path.join(segment));
    match tokio::fs::metadata(&requested).await {
        Ok(metadata) if metadata.is_file() => serve_file(&state, requested, head_only).await,
        Ok(_) => serve_index(&state, head_only).await,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if looks_like_asset(&segments) {
                error_response(StatusCode::NOT_FOUND, "not found\n")
            } else {
                serve_index(&state, head_only).await
            }
        }
        Err(error) => {
            tracing::warn!(path = %requested.display(), %error, "failed to inspect SPA path");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n")
        }
    }
}

async fn serve_index(state: &SpaState, head_only: bool) -> Response {
    serve_known_file(
        &state.index,
        "text/html; charset=utf-8",
        INDEX_CACHE_CONTROL,
        head_only,
    )
    .await
}

async fn serve_file(state: &SpaState, requested: PathBuf, head_only: bool) -> Response {
    let canonical = match tokio::fs::canonicalize(&requested).await {
        Ok(path) if path.starts_with(&state.root) => path,
        Ok(path) => {
            tracing::warn!(path = %path.display(), "refused SPA path outside configured root");
            return error_response(StatusCode::NOT_FOUND, "not found\n");
        }
        Err(error) => {
            tracing::warn!(path = %requested.display(), %error, "failed to resolve SPA path");
            return error_response(StatusCode::NOT_FOUND, "not found\n");
        }
    };
    let content_type = content_type(&canonical);
    let cache_control = if canonical
        .strip_prefix(&state.root)
        .is_ok_and(|path| path.starts_with("static"))
    {
        ASSET_CACHE_CONTROL
    } else {
        FILE_CACHE_CONTROL
    };
    serve_known_file(&canonical, content_type, cache_control, head_only).await
}

async fn serve_known_file(
    path: &Path,
    content_type: &'static str,
    cache_control: &'static str,
    head_only: bool,
) -> Response {
    match tokio::fs::read(path).await {
        Ok(body) => response(StatusCode::OK, content_type, cache_control, body, head_only),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "failed to read SPA file");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n")
        }
    }
}

fn response(
    status: StatusCode,
    content_type: &'static str,
    cache_control: &'static str,
    body: Vec<u8>,
    head_only: bool,
) -> Response {
    let content_length = body.len();
    let mut response = Response::new(if head_only {
        Body::empty()
    } else {
        Body::from(body)
    });
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
    if let Ok(value) = HeaderValue::from_str(&content_length.to_string()) {
        headers.insert(CONTENT_LENGTH, value);
    }
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), geolocation=(), microphone=()"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    response
}

fn error_response(status: StatusCode, message: &'static str) -> Response {
    response(
        status,
        "text/plain; charset=utf-8",
        INDEX_CACHE_CONTROL,
        message.as_bytes().to_vec(),
        false,
    )
}

fn decode_path(path: &str) -> Result<Vec<String>, ()> {
    if !path.starts_with('/') {
        return Err(());
    }
    let decoded = percent_decode(path.as_bytes())?;
    let decoded = String::from_utf8(decoded).map_err(|_| ())?;
    let mut segments = Vec::new();
    for segment in decoded.split('/').skip(1) {
        if segment.is_empty() {
            continue;
        }
        if segment == "."
            || segment == ".."
            || segment.starts_with('.')
            || segment.contains(['\\', '\0', ':'])
        {
            return Err(());
        }
        segments.push(segment.to_owned());
    }
    Ok(segments)
}

fn percent_decode(input: &[u8]) -> Result<Vec<u8>, ()> {
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let high = input
                .get(index + 1)
                .and_then(|value| hex(*value))
                .ok_or(())?;
            let low = input
                .get(index + 2)
                .and_then(|value| hex(*value))
                .ok_or(())?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(input[index]);
            index += 1;
        }
    }
    Ok(decoded)
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn looks_like_asset(segments: &[String]) -> bool {
    segments.last().is_some_and(|segment| segment.contains('.'))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        Some("xml") => "application/xml; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use tempfile::TempDir;
    use tower::ServiceExt;

    async fn fixture() -> Result<(TempDir, Router), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::create_dir_all(directory.path().join("static/js"))?;
        std::fs::write(
            directory.path().join("index.html"),
            "<!doctype html><main>Cloud</main>",
        )?;
        std::fs::write(
            directory.path().join("static/js/index.abc123.js"),
            "console.log('cloud');",
        )?;
        let router = build_spa_router(directory.path()).await?;
        Ok((directory, router))
    }

    async fn body(response: Response) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        Ok(response.into_body().collect().await?.to_bytes().to_vec())
    }

    #[tokio::test]
    async fn serves_index_with_security_headers() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, router) = fixture().await?;
        let response = router
            .oneshot(Request::builder().uri("/").body(Body::empty())?)
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CACHE_CONTROL),
            Some(&HeaderValue::from_static(INDEX_CACHE_CONTROL))
        );
        assert_eq!(
            response
                .headers()
                .get(HeaderName::from_static("x-content-type-options")),
            Some(&HeaderValue::from_static("nosniff"))
        );
        assert_eq!(body(response).await?, b"<!doctype html><main>Cloud</main>");
        Ok(())
    }

    #[tokio::test]
    async fn serves_hashed_assets_with_immutable_caching() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_directory, router) = fixture().await?;
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/static/js/index.abc123.js")
                    .body(Body::empty())?,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CACHE_CONTROL),
            Some(&HeaderValue::from_static(ASSET_CACHE_CONTROL))
        );
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/javascript; charset=utf-8"))
        );
        assert_eq!(body(response).await?, b"console.log('cloud');");
        Ok(())
    }

    #[tokio::test]
    async fn supports_head_without_returning_a_body() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, router) = fixture().await?;
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri("/static/js/index.abc123.js")
                    .body(Body::empty())?,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_LENGTH),
            Some(&HeaderValue::from_static("21"))
        );
        assert!(body(response).await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn falls_back_to_index_for_client_routes() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, router) = fixture().await?;
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/organizations/local/projects/cloud")
                    .body(Body::empty())?,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await?, b"<!doctype html><main>Cloud</main>");
        Ok(())
    }

    #[tokio::test]
    async fn reserves_api_and_missing_asset_paths() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, router) = fixture().await?;
        for path in ["/api/v1/health/live", "/static/js/missing.js"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty())?)
                .await?;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "path: {path}");
        }
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unsafe_and_mutating_requests() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, router) = fixture().await?;
        let traversal = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/%2e%2e/secret.txt")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);

        let mutation = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(mutation.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            mutation.headers().get(ALLOW),
            Some(&HeaderValue::from_static("GET, HEAD"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn requires_an_index_entrypoint() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let error = build_spa_router(directory.path()).await;
        assert!(matches!(
            error,
            Err(SpaServerError::EntrypointUnavailable { .. })
        ));
        Ok(())
    }
}
