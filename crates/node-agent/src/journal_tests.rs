use super::*;
use a3s_cloud_contracts::{
    AgentProtocolCommandReceiptV1, AgentProtocolCommandV1, AgentProtocolRunCancelV1,
    AgentProtocolRunIdentityV1, AgentProtocolRunRecoverV1, AgentProtocolRunStartV1,
    AgentProtocolRunStateV1, AgentProviderCommandReceiptV1, AgentProviderCommandV1,
    AgentProviderProfile, AgentProviderRunIdentityV1, AgentProviderRunStartV1,
    AgentProviderRunStateV1, CloudSecretReference, NodeAgentProviderRuntimeBindingV1,
    NodeCodeAgentRuntimeBindingV1, NodeCommandMetadata, NodeCommandResult, AGENT_PROTOCOL_V1,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeActionRequest,
    RuntimeApplyRequest, RuntimeInspection, RuntimeNetworkSpec, RuntimeObservation,
    RuntimeProcessSpec, RuntimeRemoval, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
    SecretReference, SecretTarget,
};
use chrono::Duration;
use std::collections::BTreeMap;

fn envelope(
    node_id: Uuid,
    command_id: Uuid,
    lease_id: Uuid,
    sequence: u64,
    aggregate_id: Uuid,
    generation: u64,
) -> NodeCommandEnvelope {
    let issued_at = Utc::now();
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id,
            lease_id,
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::RuntimeInspect {
            unit_id: "service-1".into(),
            generation,
        },
    )
    .expect("command envelope")
}

fn outcome() -> NodeCommandOutcome {
    NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::RuntimeInspected {
            inspection: RuntimeInspection::NotFound {
                schema: RuntimeInspection::SCHEMA.into(),
                unit_id: "service-1".into(),
                last_generation: Some(1),
            },
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_envelope(
    node_id: Uuid,
    command_id: Uuid,
    sequence: u64,
    aggregate_id: Uuid,
    generation: u64,
    request_id: &str,
    artifact: char,
) -> NodeCommandEnvelope {
    let issued_at = Utc::now();
    let digest = format!("sha256:{}", artifact.to_string().repeat(64));
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: "service-1".into(),
        generation,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: format!("oci://registry.example/app@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: RuntimeProcessSpec {
            command: Vec::new(),
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Container,
        health: None,
        service_lifecycle: None,
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
        identity_attachment_digest: None,
    };
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id,
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: request_id.into(),
                deadline_at_ms: None,
                spec,
            }),
            resource_claim: None,
        },
    )
    .expect("Runtime apply envelope")
}

fn applied_outcome(envelope: &NodeCommandEnvelope) -> NodeCommandOutcome {
    let NodeCommandPayload::RuntimeApply { request, .. } = &envelope.payload else {
        panic!("apply payload");
    };
    NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::RuntimeApplied {
            observation: Box::new(RuntimeObservation {
                schema: RuntimeObservation::SCHEMA.into(),
                unit_id: request.spec.unit_id.clone(),
                generation: request.spec.generation,
                spec_digest: request.spec.digest().expect("spec digest"),
                class: request.spec.class,
                state: RuntimeUnitState::Accepted,
                provider_resource_id: None,
                provider_build: None,
                observed_at_ms: 1,
                started_at_ms: None,
                finished_at_ms: None,
                health: None,
                liveness: None,
                outputs: Vec::new(),
                usage: None,
                evidence: None,
                provider_attestation: None,
                failure: None,
            }),
        }),
    }
}

fn remove_envelope(
    node_id: Uuid,
    command_id: Uuid,
    sequence: u64,
    aggregate_id: Uuid,
) -> NodeCommandEnvelope {
    let issued_at = Utc::now();
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id,
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::RuntimeRemove {
            request: RuntimeActionRequest {
                schema: RuntimeActionRequest::SCHEMA.into(),
                request_id: "remove-service-1".into(),
                unit_id: "service-1".into(),
                generation: 1,
                deadline_at_ms: None,
            },
        },
    )
    .expect("Runtime remove envelope")
}

fn code_binding(execution_id: Uuid, run_id: &str) -> NodeCodeAgentRuntimeBindingV1 {
    NodeCodeAgentRuntimeBindingV1 {
        schema: NodeCodeAgentRuntimeBindingV1::SCHEMA.into(),
        execution_id,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: "service-1".into(),
        runtime_generation: 1,
        runtime_spec_digest: format!("sha256:{}", "b".repeat(64)),
        service_port_name: "agent".into(),
        code_run_identity: AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: AGENT_PROTOCOL_V1.into(),
            agent_release_identity: format!("sha256:{}", "a".repeat(64)),
            session_id: "conversation-1".into(),
            run_id: run_id.into(),
        },
    }
}

