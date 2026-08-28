use super::*;
use a3s_cloud_contracts::{
    AgentProviderApprovalDecisionV1, AgentProviderApprovalOutcomeV1, AgentProviderCapabilityV1,
    AgentProviderCommandReceiptV1, AgentProviderRunCancelV1, AgentProviderRunResumeV1,
    HarnessAgentReleaseBindingV1, HarnessInvocationProfileV1, HarnessProviderBindingV1,
    HarnessToolBindingV1, HarnessWorkspaceBindingV1,
};

const APPROVAL_PROMPT: &str = "Request one governed Tool approval.";

pub(super) struct ApprovalMatrix {
    pub(super) pending_restart_binding: NodeAgentProviderRuntimeBindingV1,
    pub(super) next_sequence: u64,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn exercise_approval_matrix(
    executor: &CommandExecutor,
    provider_harness: &HttpAgentProviderHarnessTransport,
    runtime: &dyn RuntimeClient,
    profile: &AgentProviderProfile,
    base_binding: &NodeAgentProviderRuntimeBindingV1,
    node_id: Uuid,
    execution_id: Uuid,
    first_sequence: u64,
) -> GateResult<ApprovalMatrix> {
    let invocation = approval_invocation(profile, base_binding)?;
    let expected_tool = invocation
        .tools
        .first()
        .cloned()
        .ok_or_else(|| invalid("approval invocation omitted its governed Tool"))?;
    let mut sequence = first_sequence;

    for outcome in [
        AgentProviderApprovalOutcomeV1::Approved,
        AgentProviderApprovalOutcomeV1::Denied,
        AgentProviderApprovalOutcomeV1::Expired,
    ] {
        let (binding, start) =
            governed_start(profile, base_binding, &invocation, outcome.as_str())?;
        let (_, started) = dispatch_provider_command(
            executor,
            profile,
            node_id,
            execution_id,
            &mut sequence,
            &binding,
            &start,
        )
        .await?;
        let started_receipt = provider_receipt(&started)?;
        if started_receipt.state != AgentProviderRunStateV1::AwaitingApproval
            || started_receipt.replayed
        {
            return Err(invalid(format!(
                "reference provider did not pause the {} approval run",
                outcome.as_str()
            ))
            .into());
        }

        let endpoint = agent_provider_harness::resolve_runtime_endpoint(runtime, &binding).await?;
        let page_request = AgentProviderEventPageRequestV1 {
            schema: AgentProviderEventPageRequestV1::SCHEMA.into(),
            identity: binding.provider_run_identity.clone(),
            after_event_sequence: None,
            limit: 64,
        };
        let page = provider_harness
            .event_page(&endpoint, &binding, &page_request, Duration::from_secs(5))
            .await?;
        let (call_id, tool, request_digest) = approval_request(&page, &expected_tool)?;
        let decision = AgentProviderApprovalDecisionV1::new(
            format!("reference-{}-decision-{}", outcome.as_str(), Uuid::now_v7()),
            format!(
                "reference-{}-checkpoint-{}",
                outcome.as_str(),
                Uuid::now_v7()
            ),
            &binding.provider_run_identity,
            call_id,
            tool,
            request_digest,
            outcome,
            timestamp_ms()?,
        )?;
        let resume = AgentProviderCommandV1::Resume {
            request: AgentProviderRunResumeV1::new(
                format!("reference-{}-resume", outcome.as_str()),
                binding.provider_run_identity.clone(),
                decision,
            )?,
        };
        let (resume_envelope, resumed) = dispatch_provider_command(
            executor,
            profile,
            node_id,
            execution_id,
            &mut sequence,
            &binding,
            &resume,
        )
        .await?;
        let resumed_receipt = provider_receipt(&resumed)?;
        if resumed_receipt.state != AgentProviderRunStateV1::Executing || resumed_receipt.replayed {
            return Err(invalid(format!(
                "reference provider did not resume the {} approval run",
                outcome.as_str()
            ))
            .into());
        }

        let settled = provider_harness
            .event_page(
                &endpoint,
                &binding,
                &AgentProviderEventPageRequestV1 {
                    after_event_sequence: Some(0),
                    ..page_request
                },
                Duration::from_secs(5),
            )
            .await?;
        if settled.state != AgentProviderRunStateV1::Executing
            || !settled.events.is_empty()
            || settled.next_after_event_sequence != Some(0)
        {
            return Err(invalid(format!(
                "reference provider did not settle the {} approval cursor exactly",
                outcome.as_str()
            ))
            .into());
        }

        if outcome == AgentProviderApprovalOutcomeV1::Approved {
            let mut fleet_redelivery = resume_envelope;
            fleet_redelivery.lease_id = Uuid::now_v7();
            let replayed_by_fleet = executor.execute(fleet_redelivery).await?;
            if replayed_by_fleet.outcome != resumed.outcome {
                return Err(invalid("Fleet journal changed the replayed approval Resume").into());
            }
            let (_, replayed_by_provider) = dispatch_provider_command(
                executor,
                profile,
                node_id,
                execution_id,
                &mut sequence,
                &binding,
                &resume,
            )
            .await?;
            let provider_replay_receipt = provider_receipt(&replayed_by_provider)?;
            if !provider_replay_receipt.replayed
                || provider_replay_receipt.command_digest != resumed_receipt.command_digest
                || provider_replay_receipt.state != resumed_receipt.state
            {
                return Err(invalid("provider changed the replayed approval Resume").into());
            }
        }
    }

    let (cancel_binding, cancel_start) =
        governed_start(profile, base_binding, &invocation, "cancelled")?;
    let (_, cancel_started) = dispatch_provider_command(
        executor,
        profile,
        node_id,
        execution_id,
        &mut sequence,
        &cancel_binding,
        &cancel_start,
    )
    .await?;
    if provider_receipt(&cancel_started)?.state != AgentProviderRunStateV1::AwaitingApproval {
        return Err(invalid("reference provider did not pause the cancellation run").into());
    }
    let cancel = AgentProviderCommandV1::Cancel {
        request: AgentProviderRunCancelV1::new(
            "reference-cancel-pending-approval".into(),
            cancel_binding.provider_run_identity.clone(),
            "Cloud execution cancelled while awaiting approval".into(),
        )?,
    };
    let (_, cancelled) = dispatch_provider_command(
        executor,
        profile,
        node_id,
        execution_id,
        &mut sequence,
        &cancel_binding,
        &cancel,
    )
    .await?;
    if provider_receipt(&cancelled)?.state != AgentProviderRunStateV1::Cancelled {
        return Err(invalid("reference provider did not cancel its pending approval").into());
    }
    let cancel_endpoint =
        agent_provider_harness::resolve_runtime_endpoint(runtime, &cancel_binding).await?;
    let cancelled_page = provider_harness
        .event_page(
            &cancel_endpoint,
            &cancel_binding,
            &AgentProviderEventPageRequestV1 {
                schema: AgentProviderEventPageRequestV1::SCHEMA.into(),
                identity: cancel_binding.provider_run_identity.clone(),
                after_event_sequence: Some(0),
                limit: 64,
            },
            Duration::from_secs(5),
        )
        .await?;
    if cancelled_page.state != AgentProviderRunStateV1::Cancelled
        || !cancelled_page.events.is_empty()
    {
        return Err(
            invalid("reference provider exposed progress after approval cancellation").into(),
        );
    }

    let (pending_restart_binding, pending_start) =
        governed_start(profile, base_binding, &invocation, "pending-restart")?;
    let (_, pending_started) = dispatch_provider_command(
        executor,
        profile,
        node_id,
        execution_id,
        &mut sequence,
        &pending_restart_binding,
        &pending_start,
    )
    .await?;
    if provider_receipt(&pending_started)?.state != AgentProviderRunStateV1::AwaitingApproval {
        return Err(invalid("reference provider did not retain a pending restart approval").into());
    }

    Ok(ApprovalMatrix {
        pending_restart_binding,
        next_sequence: sequence,
    })
}

fn approval_invocation(
    profile: &AgentProviderProfile,
    binding: &NodeAgentProviderRuntimeBindingV1,
) -> GateResult<HarnessInvocationProfileV1> {
    let invocation = HarnessInvocationProfileV1 {
        schema: HarnessInvocationProfileV1::SCHEMA.into(),
        agent: HarnessAgentReleaseBindingV1 {
            organization_id: Uuid::from_u128(1),
            asset_id: Uuid::from_u128(2),
            asset_release_id: Uuid::from_u128(3),
            build_run_id: Uuid::from_u128(4),
            artifact_digest: binding.provider_run_identity.agent_release_identity.clone(),
        },
        provider: HarnessProviderBindingV1 {
            kind: profile.kind().into(),
            revision: profile.revision().into(),
            profile_digest: profile.digest().into(),
            capability_digest: profile.capability_digest().into(),
        },
        instructions_digest: format!("sha256:{}", "a".repeat(64)),
        environment_policy_digest: format!("sha256:{}", "b".repeat(64)),
        security_policy_digest: format!("sha256:{}", "c".repeat(64)),
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: binding.workload_id,
            workload_revision_id: binding.workload_revision_id,
            runtime_unit_id: binding.runtime_unit_id.clone(),
            runtime_generation: binding.runtime_generation,
            runtime_spec_digest: binding.runtime_spec_digest.clone(),
            working_directory: Some("/".into()),
        },
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        models: Vec::new(),
        secrets: Vec::new(),
        tools: vec![HarnessToolBindingV1 {
            name: "workspace.publish".into(),
            revision: "1.0.0".into(),
            contract_digest: format!("sha256:{}", "e".repeat(64)),
            approval_required: true,
        }],
        required_capabilities: vec![
            AgentProviderCapabilityV1::Cancellation,
            AgentProviderCapabilityV1::Cleanup,
            AgentProviderCapabilityV1::EventPages,
            AgentProviderCapabilityV1::PauseResume,
            AgentProviderCapabilityV1::ToolCalls,
        ],
    };
    invocation.validate_for(profile).map_err(invalid)?;
    Ok(invocation)
}

