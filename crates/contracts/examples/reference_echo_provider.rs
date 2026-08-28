use a3s_cloud_contracts::{
    AgentProviderCommandReceiptV1, AgentProviderCommandV1, AgentProviderEventPageRequestV1,
    AgentProviderEventPageV1, AgentProviderEventRecordV1, AgentProviderProfile,
    AgentProviderRunIdentityV1, AgentProviderRunStartV1, AgentProviderRunStateV1,
    AgentProviderSemanticEventV1, AgentProviderToolPayloadIdentityV1, HarnessToolBindingV1,
    AGENT_PROVIDER_COMMAND_HTTP_PATH_V1, AGENT_PROVIDER_EVENT_PAGE_HTTP_PATH_V1,
};
use std::collections::HashMap;
use std::error::Error;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{SystemTime, UNIX_EPOCH};

const REFERENCE_ECHO_PROVIDER_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));
const MAX_HTTP_HEADER_BYTES: usize = 64 * 1024;
const MAX_HTTP_BODY_BYTES: usize = 2 * 1024 * 1024;
const APPROVAL_PROMPT: &str = "Request one governed Tool approval.";

type FixtureResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone)]
struct ReferenceRun {
    identity: AgentProviderRunIdentityV1,
    state: AgentProviderRunStateV1,
    event: AgentProviderEventRecordV1,
    pending_approval: Option<PendingApproval>,
}

#[derive(Clone)]
struct PendingApproval {
    call_id: String,
    tool: HarnessToolBindingV1,
    request_digest: String,
}

#[derive(Clone)]
struct AcceptedCommand {
    digest: String,
    receipt: AgentProviderCommandReceiptV1,
}

struct FixtureState {
    profile: AgentProviderProfile,
    runs: HashMap<String, ReferenceRun>,
    commands: HashMap<String, AcceptedCommand>,
}

impl FixtureState {
    fn new() -> FixtureResult<Self> {
        let profile = AgentProviderProfile::parse_acl(REFERENCE_ECHO_PROVIDER_PROFILE_ACL)
            .map_err(invalid)?;
        Ok(Self {
            profile,
            runs: HashMap::new(),
            commands: HashMap::new(),
        })
    }

    fn accept_command(
        &mut self,
        command: AgentProviderCommandV1,
    ) -> FixtureResult<AgentProviderCommandReceiptV1> {
        command.validate_for(&self.profile).map_err(invalid)?;
        let digest = command.digest().map_err(invalid)?;
        if let Some(accepted) = self.commands.get(command.request_id()) {
            if accepted.digest != digest {
                return Err(
                    invalid("duplicate provider request changed its command digest").into(),
                );
            }
            let mut receipt = accepted.receipt.clone();
            receipt.replayed = true;
            receipt
                .validate_for(&self.profile, &command)
                .map_err(invalid)?;
            return Ok(receipt);
        }

        let observed_at_ms = now_ms()?;
        let state = match &command {
            AgentProviderCommandV1::Start { request } => {
                if self.runs.contains_key(&request.identity.run_id) {
                    return Err(invalid("provider run already exists").into());
                }
                let (state, event, pending_approval) =
                    reference_start_event(request, observed_at_ms)?;
                self.runs.insert(
                    request.identity.run_id.clone(),
                    ReferenceRun {
                        identity: request.identity.clone(),
                        state,
                        event,
                        pending_approval,
                    },
                );
                state
            }
            AgentProviderCommandV1::Cancel { request } => {
                let run = self
                    .runs
                    .get_mut(&request.identity.run_id)
                    .ok_or_else(|| invalid("provider cancellation run does not exist"))?;
                if run.identity != request.identity {
                    return Err(invalid("provider cancellation changed its run identity").into());
                }
                run.state = AgentProviderRunStateV1::Cancelled;
                run.pending_approval = None;
                run.state
            }
            AgentProviderCommandV1::Resume { request } => {
                let run = self
                    .runs
                    .get_mut(&request.identity.run_id)
                    .ok_or_else(|| invalid("provider resume run does not exist"))?;
                if run.identity != request.identity
                    || run.state != AgentProviderRunStateV1::AwaitingApproval
                {
                    return Err(invalid("provider resume has no matching approval state").into());
                }
                let pending = run
                    .pending_approval
                    .as_ref()
                    .ok_or_else(|| invalid("provider resume omitted its pending Tool request"))?;
                if request.decision.call_id != pending.call_id
                    || request.decision.tool != pending.tool
                    || request.decision.request_digest != pending.request_digest
                {
                    return Err(invalid(
                        "provider resume changed its exact pending Tool approval identity",
                    )
                    .into());
                }
                run.state = AgentProviderRunStateV1::Executing;
                run.pending_approval = None;
                run.state
            }
            AgentProviderCommandV1::Recover { .. } => {
                return Err(invalid("reference provider does not support recovery").into());
            }
        };
        let receipt = AgentProviderCommandReceiptV1::accepted(
            &self.profile,
            &command,
            state,
            observed_at_ms,
            false,
        )
        .map_err(invalid)?;
        self.commands.insert(
            command.request_id().into(),
            AcceptedCommand {
                digest,
                receipt: receipt.clone(),
            },
        );
        Ok(receipt)
    }