fn code_envelope(
    node_id: Uuid,
    sequence: u64,
    binding: NodeCodeAgentRuntimeBindingV1,
    command: AgentProtocolCommandV1,
) -> NodeCommandEnvelope {
    let issued_at = Utc::now();
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id: binding.execution_id,
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::CodeAgentCommand {
            binding: Box::new(binding),
            command: Box::new(command),
        },
    )
    .expect("Code command envelope")
}

fn code_outcome(command: &AgentProtocolCommandV1, observed_at_ms: u64) -> NodeCommandOutcome {
    NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::CodeAgentCommandAccepted {
            receipt: Box::new(AgentProtocolCommandReceiptV1 {
                schema: AgentProtocolCommandReceiptV1::SCHEMA.into(),
                action: command.action(),
                request_id: command.request_id().into(),
                identity: command.identity().clone(),
                command_digest: command.digest().expect("Code command digest"),
                state: if matches!(command, AgentProtocolCommandV1::Cancel { .. }) {
                    AgentProtocolRunStateV1::Cancelled
                } else {
                    AgentProtocolRunStateV1::Created
                },
                latest_event_sequence_exclusive: 0,
                observed_at_ms,
                replayed: false,
            }),
        }),
    }
}

fn provider_binding(execution_id: Uuid) -> NodeAgentProviderRuntimeBindingV1 {
    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/reference-echo-provider-profile.acl"
    )))
    .expect("reference provider profile");
    NodeAgentProviderRuntimeBindingV1 {
        schema: NodeAgentProviderRuntimeBindingV1::SCHEMA.into(),
        execution_id,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: "service-1".into(),
        runtime_generation: 1,
        runtime_spec_digest: format!("sha256:{}", "b".repeat(64)),
        service_port_name: "agent".into(),
        provider_profile_acl: profile.canonical_acl().into(),
        provider_profile_digest: profile.digest().into(),
        provider_run_identity: AgentProviderRunIdentityV1::new(
            profile.digest().into(),
            profile.capability_digest().into(),
            format!("sha256:{}", "a".repeat(64)),
            "conversation-reference".into(),
            "execution-reference-attempt-1".into(),
        )
        .expect("provider identity"),
    }
}

fn provider_envelope(
    node_id: Uuid,
    sequence: u64,
    binding: NodeAgentProviderRuntimeBindingV1,
    command: AgentProviderCommandV1,
) -> NodeCommandEnvelope {
    let issued_at = Utc::now();
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id: binding.execution_id,
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(binding),
            command: Box::new(command),
        },
    )
    .expect("provider command envelope")
}

fn provider_outcome(
    binding: &NodeAgentProviderRuntimeBindingV1,
    command: &AgentProviderCommandV1,
    observed_at_ms: u64,
) -> NodeCommandOutcome {
    NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::AgentProviderCommandAccepted {
            receipt: Box::new(
                AgentProviderCommandReceiptV1::accepted(
                    &binding.profile().expect("provider profile"),
                    command,
                    AgentProviderRunStateV1::Created,
                    observed_at_ms,
                    false,
                )
                .expect("provider receipt"),
            ),
        }),
    }
}

#[tokio::test]
async fn completed_command_rebinds_a_new_lease_without_reexecuting() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let command_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
    let first = envelope(node_id, command_id, Uuid::now_v7(), 1, aggregate_id, 1);
    assert_eq!(
        journal.begin(first.clone()).await.expect("begin command"),
        JournalDecision::Execute
    );
    let completed_at = Utc::now();
    let first_ack = journal
        .complete(command_id, completed_at, outcome())
        .await
        .expect("complete command");
    let mut redelivered = first;
    redelivered.lease_id = Uuid::now_v7();
    let replay = journal
        .begin(redelivered.clone())
        .await
        .expect("redeliver command");
    let replay_ack = match replay {
        JournalDecision::Replay(value) => value,
        JournalDecision::Execute => panic!("completed command must not execute again"),
    };
    assert_ne!(first_ack.lease_id, replay_ack.lease_id);
    assert_eq!(replay_ack.lease_id, redelivered.lease_id);
    assert_eq!(replay_ack.outcome, first_ack.outcome);
    assert_eq!(replay_ack.completed_at, first_ack.completed_at);
    assert_eq!(journal.after_sequence().await.expect("after sequence"), 0);
    let receipt = NodeCommandAckReceipt {
        schema: NodeCommandAckReceipt::SCHEMA.into(),
        command_id,
        node_id,
        replayed: false,
    };
    assert_eq!(
        journal
            .mark_acknowledged(receipt)
            .await
            .expect("mark acknowledged"),
        1
    );
    assert!(journal
        .pending_acknowledgements()
        .await
        .expect("pending acknowledgements")
        .is_empty());
}

