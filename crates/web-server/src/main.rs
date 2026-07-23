use a3s_cloud_web_server::build_spa_router;
use std::net::SocketAddr;
use std::path::PathBuf;

const DEFAULT_LISTEN: &str = "127.0.0.1:3011";
const DEFAULT_ROOT: &str = "web/dist";

struct Options {
    listen: SocketAddr,
    root: PathBuf,
}

enum Command {
    Run(Options),
    Help,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let options = match parse_options(std::env::args().skip(1))? {
        Command::Run(options) => options,
        Command::Help => {
            print_help();
            return Ok(());
        }
    };
    let application = build_spa_router(&options.root).await?;
    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    tracing::info!(
        listen = %options.listen,
        root = %options.root.display(),
        "A3S Cloud SPA server started"
    );
    axum::serve(listener, application)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn parse_options(arguments: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut listen =
        std::env::var("A3S_CLOUD_WEB_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.into());
    let mut root = std::env::var("A3S_CLOUD_WEB_ROOT").unwrap_or_else(|_| DEFAULT_ROOT.into());
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "--listen" => {
                listen = arguments
                    .next()
                    .ok_or_else(|| "--listen requires an address".to_owned())?;
            }
            "--root" => {
                root = arguments
                    .next()
                    .ok_or_else(|| "--root requires a directory".to_owned())?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let listen = listen
        .parse()
        .map_err(|error| format!("invalid listen address {listen}: {error}"))?;
    Ok(Command::Run(Options {
        listen,
        root: PathBuf::from(root),
    }))
}

fn print_help() {
    println!(
        "A3S Cloud SPA server\n\nUsage: a3s-cloud-web-server [--listen ADDRESS] [--root DIRECTORY]\n\nDefaults:\n  --listen {DEFAULT_LISTEN}\n  --root   {DEFAULT_ROOT}"
    );
}

#[cfg(unix)]
async fn shutdown_signal() {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "failed to install SIGTERM handler"),
        }
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(error) = result {
                tracing::error!(%error, "failed to install Ctrl-C handler");
            }
        },
        () = terminate => {},
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install Ctrl-C handler");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_options() -> Result<(), Box<dyn std::error::Error>> {
        let command = parse_options(
            [
                "--listen".to_owned(),
                "127.0.0.1:4010".to_owned(),
                "--root".to_owned(),
                "custom-dist".to_owned(),
            ]
            .into_iter(),
        )?;
        let Command::Run(options) = command else {
            return Err("expected run command".into());
        };
        assert_eq!(options.listen, "127.0.0.1:4010".parse()?);
        assert_eq!(options.root, PathBuf::from("custom-dist"));
        Ok(())
    }

    #[test]
    fn rejects_unknown_arguments() {
        let result = parse_options(["--unknown".to_owned()].into_iter());
        assert!(result.is_err());
    }
}