    fn event_page(
        &self,
        request: AgentProviderEventPageRequestV1,
    ) -> FixtureResult<AgentProviderEventPageV1> {
        request.validate_for(&self.profile).map_err(invalid)?;
        let run = self
            .runs
            .get(&request.identity.run_id)
            .ok_or_else(|| invalid("provider event run does not exist"))?;
        if run.identity != request.identity {
            return Err(invalid("provider event request changed its run identity").into());
        }
        let (source_first_sequence, source_last_sequence, source_event_count, events) =
            match request.after_event_sequence {
                None => (Some(0), Some(0), 1, vec![run.event.clone()]),
                Some(0) => (None, None, 0, Vec::new()),
                Some(_) => {
                    return Err(invalid("provider event cursor exceeds the reference run").into())
                }
            };
        let page = AgentProviderEventPageV1 {
            schema: AgentProviderEventPageV1::SCHEMA.into(),
            identity: run.identity.clone(),
            after_event_sequence: request.after_event_sequence,
            first_available_sequence: Some(0),
            source_first_sequence,
            source_last_sequence,
            source_event_count,
            latest_sequence_exclusive: 1,
            next_after_event_sequence: Some(0),
            state: run.state,
            observed_at_ms: now_ms()?,
            retention_gap: false,
            has_more: false,
            terminal_failure: None,
            events,
        };
        page.validate_for(&self.profile).map_err(invalid)?;
        Ok(page)
    }
}

fn reference_start_event(
    request: &AgentProviderRunStartV1,
    occurred_at_ms: u64,
) -> FixtureResult<(
    AgentProviderRunStateV1,
    AgentProviderEventRecordV1,
    Option<PendingApproval>,
)> {
    if request.prompt != APPROVAL_PROMPT {
        return Ok((
            AgentProviderRunStateV1::Executing,
            AgentProviderEventRecordV1 {
                sequence: 0,
                occurred_at_ms,
                event: AgentProviderSemanticEventV1::ModelOutput {
                    text: "reference harness output".into(),
                },
            },
            None,
        ));
    }
    let invocation = request
        .invocation_profile
        .as_ref()
        .ok_or_else(|| invalid("governed reference run omitted its invocation profile"))?;
    let mut approval_tools = invocation
        .tools
        .iter()
        .filter(|tool| tool.approval_required);
    let tool = approval_tools
        .next()
        .cloned()
        .ok_or_else(|| invalid("governed reference run omitted its approval-required Tool"))?;
    if approval_tools.next().is_some() {
        return Err(invalid("governed reference run declared multiple approval Tools").into());
    }
    let call_id = "reference-governed-call".to_owned();
    let request_identity = AgentProviderToolPayloadIdentityV1 {
        digest: format!("sha256:{}", "f".repeat(64)),
        size_bytes: 128,
        media_type: "application/json".into(),
    };
    Ok((
        AgentProviderRunStateV1::AwaitingApproval,
        AgentProviderEventRecordV1 {
            sequence: 0,
            occurred_at_ms,
            event: AgentProviderSemanticEventV1::ToolRequest {
                call_id: call_id.clone(),
                tool: tool.clone(),
                request: request_identity.clone(),
            },
        },
        Some(PendingApproval {
            call_id,
            tool,
            request_digest: request_identity.digest,
        }),
    ))
}

