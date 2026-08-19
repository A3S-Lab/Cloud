use super::*;

#[test]
fn gateway_snapshot_commands_bind_the_complete_snapshot_and_exact_acknowledgement() {
    let metadata = metadata(9);
    let snapshot = gateway_snapshot(
        metadata.node_id,
        4,
        Some(3),
        metadata.issued_at,
        metadata.not_after,
    );
    snapshot.validate().expect("valid Gateway snapshot");

    let command = NodeCommandEnvelope::new(
        metadata,
        NodeCommandPayload::GatewaySnapshotInstall {
            snapshot: Box::new(snapshot.clone()),
        },
    )
    .expect("Gateway install command");
    assert_eq!(command.generation, snapshot.revision);
    assert_eq!(command.payload_schema, GatewaySnapshot::SCHEMA);

    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command.command_id,
        node_id: command.node_id,
        gateway_id: snapshot.gateway_id,
        revision: snapshot.revision,
        snapshot_digest: snapshot.snapshot_digest.clone(),
        expires_at: snapshot.expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at: command.issued_at + Duration::milliseconds(10),
        management_protocol: Some(GatewayManagementProtocol::v1(
            GatewayManagementProtocolDiscovery::Advertised,
        )),
    };
    acknowledgement
        .validate_for(command.command_id, command.node_id, &snapshot)
        .expect("exact Gateway acknowledgement");
    let mut legacy_acknowledgement = acknowledgement.clone();
    legacy_acknowledgement.schema = NodeGatewayAck::LEGACY_SCHEMA.into();
    legacy_acknowledgement.management_protocol = None;
    legacy_acknowledgement
        .validate_for(command.command_id, command.node_id, &snapshot)
        .expect("legacy Gateway acknowledgement remains readable");
    let mut missing_protocol = acknowledgement.clone();
    missing_protocol.management_protocol = None;
    assert!(missing_protocol.validate().is_err());

    let command_acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: acknowledgement.acknowledged_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::GatewaySnapshotInstalled {
                acknowledgement: acknowledgement.clone(),
            }),
        },
    };
    command_acknowledgement
        .validate_against(&command)
        .expect("Gateway command acknowledgement");
    let mut legacy_command_acknowledgement = command_acknowledgement.clone();
    legacy_command_acknowledgement.schema = NodeCommandAck::LEGACY_SCHEMA.into();
    {
        let NodeCommandOutcome::Succeeded { result } = &mut legacy_command_acknowledgement.outcome
        else {
            panic!("expected successful legacy Gateway command acknowledgement");
        };
        let NodeCommandResult::GatewaySnapshotInstalled {
            acknowledgement: legacy_gateway_ack,
        } = result.as_mut()
        else {
            panic!("expected legacy Gateway snapshot result");
        };
        legacy_gateway_ack.schema = NodeGatewayAck::LEGACY_SCHEMA.into();
        legacy_gateway_ack.management_protocol = None;
    }
    legacy_command_acknowledgement
        .validate_against(&command)
        .expect("legacy command and Gateway acknowledgement remain readable");
    {
        let NodeCommandOutcome::Succeeded { result } = &mut legacy_command_acknowledgement.outcome
        else {
            panic!("expected successful legacy Gateway command acknowledgement");
        };
        let NodeCommandResult::GatewaySnapshotInstalled {
            acknowledgement: legacy_gateway_ack,
        } = result.as_mut()
        else {
            panic!("expected legacy Gateway snapshot result");
        };
        legacy_gateway_ack.schema = NodeGatewayAck::SCHEMA.into();
        legacy_gateway_ack.management_protocol = Some(GatewayManagementProtocol::advertised_v1());
    }
    assert!(legacy_command_acknowledgement
        .validate_against(&command)
        .is_err());

    let mut wrong_revision = acknowledgement;
    wrong_revision.revision += 1;
    assert!(wrong_revision
        .validate_for(command.command_id, command.node_id, &snapshot)
        .is_err());

    let mut wrong_digest = snapshot.clone();
    wrong_digest.acl.push_str("# changed\n");
    assert!(wrong_digest.validate().is_err());

    let invalid_compare_and_swap = GatewaySnapshot::new(
        snapshot.gateway_id,
        4,
        Some(4),
        snapshot.issued_at,
        snapshot.expires_at,
        "valid = true\n",
    );
    assert!(invalid_compare_and_swap.is_err());
}

