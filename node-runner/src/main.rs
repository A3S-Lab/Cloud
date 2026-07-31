use std::path::PathBuf;

use a3s_workflow_node_runner::NodeExecutor;
use a3s_workflow_protocol::{NodeInvocation, NODE_INVOCATION_PATH, NODE_RESULT_PATH};
use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let invocation_path = std::env::var_os("A3S_WORKFLOW_INVOCATION_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(NODE_INVOCATION_PATH));
    let result_path = std::env::var_os("A3S_WORKFLOW_RESULT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(NODE_RESULT_PATH));

    let source = tokio::fs::read(&invocation_path)
        .await
        .with_context(|| format!("failed to read {}", invocation_path.display()))?;
    let invocation: NodeInvocation =
        serde_json::from_slice(&source).context("invalid node invocation JSON")?;
    let result = NodeExecutor::new()?.execute(&invocation).await?;
    let bytes = serde_json::to_vec(&result).context("failed to encode node result")?;

    if let Some(parent) = result_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    tokio::fs::write(&result_path, bytes)
        .await
        .with_context(|| format!("failed to write {}", result_path.display()))?;
    Ok(())
}