fn main() -> FixtureResult {
    let listen = std::env::var("A3S_REFERENCE_ECHO_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:49152".into())
        .parse::<SocketAddr>()?;
    let listener = TcpListener::bind(listen)?;
    let mut state = FixtureState::new()?;
    println!("A3S_REFERENCE_ECHO_PROVIDER_READY listen={listen}");
    for connection in listener.incoming() {
        serve_connection(connection?, &mut state);
    }
    Ok(())
}

fn serve_connection(mut stream: TcpStream, state: &mut FixtureState) {
    if let Err(error) = stream.set_nodelay(true) {
        eprintln!("reference provider could not configure a client connection: {error}");
        return;
    }
    if let Err(request_error) = handle_request(&mut stream, state) {
        if let Err(response_error) = write_json_response(
            &mut stream,
            "400 Bad Request",
            &serde_json::json!({"error": request_error.to_string()}),
        ) {
            // A health checker or caller may close as soon as it has the status
            // it needs. That is a connection-local failure, never a reason to
            // terminate the provider and abandon every admitted run.
            eprintln!(
                "reference provider client disconnected before its error response: {response_error}"
            );
        }
    }
}

fn handle_request(stream: &mut TcpStream, state: &mut FixtureState) -> FixtureResult {
    let (method, path, body) = read_request(stream)?;
    if method == "GET" && path == "/health" {
        return write_json_response(stream, "200 OK", &serde_json::json!({"status": "ok"}));
    }
    if method != "POST" {
        return Err(
            invalid("reference provider accepts only GET health and POST protocol calls").into(),
        );
    }
    if path == AGENT_PROVIDER_COMMAND_HTTP_PATH_V1 {
        let command: AgentProviderCommandV1 = serde_json::from_slice(&body)?;
        let receipt = state.accept_command(command)?;
        return write_json_response(stream, "200 OK", &receipt);
    }
    if path == AGENT_PROVIDER_EVENT_PAGE_HTTP_PATH_V1 {
        let request: AgentProviderEventPageRequestV1 = serde_json::from_slice(&body)?;
        let page = state.event_page(request)?;
        return write_json_response(stream, "200 OK", &page);
    }
    Err(invalid(format!("unsupported reference provider path {path:?}")).into())
}

fn read_request(stream: &mut TcpStream) -> FixtureResult<(String, String, Vec<u8>)> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| invalid("HTTP request omitted its method"))?
        .to_owned();
    let path = parts
        .next()
        .ok_or_else(|| invalid("HTTP request omitted its path"))?
        .to_owned();
    let mut header_bytes = request_line.len();
    let mut content_length = 0_usize;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Err(invalid("HTTP request ended before its headers").into());
        }
        header_bytes = header_bytes
            .checked_add(read)
            .ok_or_else(|| invalid("HTTP header size overflowed"))?;
        if header_bytes > MAX_HTTP_HEADER_BYTES {
            return Err(invalid("HTTP request headers exceed the fixture bound").into());
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse()?;
            }
        }
    }
    if content_length > MAX_HTTP_BODY_BYTES {
        return Err(invalid("HTTP request body exceeds the fixture bound").into());
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok((method, path, body))
}

fn write_json_response(
    stream: &mut TcpStream,
    status: &str,
    value: &impl serde::Serialize,
) -> FixtureResult {
    let body = serde_json::to_vec(value)?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

fn now_ms() -> FixtureResult<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
#[path = "reference_echo_provider/tests.rs"]
mod tests;
