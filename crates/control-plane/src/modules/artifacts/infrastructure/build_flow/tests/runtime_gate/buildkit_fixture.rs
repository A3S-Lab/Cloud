use super::BUILDKIT_ADDRESS;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use futures_util::TryStreamExt;
use std::error::Error;

const EXEC_OUTPUT_LIMIT: usize = 64 * 1024;

pub(super) async fn prune_buildkit_worker(
    docker: &Docker,
    container: &str,
) -> Result<(), Box<dyn Error>> {
    run(
        docker,
        container,
        &[
            "/usr/bin/buildctl",
            "--addr",
            BUILDKIT_ADDRESS,
            "prune",
            "--all",
        ],
    )
    .await?;
    let remaining = run(
        docker,
        container,
        &[
            "/usr/bin/buildctl",
            "--addr",
            BUILDKIT_ADDRESS,
            "du",
            "--format",
            "{{json .}}",
        ],
    )
    .await?;
    if remaining.trim() != "null" {
        return Err(std::io::Error::other(format!(
            "BuildKit worker retained internal cache after prune: {remaining}"
        ))
        .into());
    }
    Ok(())
}

async fn run(docker: &Docker, container: &str, command: &[&str]) -> Result<String, Box<dyn Error>> {
    let created = docker
        .create_exec(
            container,
            CreateExecOptions::<String> {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(command.iter().map(|argument| (*argument).into()).collect()),
                ..Default::default()
            },
        )
        .await?;
    let started = docker.start_exec(&created.id, None).await?;
    let StartExecResults::Attached {
        mut output,
        input: _,
    } = started
    else {
        return Err(std::io::Error::other("BuildKit fixture exec detached unexpectedly").into());
    };
    let mut captured = Vec::new();
    while let Some(chunk) = output.try_next().await? {
        let bytes = chunk.as_ref();
        let remaining = EXEC_OUTPUT_LIMIT.saturating_sub(captured.len());
        captured.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
    }
    let inspected = docker.inspect_exec(&created.id).await?;
    let captured = String::from_utf8_lossy(&captured).into_owned();
    if inspected.running != Some(false) || inspected.exit_code != Some(0) {
        return Err(std::io::Error::other(format!(
            "BuildKit fixture exec failed with {:?}: {captured}",
            inspected.exit_code
        ))
        .into());
    }
    Ok(captured)
}
