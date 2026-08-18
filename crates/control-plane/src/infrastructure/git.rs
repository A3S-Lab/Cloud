use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use zeroize::Zeroizing;

const MAX_COMMAND_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const GIT_HTTP_EXTRA_HEADER_ENV: &str = "A3S_CLOUD_GIT_HTTP_EXTRA_HEADER";

#[derive(Debug, thiserror::Error)]
pub(crate) enum GitCommandError {
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

pub(crate) struct GitCommandRunner {
    executable: PathBuf,
    exec_path: PathBuf,
    timeout: Duration,
    allow_file_protocol: bool,
    allow_http_protocol: bool,
}

impl GitCommandRunner {
    /// Converts a canonical host path into the representation accepted by the
    /// external Git executable.
    ///
    /// Rust preserves Windows verbatim prefixes after canonicalization, while
    /// Git for Windows rejects those prefixes for commands such as `init`.
    /// Filesystem ownership checks still use the canonical path; only the
    /// process-boundary representation is normalized.
    pub(crate) fn normalize_path(path: PathBuf) -> PathBuf {
        normalize_external_path(path)
    }

    pub(crate) fn discover(
        timeout: Duration,
        allow_file_protocol: bool,
        allow_http_protocol: bool,
    ) -> Result<Self, GitCommandError> {
        let executable = find_executable("git")?;
        let exec_path = find_exec_path(&executable)?;
        Ok(Self {
            executable,
            exec_path,
            timeout,
            allow_file_protocol,
            allow_http_protocol,
        })
    }

    pub(crate) async fn run(
        &self,
        working_directory: &Path,
        home: &Path,
        hooks: &Path,
        args: &[OsString],
        provider_token: Option<&str>,
    ) -> Result<Vec<u8>, GitCommandError> {
        self.run_inner(working_directory, home, hooks, args, provider_token, None)
            .await
    }

    pub(crate) async fn run_with_input(
        &self,
        working_directory: &Path,
        home: &Path,
        hooks: &Path,
        args: &[OsString],
        input: Vec<u8>,
    ) -> Result<Vec<u8>, GitCommandError> {
        self.run_inner(working_directory, home, hooks, args, None, Some(input))
            .await
    }