fn governed_start(
    profile: &AgentProviderProfile,
    base_binding: &NodeAgentProviderRuntimeBindingV1,
    invocation: &HarnessInvocationProfileV1,
    label: &str,
) -> GateResult<(NodeAgentProviderRuntimeBindingV1, AgentProviderCommandV1)> {
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        invocation.agent.artifact_digest.clone(),
        format!("reference-box-approval-{label}"),
        format!("reference-box-approval-{label}-{}", Uuid::now_v7()),
    )?;
    let request = AgentProviderRunStartV1::new_with_invocation_profile(
        format!("reference-{label}-start"),
        identity,
        invocation.clone(),
        APPROVAL_PROMPT.into(),
    )?;
    let mut binding = base_binding.clone();
    binding.provider_run_identity = request.identity.clone();
    Ok((binding, AgentProviderCommandV1::Start { request }))
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_provider_command(
    executor: &CommandExecutor,
    profile: &AgentProviderProfile,
    node_id: Uuid,
    execution_id: Uuid,
    sequence: &mut u64,
    binding: &NodeAgentProviderRuntimeBindingV1,
    provider_command: &AgentProviderCommandV1,
) -> GateResult<(NodeCommandEnvelope, NodeCommandAck)> {
    let envelope = command(
        node_id,
        execution_id,
        *sequence,
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(binding.clone()),
            command: Box::new(provider_command.clone()),
        },
    )?;
    *sequence = sequence
        .checked_add(1)
        .ok_or_else(|| invalid("provider command sequence overflowed"))?;
    let acknowledgement = executor.execute(envelope.clone()).await?;
    let receipt = provider_receipt(&acknowledgement)?;
    receipt
        .validate_for(profile, provider_command)
        .map_err(invalid)?;
    Ok((envelope, acknowledgement))
}

