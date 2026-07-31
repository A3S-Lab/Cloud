use std::process::ExitCode;

use a3s_workflow_cli::{execute, parse, Command, HELP};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let server =
        std::env::var("A3S_WORKFLOW_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let token = std::env::var("A3S_WORKFLOW_API_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let cli = parse(std::env::args().skip(1), &server, token)?;
    match cli.command {
        Command::Help => println!("{HELP}"),
        Command::Version => println!("a3s-workflow {}", env!("CARGO_PKG_VERSION")),
        _ => {
            let compact = cli.compact;
            let value = execute(cli).await?;
            if compact {
                println!("{}", serde_json::to_string(&value)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
        }
    }
    Ok(())
}
