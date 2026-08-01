use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Method};
use serde_json::{json, Map, Value};
use tokio::time::Instant;
use url::Url;

pub const HELP: &str = r#"A3S Workflow coding-agent CLI

Usage:
  a3s-workflow [--server URL] [--token TOKEN] [--compact] <command>

Commands:
  health
  node-types
  workflow list
  workflow get <workflow-id>
  workflow apply <workflow.json>
  run start <workflow-id> [--input <json|@file>]
  run get <run-id>
  run wait <run-id> [--timeout-seconds N] [--poll-ms N]
  run evidence <run-id>
  run approve <run-id> <node-id> [--payload <json|@file>]
  help
  version

Environment:
  A3S_WORKFLOW_URL        Control-plane URL (default http://127.0.0.1:8080)
  A3S_WORKFLOW_API_TOKEN  Optional bearer token
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub server: String,
    pub token: Option<String>,
    pub compact: bool,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Health,
    NodeTypes,
    WorkflowList,
    WorkflowGet {
        workflow_id: String,
    },
    WorkflowApply {
        path: String,
    },
    RunStart {
        workflow_id: String,
        input: String,
    },
    RunGet {
        run_id: String,
    },
    RunWait {
        run_id: String,
        timeout_seconds: u64,
        poll_ms: u64,
    },
    RunEvidence {
        run_id: String,
    },
    RunApprove {
        run_id: String,
        node_id: String,
        payload: String,
    },
}

pub fn parse(
    args: impl IntoIterator<Item = String>,
    default_server: &str,
    default_token: Option<String>,
) -> Result<Cli> {
    let mut args = args.into_iter().collect::<VecDeque<_>>();
    let mut server = default_server.to_string();
    let mut token = default_token;
    let mut compact = false;
    loop {
        match args.front().map(String::as_str) {
            Some("--server") => {
                args.pop_front();
                server = pop(&mut args, "--server requires a URL")?;
            }
            Some("--token") => {
                args.pop_front();
                token = Some(pop(&mut args, "--token requires a value")?);
            }
            Some("--compact") => {
                args.pop_front();
                compact = true;
            }
            Some("--help" | "-h") => {
                args.pop_front();
                ensure_empty(&args)?;
                return Ok(Cli {
                    server,
                    token,
                    compact,
                    command: Command::Help,
                });
            }
            Some("--version" | "-V") => {
                args.pop_front();
                ensure_empty(&args)?;
                return Ok(Cli {
                    server,
                    token,
                    compact,
                    command: Command::Version,
                });
            }
            _ => break,
        }
    }

    validate_server(&server)?;
    let command = match args.pop_front().as_deref() {
        None | Some("help") => Command::Help,
        Some("version") => Command::Version,
        Some("health") => Command::Health,
        Some("node-types") => Command::NodeTypes,
        Some("workflow") => parse_workflow(&mut args)?,
        Some("run") => parse_run(&mut args)?,
        Some(value) => bail!("unknown command {value:?}\n\n{HELP}"),
    };
    ensure_empty(&args)?;
    Ok(Cli {
        server: server.trim_end_matches('/').to_string(),
        token: token.filter(|value| !value.is_empty()),
        compact,
        command,
    })
}

fn parse_workflow(args: &mut VecDeque<String>) -> Result<Command> {
    match args.pop_front().as_deref() {
        Some("list") => Ok(Command::WorkflowList),
        Some("get") => Ok(Command::WorkflowGet {
            workflow_id: identifier(
                "workflow ID",
                pop(args, "workflow get requires <workflow-id>")?,
            )?,
        }),
        Some("apply") => Ok(Command::WorkflowApply {
            path: pop(args, "workflow apply requires <workflow.json>")?,
        }),
        Some(value) => bail!("unknown workflow command {value:?}"),
        None => bail!("workflow requires list, get, or apply"),
    }
}