fn provider_receipt(
    acknowledgement: &NodeCommandAck,
) -> GateResult<&AgentProviderCommandReceiptV1> {
    let NodeCommandResult::AgentProviderCommandAccepted { receipt } =
        succeeded_result(acknowledgement)?
    else {
        return Err(invalid("reference approval command returned another result kind").into());
    };
    Ok(receipt)
}

fn approval_request(
    page: &a3s_cloud_contracts::AgentProviderEventPageV1,
    expected_tool: &HarnessToolBindingV1,
) -> GateResult<(String, HarnessToolBindingV1, String)> {
    if page.state != AgentProviderRunStateV1::AwaitingApproval
        || page.events.len() != 1
        || page.next_after_event_sequence != Some(0)
    {
        return Err(invalid("reference provider omitted its exact approval page").into());
    }
    let AgentProviderSemanticEventV1::ToolRequest {
        call_id,
        tool,
        request,
    } = &page.events[0].event
    else {
        return Err(invalid("reference provider emitted another approval event kind").into());
    };
    if tool != expected_tool
        || request.size_bytes != 128
        || request.media_type != "application/json"
    {
        return Err(
            invalid("reference provider changed its governed Tool request identity").into(),
        );
    }
    Ok((call_id.clone(), tool.clone(), request.digest.clone()))
}

fn timestamp_ms() -> GateResult<u64> {
    Ok(u64::try_from(Utc::now().timestamp_millis())?)
}