    async fn run_inner(
        &self,
        working_directory: &Path,
        home: &Path,
        hooks: &Path,
        args: &[OsString],
        provider_token: Option<&str>,
        input: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, GitCommandError> {
        let authentication_header = provider_token.map(|token| {
            let credentials = Zeroizing::new(format!("x-access-token:{token}"));
            Zeroizing::new(format!(
                "Authorization: Basic {}",
                STANDARD.encode(credentials.as_bytes())
            ))
        });
        let mut command = Command::new(&self.executable);
        command
            .current_dir(working_directory)
            .env_clear()
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_EXEC_PATH", &self.exec_path)
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
            .arg(if self.allow_http_protocol {
                "protocol.http.allow=always"
            } else {
                "protocol.http.allow=never"
            })
            .arg("-c")
            .arg("submodule.recurse=false")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(header) = authentication_header.as_ref() {
            command
                .env(GIT_HTTP_EXTRA_HEADER_ENV, header.as_str())
                .arg(format!(
                    "--config-env=http.extraHeader={GIT_HTTP_EXTRA_HEADER_ENV}"
                ));
        }
        command.args(args);
        let mut child = command.spawn().map_err(|_| GitCommandError::Spawn)?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or(GitCommandError::Spawn)?;
        let stderr = child.stderr.take().ok_or(GitCommandError::Spawn)?;
        let completed = tokio::time::timeout(self.timeout, async {
            let write = async move {
                match (stdin, input) {
                    (Some(mut stdin), Some(input)) => {
                        stdin.write_all(&input).await?;
                        stdin.shutdown().await
                    }
                    (None, None) => Ok(()),
                    _ => Err(io::Error::other("Git command stdin configuration changed")),
                }
            };
            let (stdout, stderr, (), status) = tokio::try_join!(
                read_bounded(stdout),
                read_bounded(stderr),
                write,
                child.wait()
            )?;
            Ok::<_, io::Error>((stdout, stderr, status))
        })
        .await;
        let (stdout, _stderr, status) = match completed {
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
        if let Some(candidate) = resolve_executable(&directory.join(name)) {
            return candidate
                .canonicalize()
                .map(normalize_external_path)
                .map_err(|_| GitCommandError::ExecutableUnavailable);
        }
    }
    Err(GitCommandError::ExecutableUnavailable)
}

fn find_exec_path(executable: &Path) -> Result<PathBuf, GitCommandError> {
    let output = std::process::Command::new(executable)
        .arg("--exec-path")
        .env_clear()
        .env("LC_ALL", "C")
        .output()
        .map_err(|_| GitCommandError::ExecutableUnavailable)?;
    if !output.status.success() {
        return Err(GitCommandError::ExecutableUnavailable);
    }
    let path = std::str::from_utf8(&output.stdout)
        .map_err(|_| GitCommandError::ExecutableUnavailable)?
        .trim_end_matches(['\r', '\n']);
    if path.is_empty() || path.contains(['\r', '\n', '\0']) {
        return Err(GitCommandError::ExecutableUnavailable);
    }
    let path = PathBuf::from(path)
        .canonicalize()
        .map(normalize_external_path)
        .map_err(|_| GitCommandError::ExecutableUnavailable)?;
    if !path.is_dir() || !is_valid_exec_path(&path) {
        return Err(GitCommandError::ExecutableUnavailable);
    }
    Ok(path)
}

#[cfg(not(windows))]
fn normalize_external_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn normalize_external_path(path: PathBuf) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    const SEPARATOR: u16 = b'\\' as u16;
    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let verbatim = units.starts_with(&[SEPARATOR, SEPARATOR, b'?' as u16, SEPARATOR]);
    if !verbatim {
        return path;
    }
    let drive = units.get(4).copied().unwrap_or_default();
    if units.len() >= 7
        && ((b'A' as u16..=b'Z' as u16).contains(&drive)
            || (b'a' as u16..=b'z' as u16).contains(&drive))
        && units[5] == b':' as u16
        && units[6] == SEPARATOR
    {
        return PathBuf::from(OsString::from_wide(&units[4..]));
    }
    if units.len() >= 8
        && matches!(units[4], value if value == b'U' as u16 || value == b'u' as u16)
        && matches!(units[5], value if value == b'N' as u16 || value == b'n' as u16)
        && matches!(units[6], value if value == b'C' as u16 || value == b'c' as u16)
        && units[7] == SEPARATOR
    {
        let mut normalized = vec![SEPARATOR, SEPARATOR];
        normalized.extend_from_slice(&units[8..]);
        return PathBuf::from(OsString::from_wide(&normalized));
    }
    path
}

#[cfg(not(windows))]
fn is_valid_exec_path(path: &Path) -> bool {
    resolve_executable(&path.join("git-upload-pack")).is_some()
        && resolve_executable(&path.join("git-receive-pack")).is_some()
}

#[cfg(windows)]
fn is_valid_exec_path(path: &Path) -> bool {
    // Git for Windows dispatches upload-pack and receive-pack as built-ins from
    // git-core/git.exe instead of installing separate helper executables.
    resolve_executable(&path.join("git")).is_some()
}

#[cfg(not(windows))]
fn resolve_executable(path: &Path) -> Option<PathBuf> {
    is_executable(path).then(|| path.to_owned())
}

#[cfg(windows)]
fn resolve_executable(path: &Path) -> Option<PathBuf> {
    if is_executable(path) {
        return Some(path.to_owned());
    }
    let path_extensions =
        std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
    resolve_windows_executable(path, &path_extensions.to_string_lossy())
}

#[cfg(windows)]
fn resolve_windows_executable(path: &Path, path_extensions: &str) -> Option<PathBuf> {
    path_extensions.split(';').find_map(|extension| {
        let extension = extension.trim();
        if extension.len() < 2
            || extension.len() > 16
            || !extension.starts_with('.')
            || !extension[1..]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return None;
        }
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(extension);
        let candidate = PathBuf::from(candidate);
        is_executable(&candidate).then_some(candidate)
    })
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

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn windows_git_tools_resolve_through_pathext() {
        let directory = tempfile::tempdir().expect("temporary Git tool directory");
        let executable = directory.path().join("git-upload-pack.exe");
        std::fs::write(&executable, []).expect("stub Git tool");

        let resolved = super::resolve_windows_executable(
            &directory.path().join("git-upload-pack"),
            ".COM;.EXE;.BAT;.CMD",
        )
        .expect("PATHEXT-resolved Git tool");
        assert!(resolved.is_file());
        assert!(resolved
            .to_string_lossy()
            .eq_ignore_ascii_case(&executable.to_string_lossy()));

        std::fs::write(directory.path().join("git.exe"), []).expect("stub Git dispatcher");
        assert!(super::is_valid_exec_path(directory.path()));
    }

    #[cfg(windows)]
    #[test]
    fn windows_verbatim_paths_are_normalized_only_for_git() {
        for (canonical, external) in [
            (r"\\?\C:\a3s\repository.git", r"C:\a3s\repository.git"),
            (
                r"\\?\UNC\git-host\a3s\repository.git",
                r"\\git-host\a3s\repository.git",
            ),
        ] {
            assert_eq!(
                super::normalize_external_path(canonical.into()),
                std::path::PathBuf::from(external)
            );
        }
        let ordinary = std::path::PathBuf::from(r"C:\a3s\repository.git");
        assert_eq!(super::normalize_external_path(ordinary.clone()), ordinary);
    }

    #[test]
    fn git_consumers_reuse_the_shared_command_runner() {
        for (name, source) in [
            (
                "Source checkout",
                include_str!("../modules/sources/infrastructure/git_source_checkout.rs"),
            ),
            (
                "Asset repository",
                include_str!("../modules/assets/infrastructure/git_repository/mod.rs"),
            ),
        ] {
            assert!(
                source.contains("GitCommandRunner"),
                "{name} must use GitCommandRunner"
            );
            for forbidden in [
                "tokio::process::Command",
                "std::process::Command",
                "Command::new(",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{name} must reuse GitCommandRunner; found {forbidden}"
                );
            }
        }
    }
}