fn parse_run(args: &mut VecDeque<String>) -> Result<Command> {
    match args.pop_front().as_deref() {
        Some("start") => {
            let workflow_id = identifier(
                "workflow ID",
                pop(args, "run start requires <workflow-id>")?,
            )?;
            let options = options(args, &["--input"])?;
            Ok(Command::RunStart {
                workflow_id,
                input: options
                    .get("--input")
                    .cloned()
                    .unwrap_or_else(|| "{}".to_string()),
            })
        }
        Some("get") => Ok(Command::RunGet {
            run_id: identifier("run ID", pop(args, "run get requires <run-id>")?)?,
        }),
        Some("wait") => {
            let run_id = identifier("run ID", pop(args, "run wait requires <run-id>")?)?;
            let options = options(args, &["--timeout-seconds", "--poll-ms"])?;
            Ok(Command::RunWait {
                run_id,
                timeout_seconds: positive_option(&options, "--timeout-seconds", 300)?,
                poll_ms: positive_option(&options, "--poll-ms", 500)?,
            })
        }
        Some("evidence") => Ok(Command::RunEvidence {
            run_id: identifier("run ID", pop(args, "run evidence requires <run-id>")?)?,
        }),
        Some("approve") => {
            let run_id = identifier("run ID", pop(args, "run approve requires <run-id>")?)?;
            let node_id = identifier("node ID", pop(args, "run approve requires <node-id>")?)?;
            let options = options(args, &["--payload"])?;
            Ok(Command::RunApprove {
                run_id,
                node_id,
                payload: options
                    .get("--payload")
                    .cloned()
                    .unwrap_or_else(|| r#"{"approved":true}"#.to_string()),
            })
        }
        Some(value) => bail!("unknown run command {value:?}"),
        None => bail!("run requires start, get, wait, evidence, or approve"),
    }
}

fn options(args: &mut VecDeque<String>, allowed: &[&str]) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    while let Some(name) = args.pop_front() {
        if !allowed.contains(&name.as_str()) {
            bail!("unknown option {name:?}");
        }
        if values.contains_key(&name) {
            bail!("duplicate option {name:?}");
        }
        let value = pop(args, &format!("{name} requires a value"))?;
        values.insert(name, value);
    }
    Ok(values)
}

fn positive_option(values: &BTreeMap<String, String>, name: &str, default: u64) -> Result<u64> {
    let Some(value) = values.get(name) else {
        return Ok(default);
    };
    let parsed = value
        .parse::<u64>()
        .with_context(|| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        bail!("{name} must be a positive integer");
    }
    Ok(parsed)
}

fn pop(args: &mut VecDeque<String>, message: &str) -> Result<String> {
    args.pop_front().ok_or_else(|| anyhow!(message.to_string()))
}

fn ensure_empty(args: &VecDeque<String>) -> Result<()> {
    if let Some(value) = args.front() {
        bail!("unexpected argument {value:?}");
    }
    Ok(())
}

fn identifier(label: &str, value: String) -> Result<String> {
    let valid = !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if !valid {
        bail!("{label} must use bounded ASCII letters, numbers, hyphens, or underscores");
    }
    Ok(value)
}

fn validate_server(value: &str) -> Result<()> {
    let url = Url::parse(value).context("--server is not a valid URL")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("--server must be an absolute http:// or https:// URL");
    }
    Ok(())
}

#[derive(Clone)]
struct ApiClient {
    base_url: String,
    token: Option<String>,
    client: Client,
}

impl ApiClient {
    fn new(server: String, token: Option<String>) -> Result<Self> {
        validate_server(&server)?;
        Ok(Self {
            base_url: server.trim_end_matches('/').to_string(),
            token,
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .context("failed to create HTTP client")?,
        })
    }

    async fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .header("accept", "application/json");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .context("Workflow API request failed")?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .context("failed to read Workflow API response")?;
        if !status.is_success() {
            let message = String::from_utf8_lossy(&bytes);
            bail!(
                "Workflow API returned {status}: {}",
                if message.trim().is_empty() {
                    "empty response"
                } else {
                    message.trim()
                }
            );
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).context("Workflow API returned invalid JSON")
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, None).await
    }
}

