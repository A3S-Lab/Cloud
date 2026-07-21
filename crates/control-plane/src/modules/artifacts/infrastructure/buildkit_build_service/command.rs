use super::BuildkitConnection;
use crate::modules::sources::domain::BuildRecipe;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub(super) enum BuildkitCommandError {
    #[error("BuildKit client executable is unavailable")]
    ExecutableUnavailable,
    #[error("BuildKit client could not be started")]
    Spawn,
    #[error("BuildKit rejected the build")]
    Failed,
}

pub(super) struct BuildkitCommand {
    executable: PathBuf,
    connection: BuildkitConnection,
}

pub(super) struct BuildkitCommandInput<'a> {
    pub(super) source: &'a Path,
    pub(super) context: &'a Path,
    pub(super) recipe: &'a BuildRecipe,
    pub(super) layout: &'a Path,
    pub(super) metadata: &'a Path,
    pub(super) home: &'a Path,
}

impl BuildkitCommand {
    pub(super) fn new(
        executable: impl Into<PathBuf>,
        connection: BuildkitConnection,
    ) -> Result<Self, BuildkitCommandError> {
        let executable = executable.into();
        if !is_executable(&executable) {
            return Err(BuildkitCommandError::ExecutableUnavailable);
        }
        let executable = executable
            .canonicalize()
            .map_err(|_| BuildkitCommandError::ExecutableUnavailable)?;
        Ok(Self {
            executable,
            connection,
        })
    }

    pub(super) async fn run(
        &self,
        input: BuildkitCommandInput<'_>,
    ) -> Result<(), BuildkitCommandError> {
        let arguments = self.arguments(&input)?;
        let status = Command::new(&self.executable)
            .args(arguments)
            .env_clear()
            .env("HOME", input.home)
            .env("XDG_CONFIG_HOME", input.home)
            .env("DOCKER_CONFIG", input.home)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status()
            .await
            .map_err(|_| BuildkitCommandError::Spawn)?;
        if !status.success() {
            return Err(BuildkitCommandError::Failed);
        }
        Ok(())
    }

    fn arguments(
        &self,
        input: &BuildkitCommandInput<'_>,
    ) -> Result<Vec<OsString>, BuildkitCommandError> {
        let source = path_text(input.source)?;
        let context = path_text(input.context)?;
        let layout = path_text(input.layout)?;
        let metadata = path_text(input.metadata)?;
        if layout.contains(',') {
            return Err(BuildkitCommandError::Spawn);
        }
        let mut arguments = self.connection.arguments();
        arguments.extend([
            "build".into(),
            "--frontend".into(),
            "dockerfile.v0".into(),
            "--progress".into(),
            "plain".into(),
            "--local".into(),
            format!("context={context}").into(),
            "--local".into(),
            format!("dockerfile={source}").into(),
            "--opt".into(),
            format!("filename={}", input.recipe.dockerfile_path()).into(),
            "--opt".into(),
            format!(
                "platform={}",
                input
                    .recipe
                    .platforms()
                    .iter()
                    .map(|platform| platform.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .into(),
            "--opt".into(),
            "force-network-mode=none".into(),
        ]);
        if let Some(target) = input.recipe.target() {
            arguments.extend(["--opt".into(), format!("target={target}").into()]);
        }
        arguments.extend([
            "--output".into(),
            format!("type=oci,dest={layout},tar=false,oci-mediatypes=true").into(),
            "--metadata-file".into(),
            metadata.into(),
        ]);
        Ok(arguments)
    }
}

fn path_text(path: &Path) -> Result<&str, BuildkitCommandError> {
    path.to_str().ok_or(BuildkitCommandError::Spawn)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