#[tokio::test]
async fn journal_rejects_sequence_gaps_and_command_content_conflicts() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let command_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
    assert!(journal
        .begin(envelope(
            node_id,
            command_id,
            Uuid::now_v7(),
            2,
            aggregate_id,
            1,
        ))
        .await
        .is_err());
    let first = envelope(node_id, command_id, Uuid::now_v7(), 1, aggregate_id, 1);
    journal.begin(first.clone()).await.expect("first command");
    let mut conflict = first;
    conflict.payload = NodeCommandPayload::RuntimeInspect {
        unit_id: "different-service".into(),
        generation: 1,
    };
    conflict.payload_digest = conflict.payload.digest().expect("payload digest");
    assert!(journal.begin(conflict).await.is_err());
}

#[tokio::test]
async fn same_generation_recovery_apply_allows_a_new_request_for_the_same_spec() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");

    journal
        .begin(apply_envelope(
            node_id,
            Uuid::now_v7(),
            1,
            aggregate_id,
            1,
            "deployment-apply",
            'a',
        ))
        .await
        .expect("initial apply");
    assert_eq!(
        journal
            .begin(apply_envelope(
                node_id,
                Uuid::now_v7(),
                2,
                aggregate_id,
                1,
                "recovery-apply",
                'a',
            ))
            .await
            .expect("same-spec recovery apply"),
        JournalDecision::Execute
    );
    assert!(journal
        .begin(apply_envelope(
            node_id,
            Uuid::now_v7(),
            3,
            aggregate_id,
            1,
            "conflicting-apply",
            'b',
        ))
        .await
        .is_err());
}

#[tokio::test]
async fn older_generation_retirement_does_not_regress_desired_state() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");

    journal
        .begin(apply_envelope(
            node_id,
            Uuid::now_v7(),
            1,
            aggregate_id,
            3,
            "generation-three-apply",
            'c',
        ))
        .await
        .expect("generation three apply");
    let mut retirement = remove_envelope(node_id, Uuid::now_v7(), 2, aggregate_id);
    let NodeCommandPayload::RuntimeRemove { request } = &retirement.payload else {
        panic!("retirement fixture payload");
    };
    let mut request = request.clone();
    request.request_id = "retire-generation-one".into();
    retirement.payload = NodeCommandPayload::RuntimeStop { request };
    retirement.payload_schema = retirement.payload.schema().into();
    retirement.payload_digest = retirement.payload.digest().expect("retirement digest");
    retirement.validate().expect("retirement command");
    assert_eq!(
        journal
            .begin(retirement)
            .await
            .expect("retire older generation"),
        JournalDecision::Execute
    );
    assert!(journal
        .begin(apply_envelope(
            node_id,
            Uuid::now_v7(),
            3,
            aggregate_id,
            2,
            "regressed-generation-apply",
            'b',
        ))
        .await
        .is_err());
}

#[tokio::test]
async fn command_journal_persists_only_secret_references() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let mut envelope = apply_envelope(
        node_id,
        Uuid::now_v7(),
        1,
        Uuid::now_v7(),
        1,
        "secret-reference-apply",
        'a',
    );
    let reference =
        CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 3).expect("reference");
    let NodeCommandPayload::RuntimeApply { request, .. } = &mut envelope.payload else {
        panic!("apply envelope payload");
    };
    request.spec.secrets.push(SecretReference {
        name: "api-token".into(),
        reference: reference.to_string(),
        target: SecretTarget::Environment {
            variable: "API_TOKEN".into(),
        },
    });
    envelope.payload_digest = envelope.payload.digest().expect("payload digest");
    envelope.validate().expect("Secret reference envelope");

    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
    journal.begin(envelope).await.expect("persist command");
    let persisted = tokio::fs::read_to_string(directory.path().join(JOURNAL_FILE))
        .await
        .expect("journal JSON");
    assert!(persisted.contains(&reference.to_string()));
    assert!(!persisted.contains("materialized-at-runtime"));
}

