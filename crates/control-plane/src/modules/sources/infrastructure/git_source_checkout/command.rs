use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const MAX_COMMAND_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(super) enum GitCommandError {
    #[error("Git executable is unavailable")]
    ExecutableUnavailable,
    #[error("Git command could not be started")]
    Spawn,
    #[error("Git command exceeded its deadline")]
    Timeout,
    #[error("Git command output exceeded its bound")]
    OutputLimit,
    #[error("Git command failed")]
    Failed,
}

pub(super) struct GitCommandRunner {
    executable: PathBuf,
    timeout: Duration,
    allow_file_protocol: bool,
}

impl GitCommandRunner {
    pub(super) fn discover(
        timeout: Duration,
        allow_file_protocol: bool,
    ) -> Result<Self, GitCommandError> {
        Ok(Self {
            executable: find_executable("git")?,
            timeout,
            allow_file_protocol,
        })
    }

    pub(super) async fn run(
        &self,
        working_directory: &Path,
        home: &Path,
        hooks: &Path,
        args: &[OsString],
    ) -> Result<Vec<u8>, GitCommandError> {
        let mut command = Command::new(&self.executable);
        command
            .current_dir(working_directory)
            .env_clear()
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .arg("-c")
            .arg("advice.detachedHead=false")
            .arg("-c")
            .arg("core.autocrlf=false")
            .arg("-c")
            .arg("core.eol=lf")
            .arg("-c")
            .arg("core.protectHFS=true")
            .arg("-c")
            .arg("core.protectNTFS=true")
            .arg("-c")
            .arg(format!("core.hooksPath={}", hooks.display()))
            .arg("-c")
            .arg("credential.helper=")
            .arg("-c")
            .arg("fetch.fsckObjects=true")
            .arg("-c")
            .arg("fetch.writeCommitGraph=false")
            .arg("-c")
            .arg("http.followRedirects=false")
            .arg("-c")
            .arg("http.sslVerify=true")
            .arg("-c")
            .arg("protocol.allow=never")
            .arg("-c")
            .arg("protocol.https.allow=always")
            .arg("-c")
            .arg(if self.allow_file_protocol {
                "protocol.file.allow=always"
            } else {
                "protocol.file.allow=never"
            })
            .arg("-c")
            .arg("submodule.recurse=false")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| GitCommandError::Spawn)?;
        let stdout = child.stdout.take().ok_or(GitCommandError::Spawn)?;
        let stderr = child.stderr.take().ok_or(GitCommandError::Spawn)?;
        let completed = tokio::time::timeout(self.timeout, async {
            let (stdout, _stderr, status) =
                tokio::try_join!(read_bounded(stdout), read_bounded(stderr), child.wait())?;
            Ok::<_, io::Error>((stdout, status))
        })
        .await;
        let (stdout, status) = match completed {
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(GitCommandError::Timeout);
            }
            Ok(Err(error)) if error.kind() == io::ErrorKind::FileTooLarge => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(GitCommandError::OutputLimit);
            }
            Ok(Err(_)) => return Err(GitCommandError::Spawn),
            Ok(Ok(result)) => result,
        };
        if !status.success() {
            return Err(GitCommandError::Failed);
        }
        Ok(stdout)
    }
}

async fn read_bounded(mut stream: impl AsyncRead + Unpin) -> Result<Vec<u8>, std::io::Error> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Ok(output);
        }
        if output
            .len()
            .checked_add(read)
            .is_none_or(|length| length > MAX_COMMAND_OUTPUT_BYTES)
        {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "Git command output exceeded its bound",
            ));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

fn find_executable(name: &str) -> Result<PathBuf, GitCommandError> {
    let path = std::env::var_os("PATH").ok_or(GitCommandError::ExecutableUnavailable)?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if is_executable(&candidate) {
            return candidate
                .canonicalize()
                .map_err(|_| GitCommandError::ExecutableUnavailable);
        }
    }
    Err(GitCommandError::ExecutableUnavailable)
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