pub async fn execute(cli: Cli) -> Result<Value> {
    let client = ApiClient::new(cli.server, cli.token)?;
    match cli.command {
        Command::Help | Command::Version => bail!("help and version do not execute API requests"),
        Command::Health => client.get("/api/health").await,
        Command::NodeTypes => client.get("/api/v1/node-types").await,
        Command::WorkflowList => client.get("/api/v1/workflows").await,
        Command::WorkflowGet { workflow_id } => {
            client
                .get(&format!("/api/v1/workflows/{workflow_id}"))
                .await
        }
        Command::WorkflowApply { path } => {
            let source = read_json_file(&path).await?;
            let body = mutable_workflow_body(&source)?;
            if let Some(id) = source.get("id").and_then(Value::as_str) {
                let id = identifier("workflow ID", id.to_string())?;
                client
                    .request(Method::PUT, &format!("/api/v1/workflows/{id}"), Some(&body))
                    .await
            } else {
                client
                    .request(Method::POST, "/api/v1/workflows", Some(&body))
                    .await
            }
        }
        Command::RunStart { workflow_id, input } => {
            let input = read_json_argument(&input).await?;
            client
                .request(
                    Method::POST,
                    &format!("/api/v1/workflows/{workflow_id}/runs"),
                    Some(&json!({ "input": input })),
                )
                .await
        }
        Command::RunGet { run_id } => client.get(&format!("/api/v1/runs/{run_id}")).await,
        Command::RunWait {
            run_id,
            timeout_seconds,
            poll_ms,
        } => wait_for_run(&client, &run_id, timeout_seconds, poll_ms).await,
        Command::RunEvidence { run_id } => {
            client
                .get(&format!("/api/v1/runs/{run_id}/node-executions"))
                .await
        }
        Command::RunApprove {
            run_id,
            node_id,
            payload,
        } => {
            let payload = read_json_argument(&payload).await?;
            client
                .request(
                    Method::POST,
                    &format!("/api/v1/runs/{run_id}/approvals/{node_id}"),
                    Some(&json!({ "payload": payload })),
                )
                .await
        }
    }
}

async fn wait_for_run(
    client: &ApiClient,
    run_id: &str,
    timeout_seconds: u64,
    poll_ms: u64,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let value = client.get(&format!("/api/v1/runs/{run_id}")).await?;
        let status = value
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("run response is missing status"))?;
        if status != "running" {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for run {run_id}");
        }
        tokio::time::sleep(Duration::from_millis(poll_ms)).await;
    }
}

async fn read_json_file(path: &str) -> Result<Value> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read {path}"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("{path} is not valid JSON"))
}

async fn read_json_argument(source: &str) -> Result<Value> {
    if let Some(path) = source.strip_prefix('@') {
        if path.is_empty() {
            bail!("@ JSON input requires a file path");
        }
        read_json_file(path).await
    } else {
        serde_json::from_str(source).context("argument is not valid JSON")
    }
}