#[test]
fn gateway_snapshot_observation_reports_exact_physical_state_without_an_apply() {
    let metadata = metadata(10);
    let request = GatewaySnapshotObservationRequest::new(
        metadata.node_id,
        4,
        format!("sha256:{}", "d".repeat(64)),
    )
    .expect("Gateway observation request");
    let command = NodeCommandEnvelope::new(
        metadata,
        NodeCommandPayload::GatewaySnapshotObserve {
            request: request.clone(),
        },
    )
    .expect("Gateway observation command");
    assert_eq!(command.generation, request.revision);
    assert_eq!(
        command.payload_schema,
        GatewaySnapshotObservationRequest::SCHEMA
    );

    let prior = AppliedGatewaySnapshot {
        gateway_id: request.gateway_id,
        revision: 3,
        expected_revision: Some(2),
        snapshot_digest: format!("sha256:{}", "c".repeat(64)),
        issued_at: command.issued_at - Duration::minutes(2),
        expires_at: command.issued_at + Duration::hours(1),
        applied_at: command.issued_at - Duration::minutes(1),
    };
    let observed_at = command.issued_at + Duration::milliseconds(10);
    let observation = NodeGatewaySnapshotObservation {
        schema: NodeGatewaySnapshotObservation::SCHEMA.into(),
        observation_id: Uuid::now_v7(),
        command_id: command.command_id,
        node_id: command.node_id,
        gateway_id: request.gateway_id,
        revision: request.revision,
        snapshot_digest: request.snapshot_digest.clone(),
        state: GatewaySnapshotObservationState::NotApplied,
        ready: false,
        applied: Some(prior),
        observed_at,
        management_protocol: GatewayManagementProtocol::advertised_v1(),
    };
    observation
        .validate_for(command.command_id, command.node_id, &request)
        .expect("exact unapplied observation with prior physical state");
    NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: observed_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::GatewaySnapshotObserved {
                observation: observation.clone(),
            }),
        },
    }
    .validate_against(&command)
    .expect("Gateway observation command acknowledgement");

    let mut false_applied = observation.clone();
    false_applied.state = GatewaySnapshotObservationState::Applied;
    false_applied.ready = true;
    assert!(false_applied.validate().is_err());

    let mut exact_applied = observation;
    exact_applied.state = GatewaySnapshotObservationState::Applied;
    exact_applied.ready = true;
    exact_applied.applied = Some(AppliedGatewaySnapshot {
        gateway_id: request.gateway_id,
        revision: request.revision,
        expected_revision: Some(3),
        snapshot_digest: request.snapshot_digest,
        issued_at: command.issued_at - Duration::seconds(1),
        expires_at: command.issued_at + Duration::hours(1),
        applied_at: command.issued_at,
    });
    exact_applied
        .validate()
        .expect("exact applied physical observation");
    let future_applied_at = exact_applied.observed_at + Duration::milliseconds(1);
    exact_applied
        .applied
        .as_mut()
        .expect("applied snapshot")
        .applied_at = future_applied_at;
    assert!(exact_applied.validate().is_err());
}

