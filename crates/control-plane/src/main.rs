use a3s_boot::AxumAdapter;
use a3s_cloud_control_plane::{build_application, CloudConfig};

const DEFAULT_API_BODY_LIMIT_BYTES: usize = 1024 * 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/cloud.acl".to_owned());
    let config = CloudConfig::load(path)?;
    let address = config.server_address()?;
    let body_limit = DEFAULT_API_BODY_LIMIT_BYTES.max(config.sources.github_webhook_max_body_bytes);
    let application = build_application(config).await?;
    application
        .serve_with(&AxumAdapter::new().with_body_limit(body_limit), address)
        .await?;
    Ok(())
}