fn mutable_workflow_body(source: &Value) -> Result<Value> {
    let object = source
        .as_object()
        .ok_or_else(|| anyhow!("workflow file must contain a JSON object"))?;
    let mut body = Map::new();
    for field in ["version", "name", "description", "nodes", "edges"] {
        if let Some(value) = object.get(field) {
            body.insert(field.to_string(), value.clone());
        }
    }
    for required in ["name", "nodes", "edges"] {
        if !body.contains_key(required) {
            bail!("workflow file is missing {required}");
        }
    }
    Ok(Value::Object(body))
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn parses_global_options_and_all_command_families() {
        let cli = parse(
            args(&[
                "--server",
                "https://workflow.example.test/",
                "--token",
                "secret",
                "--compact",
                "run",
                "wait",
                "run-1",
                "--timeout-seconds",
                "30",
                "--poll-ms",
                "10",
            ]),
            "http://localhost:8080",
            None,
        )
        .expect("parse run wait");
        assert_eq!(cli.server, "https://workflow.example.test");
        assert_eq!(cli.token.as_deref(), Some("secret"));
        assert!(cli.compact);
        assert_eq!(
            cli.command,
            Command::RunWait {
                run_id: "run-1".to_string(),
                timeout_seconds: 30,
                poll_ms: 10,
            }
        );

        assert_eq!(
            parse(
                args(&["workflow", "get", "workflow_1"]),
                "http://localhost:8080",
                None
            )
            .expect("workflow get")
            .command,
            Command::WorkflowGet {
                workflow_id: "workflow_1".to_string()
            }
        );
        assert_eq!(
            parse(
                args(&["run", "approve", "run-1", "approval-1"]),
                "http://localhost:8080",
                None
            )
            .expect("run approve")
            .command,
            Command::RunApprove {
                run_id: "run-1".to_string(),
                node_id: "approval-1".to_string(),
                payload: r#"{"approved":true}"#.to_string()
            }
        );
    }

    #[test]
    fn parser_rejects_unsafe_ids_urls_options_and_zero_waits() {
        for values in [
            vec!["--server", "file:///tmp/workflow", "health"],
            vec!["run", "get", "../secret"],
            vec!["run", "wait", "run-1", "--poll-ms", "0"],
            vec!["run", "start", "workflow-1", "--unknown", "value"],
            vec!["workflow", "unknown"],
        ] {
            assert!(parse(args(&values), "http://localhost:8080", None).is_err());
        }
    }

    #[tokio::test]
    async fn reads_inline_and_file_json_inputs() {
        assert_eq!(
            read_json_argument(r#"{"name":"Ada"}"#)
                .await
                .expect("inline JSON"),
            json!({ "name": "Ada" })
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("input.json");
        tokio::fs::write(&path, b"[1,2,3]")
            .await
            .expect("write JSON input");
        assert_eq!(
            read_json_argument(&format!("@{}", path.display()))
                .await
                .expect("file JSON"),
            json!([1, 2, 3])
        );
        assert!(read_json_argument("@").await.is_err());
        assert!(read_json_argument("not-json").await.is_err());
    }

    #[test]
    fn workflow_payload_strips_server_fields_and_requires_graph_fields() {
        assert_eq!(
            mutable_workflow_body(&json!({
                "id": "workflow-1",
                "version": 2,
                "name": "Demo",
                "description": "test",
                "nodes": [],
                "edges": [],
                "createdAt": "ignored"
            }))
            .expect("workflow body"),
            json!({
                "version": 2,
                "name": "Demo",
                "description": "test",
                "nodes": [],
                "edges": []
            })
        );
        assert!(mutable_workflow_body(&json!({ "name": "missing graph" })).is_err());
        assert!(mutable_workflow_body(&json!([])).is_err());
    }

    async fn mock_server(
        responses: Vec<(u16, Value)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock API");
        let address = listener.local_addr().expect("mock API address");
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.expect("accept API request");
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                let read = socket.read(&mut buffer).await.expect("read API request");
                bytes.extend_from_slice(&buffer[..read]);
                requests.push(String::from_utf8_lossy(&bytes).into_owned());
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status} Test\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write API response");
            }
            requests
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn executes_health_and_bearer_authenticated_requests() {
        let (server, task) = mock_server(vec![(200, json!({ "status": "ok" }))]).await;
        let value = execute(Cli {
            server,
            token: Some("secret".to_string()),
            compact: false,
            command: Command::Health,
        })
        .await
        .expect("health request");
        assert_eq!(value, json!({ "status": "ok" }));
        let requests = task.await.expect("mock API task");
        assert!(requests[0].starts_with("GET /api/health HTTP/1.1"));
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("authorization: bearer secret"));
    }

    #[tokio::test]
    async fn waits_until_a_run_reaches_a_terminal_state() {
        let (server, task) = mock_server(vec![
            (200, json!({ "status": "running" })),
            (200, json!({ "status": "completed", "output": 42 })),
        ])
        .await;
        let value = execute(Cli {
            server,
            token: None,
            compact: false,
            command: Command::RunWait {
                run_id: "run-1".to_string(),
                timeout_seconds: 1,
                poll_ms: 1,
            },
        })
        .await
        .expect("wait for run");
        assert_eq!(value["status"], "completed");
        assert_eq!(task.await.expect("mock API task").len(), 2);
    }

    #[tokio::test]
    async fn api_errors_include_status_and_response_body() {
        let (server, task) = mock_server(vec![(422, json!({ "error": "invalid graph" }))]).await;
        let error = execute(Cli {
            server,
            token: None,
            compact: false,
            command: Command::WorkflowList,
        })
        .await
        .expect_err("API failure");
        assert!(error.to_string().contains("422"));
        assert!(error.to_string().contains("invalid graph"));
        task.await.expect("mock API task");
    }
}
