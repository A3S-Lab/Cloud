use super::{NativeCodeAgentExecutionProvider, ReferenceEchoAgentExecutionProvider};
use crate::modules::agents::domain::AgentExecutionProvider;
use a3s_cloud_contracts::{
    AgentProviderApprovalDecisionV1, AgentProviderApprovalOutcomeV1,
    AgentProviderCapabilityRequirementsV1, AgentProviderCapabilityV1,
    AgentProviderCommandReceiptV1, AgentProviderCommandV1, AgentProviderEventPageV1,
    AgentProviderEventRecordV1, AgentProviderRunIdentityV1, AgentProviderRunStateV1,
    AgentProviderSemanticEventV1, AgentProviderToolPayloadIdentityV1, HarnessToolBindingV1,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
struct ReferenceRun {
    identity: AgentProviderRunIdentityV1,
    state: AgentProviderRunStateV1,
    events: Vec<AgentProviderEventRecordV1>,
}

#[derive(Debug, Clone)]
struct AcceptedCommand {
    digest: String,
    receipt: AgentProviderCommandReceiptV1,
}

#[derive(Debug, Clone)]
struct ReferenceEchoHarness {
    provider: ReferenceEchoAgentExecutionProvider,
    runs: BTreeMap<String, ReferenceRun>,
    commands: BTreeMap<String, AcceptedCommand>,
    cleaned_runs: BTreeSet<String>,
}

impl ReferenceEchoHarness {
    fn new() -> Self {
        Self {
            provider: ReferenceEchoAgentExecutionProvider::new().expect("reference provider"),
            runs: BTreeMap::new(),
            commands: BTreeMap::new(),
            cleaned_runs: BTreeSet::new(),
        }
    }

    fn restart(&self) -> Self {
        self.clone()
    }

    fn accept(
        &mut self,
        command: &AgentProviderCommandV1,
        observed_at_ms: u64,
    ) -> Result<AgentProviderCommandReceiptV1, String> {
        let profile = self.provider.profile().profile()?;
        command.validate_for(&profile)?;
        let digest = command.digest()?;
        if let Some(accepted) = self.commands.get(command.request_id()) {
            if accepted.digest != digest {
                return Err("duplicate reference request changed its command digest".into());
            }
            let mut receipt = accepted.receipt.clone();
            receipt.replayed = true;
            receipt.validate_for(&profile, command)?;
            return Ok(receipt);
        }
        if self.cleaned_runs.contains(&command.identity().run_id) {
            return Err("cleaned reference run cannot be recreated implicitly".into());
        }
        let state = match command {
            AgentProviderCommandV1::Start { .. } => {
                if self.runs.contains_key(&command.identity().run_id) {
                    return Err("reference run already exists under another request".into());
                }
                self.runs.insert(
                    command.identity().run_id.clone(),
                    ReferenceRun {
                        identity: command.identity().clone(),
                        state: AgentProviderRunStateV1::Executing,
                        events: vec![AgentProviderEventRecordV1 {
                            sequence: 0,
                            occurred_at_ms: observed_at_ms,
                            event: AgentProviderSemanticEventV1::ModelOutput {
                                text: "reference harness output".into(),
                            },
                        }],
                    },
                );
                AgentProviderRunStateV1::Executing
            }
            AgentProviderCommandV1::Cancel { .. } => {
                let run = self
                    .runs
                    .get_mut(&command.identity().run_id)
                    .ok_or_else(|| "reference cancellation run does not exist".to_owned())?;
                if run.identity != *command.identity() {
                    return Err("reference cancellation changed its run identity".into());
                }
                run.state = AgentProviderRunStateV1::Cancelled;
                run.state
            }
            AgentProviderCommandV1::Recover { .. } => {
                return Err("reference provider does not support recovery".into())
            }
            AgentProviderCommandV1::Resume { request } => {
                let run = self
                    .runs
                    .get_mut(&command.identity().run_id)
                    .ok_or_else(|| "reference resume run does not exist".to_owned())?;
                if run.identity != *command.identity()
                    || run.state != AgentProviderRunStateV1::AwaitingApproval
                {
                    return Err("reference resume changed its paused run identity".into());
                }
                let Some(AgentProviderEventRecordV1 {
                    event:
                        AgentProviderSemanticEventV1::ToolRequest {
                            call_id,
                            tool,
                            request: tool_request,
                        },
                    ..
                }) = run.events.last()
                else {
                    return Err("reference resume has no pending Tool request".into());
                };
                if !tool.approval_required
                    || request.decision.call_id != *call_id
                    || request.decision.tool != *tool
                    || request.decision.request_digest != tool_request.digest
                {
                    return Err("reference resume changed its exact Tool approval".into());
                }
                run.state = AgentProviderRunStateV1::Executing;
                run.events.push(AgentProviderEventRecordV1 {
                    sequence: u64::try_from(run.events.len())
                        .map_err(|_| "reference event sequence overflowed".to_owned())?,
                    occurred_at_ms: observed_at_ms,
                    event: AgentProviderSemanticEventV1::ModelOutput {
                        text: format!("approval {} accepted", request.decision.outcome.as_str()),
                    },
                });
                run.state
            }
        };
        let receipt = AgentProviderCommandReceiptV1::accepted(
            &profile,
            command,
            state,
            observed_at_ms,
            false,
        )?;
        self.commands.insert(
            command.request_id().into(),
            AcceptedCommand {
                digest,
                receipt: receipt.clone(),
            },
        );
        Ok(receipt)
    }

    fn page(
        &self,
        identity: &AgentProviderRunIdentityV1,
        after_event_sequence: Option<u64>,
        observed_at_ms: u64,
    ) -> Result<AgentProviderEventPageV1, String> {
        let run = self
            .runs
            .get(&identity.run_id)
            .ok_or_else(|| "reference run does not exist".to_owned())?;
        if run.identity != *identity {
            return Err("reference event request changed its run identity".into());
        }
        let events = run
            .events
            .iter()
            .filter(|event| after_event_sequence.is_none_or(|after| event.sequence > after))
            .cloned()
            .collect::<Vec<_>>();
        let source_event_count = u16::try_from(events.len())
            .map_err(|_| "reference event count exceeds protocol bounds".to_owned())?;
        let latest_sequence_exclusive = u64::try_from(run.events.len())
            .map_err(|_| "reference event sequence exceeds protocol bounds".to_owned())?;
        let page = AgentProviderEventPageV1 {
            schema: AgentProviderEventPageV1::SCHEMA.into(),
            identity: identity.clone(),
            after_event_sequence,
            first_available_sequence: (!run.events.is_empty()).then_some(0),
            source_first_sequence: events.first().map(|event| event.sequence),
            source_last_sequence: events.last().map(|event| event.sequence),
            source_event_count,
            latest_sequence_exclusive,
            next_after_event_sequence: events
                .last()
                .map(|event| Some(event.sequence))
                .unwrap_or(after_event_sequence),
            state: run.state,
            observed_at_ms,
            retention_gap: false,
            has_more: false,
            terminal_failure: None,
            events,
        };
        page.validate_for(&self.provider.profile().profile()?)?;
        Ok(page)
    }

    fn cleanup(&mut self, identity: &AgentProviderRunIdentityV1) -> Result<bool, String> {
        if let Some(run) = self.runs.get(&identity.run_id) {
            if run.identity != *identity || !run.state.is_terminal() {
                return Err("reference cleanup requires the exact terminal run".into());
            }
        }
        let removed = self.runs.remove(&identity.run_id).is_some();
        self.cleaned_runs.insert(identity.run_id.clone());
        Ok(removed)
    }

    fn pause_for_approval(
        &mut self,
        identity: &AgentProviderRunIdentityV1,
        occurred_at_ms: u64,
    ) -> Result<(HarnessToolBindingV1, AgentProviderToolPayloadIdentityV1), String> {
        let run = self
            .runs
            .get_mut(&identity.run_id)
            .ok_or_else(|| "reference approval run does not exist".to_owned())?;
        if run.identity != *identity || run.state != AgentProviderRunStateV1::Executing {
            return Err("reference approval requires the exact executing run".into());
        }
        let tool = HarnessToolBindingV1 {
            name: "workspace.publish".into(),
            revision: "1.0.0".into(),
            contract_digest: format!("sha256:{}", "b".repeat(64)),
            approval_required: true,
        };
        let request = AgentProviderToolPayloadIdentityV1 {
            digest: format!("sha256:{}", "c".repeat(64)),
            size_bytes: 42,
            media_type: "application/json".into(),
        };
        run.events.push(AgentProviderEventRecordV1 {
            sequence: u64::try_from(run.events.len())
                .map_err(|_| "reference event sequence overflowed".to_owned())?,
            occurred_at_ms,
            event: AgentProviderSemanticEventV1::ToolRequest {
                call_id: "call-approval-1".into(),
                tool: tool.clone(),
                request: request.clone(),
            },
        });
        run.state = AgentProviderRunStateV1::AwaitingApproval;
        Ok((tool, request))
    }
}

fn identity(provider: &dyn AgentExecutionProvider, run_id: &str) -> AgentProviderRunIdentityV1 {
    AgentProviderRunIdentityV1::new(
        provider.profile().profile_digest().into(),
        provider.profile().capability_digest().into(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-1".into(),
        run_id.into(),
    )
    .expect("provider identity")
}

#[test]
fn code_and_reference_providers_share_one_command_and_negotiation_contract() {
    let code = NativeCodeAgentExecutionProvider::new().expect("Code provider");
    let reference = ReferenceEchoAgentExecutionProvider::new().expect("reference provider");
    let requirements = AgentProviderCapabilityRequirementsV1::new(vec![
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::EventPages,
    ])
    .expect("requirements");
    for provider in [
        &code as &dyn AgentExecutionProvider,
        &reference as &dyn AgentExecutionProvider,
    ] {
        provider
            .negotiate(&requirements)
            .expect("baseline negotiation")
            .validate_for(
                &provider.profile().profile().expect("profile"),
                &requirements,
            )
            .expect("negotiation evidence");
        provider
            .start_command(
                "start-1".into(),
                identity(provider, "run-1"),
                "bounded input".into(),
            )
            .expect("common start command");
    }
}

#[test]
fn reference_harness_replays_across_process_death_cancels_and_cleans_up() {
    let mut harness = ReferenceEchoHarness::new();
    let identity = identity(&harness.provider, "run-replay");
    let start = harness
        .provider
        .start_command(
            "start-replay".into(),
            identity.clone(),
            "do not echo secret-value-123".into(),
        )
        .expect("start command");
    let accepted = harness.accept(&start, 1).expect("accepted start");
    let mut restarted = harness.restart();
    let replayed = restarted.accept(&start, 1).expect("replayed start");
    assert!(!accepted.replayed);
    assert!(replayed.replayed);
    assert_eq!(accepted.command_digest, replayed.command_digest);

    let page = restarted.page(&identity, None, 2).expect("event page");
    let encoded_page = serde_json::to_string(&page).expect("encoded page");
    let encoded_receipt = serde_json::to_string(&replayed).expect("encoded receipt");
    assert!(!encoded_page.contains("secret-value-123"));
    assert!(!encoded_receipt.contains("secret-value-123"));

    let cancel = restarted
        .provider
        .cancel_command(
            "cancel-replay".into(),
            identity.clone(),
            "test cancellation".into(),
        )
        .expect("cancel command");
    let cancelled = restarted.accept(&cancel, 3).expect("accepted cancellation");
    assert_eq!(cancelled.state, AgentProviderRunStateV1::Cancelled);
    assert!(restarted.cleanup(&identity).expect("cleanup"));
    assert!(!restarted.cleanup(&identity).expect("idempotent cleanup"));
    assert!(
        restarted.accept(&start, 4).is_ok(),
        "exact receipt replay remains valid"
    );
}

#[test]
fn reference_harness_pauses_and_exactly_replays_one_approval_resume() {
    let mut harness = ReferenceEchoHarness::new();
    let identity = identity(&harness.provider, "run-approval");
    let start = harness
        .provider
        .start_command(
            "start-approval".into(),
            identity.clone(),
            "publish the release".into(),
        )
        .expect("start command");
    harness.accept(&start, 1).expect("accepted start");
    let (tool, request) = harness
        .pause_for_approval(&identity, 2)
        .expect("approval request");
    let paused = harness.page(&identity, Some(0), 3).expect("paused page");
    assert_eq!(paused.state, AgentProviderRunStateV1::AwaitingApproval);
    assert_eq!(paused.events.len(), 1);

    let decision = AgentProviderApprovalDecisionV1::new(
        "decision-approval-1".into(),
        "checkpoint-approval-1".into(),
        &identity,
        "call-approval-1".into(),
        tool,
        request.digest,
        AgentProviderApprovalOutcomeV1::Approved,
        4,
    )
    .expect("approval decision");
    let resume = harness
        .provider
        .resume_command("resume-approval-1".into(), identity.clone(), decision)
        .expect("resume command");
    let accepted = harness.accept(&resume, 5).expect("accepted resume");
    assert_eq!(accepted.state, AgentProviderRunStateV1::Executing);
    let mut restarted = harness.restart();
    let replayed = restarted.accept(&resume, 5).expect("replayed resume");
    assert!(replayed.replayed);
    assert_eq!(accepted.command_digest, replayed.command_digest);

    let mut changed = resume;
    let AgentProviderCommandV1::Resume { request } = &mut changed else {
        unreachable!("resume command")
    };
    request.decision.outcome = AgentProviderApprovalOutcomeV1::Denied;
    assert!(restarted.accept(&changed, 6).is_err());
}

#[test]
fn reference_harness_fails_closed_for_unsupported_malformed_gap_and_mixed_version_inputs() {
    let provider = ReferenceEchoAgentExecutionProvider::new().expect("reference provider");
    let requirements =
        AgentProviderCapabilityRequirementsV1::new(vec![AgentProviderCapabilityV1::Checkpoints])
            .expect("checkpoint requirement");
    assert!(provider.negotiate(&requirements).is_err());
    assert!(provider
        .recover_command(
            "recover-1".into(),
            identity(&provider, "run-2"),
            "run-1".into(),
        )
        .is_err());

    let identity = identity(&provider, "run-gap");
    let malformed = AgentProviderEventPageV1 {
        schema: "a3s.cloud.agent-provider-event-page.v2".into(),
        identity,
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(1),
        source_last_sequence: Some(1),
        source_event_count: 1,
        latest_sequence_exclusive: 2,
        next_after_event_sequence: Some(1),
        state: AgentProviderRunStateV1::Executing,
        observed_at_ms: 2,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 1,
            occurred_at_ms: 1,
            event: AgentProviderSemanticEventV1::ModelOutput { text: "gap".into() },
        }],
    };
    assert!(malformed.validate().is_err());
    let mut gap = malformed;
    gap.schema = AgentProviderEventPageV1::SCHEMA.into();
    assert!(gap.validate().is_err());
}
