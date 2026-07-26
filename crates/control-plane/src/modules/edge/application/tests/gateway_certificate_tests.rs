use super::*;

#[tokio::test]
async fn signs_each_gateway_certificate_once_for_the_authenticated_node_and_csr() {
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let node_id = NodeId::new();
    let now = Utc::now();
    let staged = stage_certificate(
        Arc::clone(&routes),
        node_id,
        "api.example.com",
        "sign-certificate",
        now,
    )
    .await;
    let authority = Arc::new(RecordingGatewayCertificateAuthority {
        calls: AtomicUsize::new(0),
        unavailable: AtomicBool::new(false),
    });
    let repository: Arc<dyn IEdgeRepository> = routes.clone();
    let certificate_authority: Arc<dyn IGatewayCertificateAuthority> = authority.clone();
    let handler =
        SignGatewayCertificateHandler::new(repository, certificate_authority, Duration::days(30))
            .expect("signing handler");
    let request = GatewayCertificateSigningRequest {
        schema: GatewayCertificateSigningRequest::SCHEMA.into(),
        certificate_id: staged.certificate.id.as_uuid(),
        node_id: node_id.as_uuid(),
        csr_pem:
            "-----BEGIN CERTIFICATE REQUEST-----\ndGVzdA==\n-----END CERTIFICATE REQUEST-----\n"
                .into(),
        requested_at: now + Duration::milliseconds(1),
    };
    let command = SignGatewayCertificate {
        authenticated_node_id: node_id,
        request: request.clone(),
        received_at: now + Duration::seconds(1),
    };
    let issued = handler
        .execute(command.clone(), context())
        .await
        .expect("command bus")
        .expect("issue certificate");
    assert_eq!(issued.dns_names, vec!["api.example.com"]);
    assert!(!issued.certificate_pem.contains("PRIVATE KEY"));
    let replay = handler
        .execute(
            SignGatewayCertificate {
                received_at: now + Duration::seconds(2),
                ..command
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect("replay certificate");
    assert_eq!(replay, issued);
    assert_eq!(authority.calls.load(Ordering::SeqCst), 1);

    let conflicting = handler
        .execute(
            SignGatewayCertificate {
                authenticated_node_id: node_id,
                request: GatewayCertificateSigningRequest {
                    csr_pem: "-----BEGIN CERTIFICATE REQUEST-----\nY29uZmxpY3Q=\n-----END CERTIFICATE REQUEST-----\n".into(),
                    ..request
                },
                received_at: now + Duration::seconds(3),
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect_err("different CSR must conflict");
    assert!(matches!(
        conflicting,
        crate::modules::shared_kernel::application::ApplicationError::Conflict(_)
    ));
    assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn keeps_unavailable_gateway_certificate_authority_retryable_without_provider_details() {
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let node_id = NodeId::new();
    let now = Utc::now();
    let staged = stage_certificate(
        Arc::clone(&routes),
        node_id,
        "failed.example.com",
        "failed-signing",
        now,
    )
    .await;
    let repository: Arc<dyn IEdgeRepository> = routes.clone();
    let authority = Arc::new(RecordingGatewayCertificateAuthority {
        calls: AtomicUsize::new(0),
        unavailable: AtomicBool::new(true),
    });
    let handler =
        SignGatewayCertificateHandler::new(repository, authority.clone(), Duration::days(30))
            .expect("signing handler");
    let command = SignGatewayCertificate {
        authenticated_node_id: node_id,
        request: GatewayCertificateSigningRequest {
            schema: GatewayCertificateSigningRequest::SCHEMA.into(),
            certificate_id: staged.certificate.id.as_uuid(),
            node_id: node_id.as_uuid(),
            csr_pem:
                "-----BEGIN CERTIFICATE REQUEST-----\nZmFpbA==\n-----END CERTIFICATE REQUEST-----\n"
                    .into(),
            requested_at: now + Duration::milliseconds(1),
        },
        received_at: now + Duration::seconds(1),
    };
    let result = handler
        .execute(command.clone(), context())
        .await
        .expect("command bus")
        .expect_err("unavailable authority");
    assert!(matches!(
        result,
        crate::modules::shared_kernel::application::ApplicationError::Internal(_)
    ));
    let stored = routes
        .find_gateway_certificate(node_id, staged.certificate.id)
        .await
        .expect("pending certificate");
    assert_eq!(
        stored.state,
        crate::modules::edge::domain::GatewayCertificateState::Provisioning
    );
    assert_eq!(stored.failure, None);
    assert_eq!(
        stored.aggregate_version,
        staged.certificate.aggregate_version
    );

    authority.unavailable.store(false, Ordering::SeqCst);
    let issued = handler
        .execute(command, context())
        .await
        .expect("command bus")
        .expect("retry Gateway certificate signing");
    assert_eq!(issued.serial_number, staged.certificate.id.to_string());
    assert_eq!(authority.calls.load(Ordering::SeqCst), 2);
}