#[test]
fn gateway_tls_snapshot_binds_one_closed_certificate_request() {
    let certificate_id = Uuid::now_v7();
    let certificate = GatewayCertificateRequest::new(
        certificate_id,
        vec!["*.example.com".into(), "api.internal.example.com".into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let acl = format!(
        r#"entrypoints "https" {{
  address = "0.0.0.0:443"
  tls {{
    cert_file = "{}"
    key_file = "{}"
  }}
}}
"#,
        certificate.certificate_file, certificate.private_key_file
    );
    let issued_at = Utc::now();
    let snapshot = GatewaySnapshot::new_with_certificate(
        Uuid::now_v7(),
        5,
        Some(4),
        issued_at,
        issued_at + Duration::minutes(10),
        acl,
        Some(certificate.clone()),
    )
    .expect("TLS snapshot");
    snapshot.validate().expect("valid TLS snapshot");

    let mut changed_certificate = snapshot.clone();
    changed_certificate
        .certificate_request
        .as_mut()
        .expect("certificate")
        .dns_names = vec!["other.example.com".into()];
    changed_certificate
        .validate()
        .expect("certificate remains structurally valid");
    let original_digest = NodeCommandPayload::GatewaySnapshotInstall {
        snapshot: Box::new(snapshot.clone()),
    }
    .digest()
    .expect("original payload digest");
    let changed_digest = NodeCommandPayload::GatewaySnapshotInstall {
        snapshot: Box::new(changed_certificate),
    }
    .digest()
    .expect("changed payload digest");
    assert_ne!(original_digest, changed_digest);

    let mut missing_reference = snapshot;
    missing_reference.acl = "management { enabled = true }\n".into();
    assert!(missing_reference.validate().is_err());
}

#[test]
fn gateway_certificate_request_rejects_ambiguous_names_and_paths() {
    let certificate_id = Uuid::now_v7();
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["*.example.com".into(), "api.example.com".into()],
        "/cert.pem",
        "/key.pem",
    )
    .is_ok());
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["api.example.com".into(), "*.example.com".into()],
        "/cert.pem",
        "/key.pem",
    )
    .is_err());
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["*.*.example.com".into()],
        "/cert.pem",
        "/key.pem",
    )
    .is_err());
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["api.example.com".into()],
        "relative/cert.pem",
        "/key.pem",
    )
    .is_err());
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["api.example.com".into()],
        r"C:\a3s\gateway\cert.pem",
        r"C:\a3s\gateway\key.pem",
    )
    .is_ok());
    for invalid in [r"C:relative\cert.pem", r"C:\a3s\..\cert.pem"] {
        assert!(GatewayCertificateRequest::new(
            certificate_id,
            vec!["api.example.com".into()],
            invalid,
            r"C:\a3s\gateway\key.pem",
        )
        .is_err());
    }
}

#[test]
fn gateway_certificate_signing_contract_never_accepts_or_debugs_a_private_key() {
    let request = GatewayCertificateSigningRequest {
        schema: GatewayCertificateSigningRequest::SCHEMA.into(),
        certificate_id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        csr_pem:
            "-----BEGIN CERTIFICATE REQUEST-----\ndGVzdA==\n-----END CERTIFICATE REQUEST-----\n"
                .into(),
        requested_at: Utc::now(),
    };
    request.validate().expect("signing request");
    let debug = format!("{request:?}");
    assert!(debug.contains("<redacted-csr>"));
    assert!(!debug.contains("dGVzdA"));

    let mut leaked = request;
    leaked.csr_pem =
        "-----BEGIN CERTIFICATE REQUEST-----\nPRIVATE KEY\n-----END CERTIFICATE REQUEST-----\n"
            .into();
    assert!(leaked.validate().is_err());
}

#[test]
fn gateway_certificate_signing_contract_accepts_standard_pem_line_endings() {
    for csr_pem in [
        "-----BEGIN CERTIFICATE REQUEST-----\ndGVzdA==\n-----END CERTIFICATE REQUEST-----\n",
        "-----BEGIN CERTIFICATE REQUEST-----\r\ndGVzdA==\r\n-----END CERTIFICATE REQUEST-----\r\n",
    ] {
        GatewayCertificateSigningRequest {
            schema: GatewayCertificateSigningRequest::SCHEMA.into(),
            certificate_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            csr_pem: csr_pem.into(),
            requested_at: Utc::now(),
        }
        .validate()
        .expect("standard PEM line endings");
    }
}