#[tokio::test]
async fn successful_runtime_outcomes_project_restart_safe_log_targets() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let apply_id = Uuid::now_v7();
    let apply = apply_envelope(
        node_id,
        apply_id,
        1,
        aggregate_id,
        1,
        "log-target-apply",
        'a',
    );
    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
    journal.begin(apply.clone()).await.expect("begin apply");
    journal
        .complete(apply_id, Utc::now(), applied_outcome(&apply))
        .await
        .expect("complete apply");

    let reopened = FileCommandJournal::new(directory.path(), node_id).expect("reopen journal");
    assert_eq!(
        reopened.log_targets().await.expect("project targets"),
        vec![RuntimeLogTarget {
            unit_id: "service-1".into(),
            generation: 1,
        }]
    );

    let remove_id = Uuid::now_v7();
    let remove = remove_envelope(node_id, remove_id, 2, aggregate_id);
    reopened.begin(remove).await.expect("begin remove");
    reopened
        .complete(
            remove_id,
            Utc::now(),
            NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeRemoved {
                    removal: RuntimeRemoval {
                        schema: RuntimeRemoval::SCHEMA.into(),
                        request_id: "remove-service-1".into(),
                        unit_id: "service-1".into(),
                        generation: 1,
                        removed_at_ms: 2,
                        already_absent: false,
                    },
                }),
            },
        )
        .await
        .expect("complete remove");
    assert!(reopened
        .log_targets()
        .await
        .expect("project removed targets")
        .is_empty());
}

#[tokio::test]
async fn successful_code_commands_project_only_the_current_exact_run_binding() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    let first_binding = code_binding(execution_id, "execution-1-attempt-1");
    let start = AgentProtocolCommandV1::Start {
        request: AgentProtocolRunStartV1 {
            schema: AgentProtocolRunStartV1::SCHEMA.into(),
            request_id: "execution-1:start".into(),
            identity: first_binding.code_run_identity.clone(),
            prompt: "Fix the failing test.".into(),
        },
    };
    let start_envelope = code_envelope(node_id, 1, first_binding.clone(), start.clone());
    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
    journal
        .begin(start_envelope.clone())
        .await
        .expect("begin Code start");
    let start_completed_at = Utc::now();
    let start_observed_at_ms = start_completed_at
        .timestamp_millis()
        .try_into()
        .expect("Code start completion time after Unix epoch");
    journal
        .complete(
            start_envelope.command_id,
            start_completed_at,
            code_outcome(&start, start_observed_at_ms),
        )
        .await
        .expect("complete Code start");

    let reopened = FileCommandJournal::new(directory.path(), node_id).expect("reopen journal");
    assert_eq!(
        reopened
            .code_run_bindings()
            .await
            .expect("project initial Code binding"),
        vec![first_binding.clone()]
    );

    let mut recovery_binding = first_binding.clone();
    recovery_binding.code_run_identity.run_id = "execution-1-attempt-2".into();
    let recover = AgentProtocolCommandV1::Recover {
        request: AgentProtocolRunRecoverV1 {
            schema: AgentProtocolRunRecoverV1::SCHEMA.into(),
            request_id: "execution-1:recover:2".into(),
            identity: recovery_binding.code_run_identity.clone(),
            checkpoint_run_id: first_binding.code_run_identity.run_id.clone(),
        },
    };
    let recover_envelope = code_envelope(node_id, 2, recovery_binding.clone(), recover.clone());
    reopened
        .begin(recover_envelope.clone())
        .await
        .expect("begin Code recovery");
    let recovery_completed_at = Utc::now();
    let recovery_observed_at_ms = recovery_completed_at
        .timestamp_millis()
        .try_into()
        .expect("Code recovery completion time after Unix epoch");
    reopened
        .complete(
            recover_envelope.command_id,
            recovery_completed_at,
            code_outcome(&recover, recovery_observed_at_ms),
        )
        .await
        .expect("complete Code recovery");
    assert_eq!(
        reopened
            .code_run_bindings()
            .await
            .expect("project recovered Code binding"),
        vec![recovery_binding.clone()]
    );

    let cancel = AgentProtocolCommandV1::Cancel {
        request: AgentProtocolRunCancelV1 {
            schema: AgentProtocolRunCancelV1::SCHEMA.into(),
            request_id: "execution-1:cancel".into(),
            identity: recovery_binding.code_run_identity.clone(),
            reason: "requested by the Cloud execution owner".into(),
        },
    };
    let cancel_envelope = code_envelope(node_id, 3, recovery_binding.clone(), cancel.clone());
    reopened
        .begin(cancel_envelope.clone())
        .await
        .expect("begin Code cancellation");
    let cancellation_completed_at = Utc::now();
    let cancellation_observed_at_ms = cancellation_completed_at
        .timestamp_millis()
        .try_into()
        .expect("Code cancellation completion time after Unix epoch");
    reopened
        .complete(
            cancel_envelope.command_id,
            cancellation_completed_at,
            code_outcome(&cancel, cancellation_observed_at_ms),
        )
        .await
        .expect("complete Code cancellation");
    assert_eq!(
        reopened
            .code_run_bindings()
            .await
            .expect("project cancelled Code binding"),
        vec![recovery_binding]
    );

    let remove = remove_envelope(node_id, Uuid::now_v7(), 4, execution_id);
    let remove_id = remove.command_id;
    reopened.begin(remove).await.expect("begin Runtime removal");
    reopened
        .complete(
            remove_id,
            Utc::now(),
            NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeRemoved {
                    removal: RuntimeRemoval {
                        schema: RuntimeRemoval::SCHEMA.into(),
                        request_id: "remove-service-1".into(),
                        unit_id: "service-1".into(),
                        generation: 1,
                        removed_at_ms: 2,
                        already_absent: false,
                    },
                }),
            },
        )
        .await
        .expect("complete Runtime removal");
    assert!(reopened
        .code_run_bindings()
        .await
        .expect("project removed Code bindings")
        .is_empty());
}

