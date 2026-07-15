use super::*;

pub(super) fn capabilities(build: &str) -> NodeCapabilities {
    NodeCapabilities::new(
        "docker",
        build,
        json!({
            "schema": "a3s.runtime.capabilities.v2",
            "provider_id": "docker",
            "provider_build": build
        }),
    )
    .expect("capabilities")
}

pub(super) fn runtime_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: "docker".into(),
        provider_build: "observation-test".into(),
        unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::None, NetworkMode::Service],
        mount_kinds: Vec::new(),
        health_check_kinds: Vec::new(),
        resource_controls: vec![ResourceControl::Cpu, ResourceControl::Memory],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
        ],
    }
}

pub(super) fn runtime_observation(
    unit_id: &str,
    generation: u64,
    observed_at_ms: u64,
) -> RuntimeObservation {
    RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: unit_id.into(),
        generation,
        spec_digest: format!("sha256:{}", "7".repeat(64)),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Accepted,
        provider_resource_id: None,
        provider_build: None,
        observed_at_ms,
        started_at_ms: None,
        finished_at_ms: None,
        health: None,
        outputs: Vec::new(),
        usage: None,
        evidence: None,
        provider_attestation: None,
        failure: None,
    }
}

pub(super) fn certificate(
    node_id: NodeId,
    suffix: char,
    issued_at: chrono::DateTime<Utc>,
) -> NodeCertificate {
    NodeCertificate::new(
        NodeCertificateId::new(),
        node_id,
        NodeCertificateMaterial {
            serial_number: format!("serial-{suffix}"),
            fingerprint: format!("sha256:{}", suffix.to_string().repeat(64)),
            certificate_pem: "certificate".into(),
            ca_bundle_pem: "CA".into(),
            issued_at,
            expires_at: issued_at + Duration::hours(1),
        },
    )
    .expect("certificate")
}

pub(super) fn event(
    organization_id: OrganizationId,
    aggregate_id: Uuid,
    aggregate_version: u64,
    event_key: &str,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id,
        aggregate_version,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: json!({}),
    }
}

pub(super) async fn command_node(
    repository: &InMemoryNodeRepository,
    now: chrono::DateTime<Utc>,
) -> (NodeId, Uuid) {
    let organization_id = OrganizationId::new();
    let credential = EnrollmentTokenCredential::from_secret(&format!("a3sn_{}", "9".repeat(64)))
        .expect("credential");
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "command worker",
        credential.clone(),
        now,
        now + Duration::minutes(10),
    )
    .expect("token");
    repository
        .issue_enrollment_token(
            token.clone(),
            event(
                organization_id,
                token.id.as_uuid(),
                token.aggregate_version,
                "fleet.enrollment-token.issued",
            ),
            IdempotencyRequest::new("fleet/tokens", "command-worker", b"command worker")
                .expect("idempotency"),
        )
        .await
        .expect("issue command token");
    let agent_instance_id = Uuid::now_v7();
    let reservation = repository
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new("command-worker").expect("node name"),
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: capabilities("command-build"),
                request_digest: format!("sha256:{}", "8".repeat(64)),
                requested_at: now,
            },
        )
        .await
        .expect("reserve command node");
    (reservation.node.id, agent_instance_id)
}

pub(super) fn inspect_draft(
    command_id: NodeCommandId,
    node_id: NodeId,
    aggregate_id: Uuid,
    unit_id: &str,
    generation: u64,
    now: chrono::DateTime<Utc>,
) -> NodeCommandDraft {
    NodeCommandDraft {
        proposed_command_id: command_id,
        node_id,
        aggregate_id,
        payload: NodeCommandPayload::RuntimeInspect {
            unit_id: unit_id.into(),
            generation,
        },
        issued_at: now,
        not_after: now + Duration::minutes(2),
        correlation_id: Uuid::now_v7(),
    }
}

pub(super) fn inspected_ack(
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    completed_at: chrono::DateTime<Utc>,
) -> NodeCommandAck {
    let NodeCommandPayload::RuntimeInspect {
        unit_id,
        generation,
    } = &command.payload
    else {
        panic!("test command must inspect Runtime state");
    };
    NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeInspected {
                inspection: a3s_runtime::contract::RuntimeInspection::NotFound {
                    unit_id: unit_id.clone(),
                    last_generation: Some(*generation),
                },
            }),
        },
    }
}
