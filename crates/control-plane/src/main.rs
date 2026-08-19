use a3s_boot::AxumAdapter;
use a3s_cloud_control_plane::{build_application, CloudConfig};
use std::ffi::OsString;

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

    let mut arguments = std::env::args_os();
    let executable = arguments
        .next()
        .unwrap_or_else(|| OsString::from("a3s-cloud-control-plane"));
    let path = arguments
        .next()
        .unwrap_or_else(|| OsString::from("config/cloud.acl"));
    let config = match (arguments.next(), arguments.next(), arguments.next()) {
        (None, None, None) => CloudConfig::load(path)?,
        (Some(flag), Some(role), None) if flag == "--role" => {
            let role = role
                .to_str()
                .ok_or("packaged process role must be valid UTF-8")?;
            CloudConfig::load(path)?.restrict_to_process_role(role)?
        }
        _ => {
            return Err(format!(
                "usage: {} [cloud.acl [--role all|api|worker|relay]]",
                executable.to_string_lossy()
            )
            .into())
        }
    };
    let address = config.server_address()?;
    let body_limit = DEFAULT_API_BODY_LIMIT_BYTES
        .max(config.sources.github_webhook_max_body_bytes)
        .max(config.assets.max_rpc_body_bytes);
    let application = build_application(config).await?;
    application
        .serve_with(&AxumAdapter::new().with_body_limit(body_limit), address)
        .await?;
    Ok(())
}