#[tokio::test]
async fn successful_non_code_executions_share_one_runtime_until_it_is_removed() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    let binding = provider_binding(execution_id);
    let command = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "reference:start".into(),
            binding.provider_run_identity.clone(),
            "Echo this input.".into(),
        )
        .expect("provider start"),
    };
    let start = provider_envelope(node_id, 1, binding.clone(), command.clone());
    let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
    journal
        .begin(start.clone())
        .await
        .expect("begin provider start");
    let completed_at = Utc::now();
    journal
        .complete(
            start.command_id,
            completed_at,
            provider_outcome(
                &binding,
                &command,
                completed_at
                    .timestamp_millis()
                    .try_into()
                    .expect("provider observation time"),
            ),
        )
        .await
        .expect("complete provider start");

    let mut sibling_binding = binding.clone();
    sibling_binding.execution_id = Uuid::now_v7();
    sibling_binding.provider_run_identity.session_id = "conversation-reference-sibling".into();
    sibling_binding.provider_run_identity.run_id = "execution-reference-sibling-attempt-1".into();
    let sibling_command = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "reference:sibling:start".into(),
            sibling_binding.provider_run_identity.clone(),
            "Echo the sibling input.".into(),
        )
        .expect("sibling provider start"),
    };
    let sibling_start =
        provider_envelope(node_id, 2, sibling_binding.clone(), sibling_command.clone());
    journal
        .begin(sibling_start.clone())
        .await
        .expect("begin sibling provider start");
    let sibling_completed_at = Utc::now();
    journal
        .complete(
            sibling_start.command_id,
            sibling_completed_at,
            provider_outcome(
                &sibling_binding,
                &sibling_command,
                sibling_completed_at
                    .timestamp_millis()
                    .try_into()
                    .expect("sibling provider observation time"),
            ),
        )
        .await
        .expect("complete sibling provider start");

    let reopened = FileCommandJournal::new(directory.path(), node_id).expect("reopen journal");
    let mut expected_bindings = vec![binding.clone(), sibling_binding];
    expected_bindings.sort_by_key(|binding| binding.execution_id);
    assert_eq!(
        reopened
            .provider_run_bindings()
            .await
            .expect("project provider binding"),
        expected_bindings
    );
    assert!(reopened
        .code_run_bindings()
        .await
        .expect("project Code bindings")
        .is_empty());

    let remove = remove_envelope(node_id, Uuid::now_v7(), 3, execution_id);
    let remove_id = remove.command_id;
    reopened.begin(remove).await.expect("begin Runtime removal");
    reopened
        .complete(
            remove_id,
            Utc::now(),
            NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeRemoved {
                    removal: RuntimeRemoval {
                        schema: RuntimeRemoval::SCHEMA.into(),
                        request_id: "remove-service-1".into(),
                        unit_id: "service-1".into(),
                        generation: 1,
                        removed_at_ms: 2,
                        already_absent: false,
                    },
                }),
            },
        )
        .await
        .expect("complete Runtime removal");
    assert!(reopened
        .provider_run_bindings()
        .await
        .expect("project removed provider bindings")
        .is_empty());
}
