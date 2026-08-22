use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use a3s_cloud_control_plane::modules::edge::domain::events::{
    expiry_risk_subject_id, renewal_subject_id, DomainClaimChanged,
    GatewayCertificateExpiryRiskChanged, GatewayCertificateRenewalChanged,
    GatewayCertificateRenewalFailureKind, GatewayCertificateRenewalStatus,
};
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, IEdgeRepository, TransitionDomainClaim,
};
use a3s_cloud_control_plane::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayCommandDispatch,
    IGatewayCertificateAuthority, IGatewayCommandQueue,
};
use a3s_cloud_control_plane::modules::edge::infrastructure::persistence::PostgresEdgeRepository;
use a3s_cloud_control_plane::modules::edge::{
    DomainClaim, GatewayCertificate, GatewayCertificateConvergenceReason,
    GatewayCertificateConvergenceState, GatewayCertificateExpiryRiskState,
    GatewayCertificateMaterial, GatewayCertificateReconciler, GatewayCertificateState,
    GatewayPublication, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, RouteState,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, NodeId, OrganizationId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::{Duration, Timelike, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct GatewayCertificateLifecycleScenario {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub domain_claim: DomainClaim,
    pub started_at: chrono::DateTime<Utc>,
}

#[derive(Default)]
struct RecordingGatewayQueue {
    publications: Mutex<Vec<GatewayPublication>>,
}

#[async_trait]
impl IGatewayCommandQueue for RecordingGatewayQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        let mut publications = self.publications.lock().await;
        let replayed = publications
            .iter()
            .any(|existing| existing.command_id == publication.command_id);
        publications.push(publication.clone());
        Ok(GatewayCommandDispatch { replayed })
    }
}

#[derive(Default)]
struct RecordingGatewayCertificateAuthority {
    fail_revoke: AtomicBool,
    revoked_serials: Mutex<Vec<String>>,
}

#[async_trait]
impl IGatewayCertificateAuthority for RecordingGatewayCertificateAuthority {
    async fn issue(
        &self,
        _request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        Err(GatewayCertificateAuthorityError::Unavailable(
            "integration test issues certificate material directly".into(),
        ))
    }

    async fn revoke(
        &self,
        certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        if self.fail_revoke.load(Ordering::SeqCst) {
            return Err(GatewayCertificateAuthorityError::Unavailable(
                "vault token=postgres-provider-secret\nunavailable".into(),
            ));
        }
        self.revoked_serials.lock().await.push(
            certificate
                .material
                .as_ref()
                .ok_or_else(|| {
                    GatewayCertificateAuthorityError::InvalidRequest(
                        "certificate has no material".into(),
                    )
                })?
                .serial_number
                .clone(),
        );
        Ok(())
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Ok(!self.fail_revoke.load(Ordering::SeqCst))
    }
}

pub async fn exercise(
    executor: &PostgresExecutor,
    mut scenario: GatewayCertificateLifecycleScenario,
) -> Result<(), Box<dyn std::error::Error>> {
    scenario.started_at = canonical_test_timestamp(scenario.started_at);
    let database = Database::new(PostgresDialect, executor.clone());
    let repository = Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let edge: Arc<dyn IEdgeRepository> = repository.clone();
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = GatewayCertificateReconciler::new(
        edge,
        queue.clone(),
        authority.clone(),
        compiler()?,
        std::time::Duration::from_secs(60),
        Duration::days(7),
        Duration::hours(6),
        Duration::minutes(3),
        100,
    )?;
    let mut before_renewal = repository.active_routes(scenario.node_id).await?;
    if before_renewal.is_empty() {
        return Err("certificate lifecycle requires active routes".into());
    }
    let previous_certificate_id = before_renewal[0]
        .gateway_certificate_id
        .ok_or("active route has no Gateway certificate")?;
    if before_renewal
        .iter()
        .any(|route| route.gateway_certificate_id != Some(previous_certificate_id))
    {
        return Err("active routes do not share the installed Gateway certificate".into());
    }
    let previous_certificate = repository
        .find_gateway_certificate(scenario.node_id, previous_certificate_id)
        .await?;
    let previous_serial = previous_certificate
        .material
        .as_ref()
        .ok_or("installed certificate has no material")?
        .serial_number
        .clone();
    let previous_certificate_version = previous_certificate.aggregate_version;
    let initial_revision = before_renewal[0]
        .gateway_revision
        .ok_or("active route has no Gateway revision")?;
    let initial_digest = before_renewal[0]
        .snapshot_digest
        .clone()
        .ok_or("active route has no snapshot digest")?;
    let snapshot_renew_at =
        scenario.domain_claim.created_at + Duration::hours(18) + Duration::minutes(1);
    let snapshot_report = reconciler.run_once(snapshot_renew_at).await?;
    assert_eq!(snapshot_report.convergence_targets, 1);
    assert_eq!(snapshot_report.staged_convergences, 1);
    assert!(snapshot_report.failures.is_empty());
    let snapshot_renewal = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        snapshot_renewal.convergence.reason,
        GatewayCertificateConvergenceReason::SnapshotRenewal
    );
    assert_eq!(
        snapshot_renewal.publication.expected_revision,
        Some(initial_revision)
    );
    assert_eq!(snapshot_renewal.publication.snapshot_digest, initial_digest);
    assert!(snapshot_renewal.publication.certificate_request.is_none());
    assert!(snapshot_renewal
        .convergence
        .replacement_certificate_id
        .is_none());
    assert!(snapshot_renewal.certificate.is_none());
    let snapshot_applied = acknowledgement(
        &snapshot_renewal,
        GatewayAckState::Applied,
        snapshot_renew_at + Duration::milliseconds(100),
    );
    repository
        .project_gateway_acknowledgement(
            &snapshot_applied,
            snapshot_applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    before_renewal = repository.active_routes(scenario.node_id).await?;
    assert!(before_renewal.iter().all(|route| {
        route.gateway_revision == Some(snapshot_renewal.publication.revision)
            && route.gateway_command_id == Some(snapshot_renewal.publication.command_id)
            && route.snapshot_digest.as_deref() == Some(initial_digest.as_str())
            && route.gateway_certificate_id == Some(previous_certificate_id)
    }));
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .aggregate_version,
        previous_certificate_version
    );
    let scope_before = repository.gateway_scope(scenario.node_id).await?;

    let previous_expires_at = previous_certificate
        .material
        .as_ref()
        .ok_or("installed certificate lost its material")?
        .expires_at;
    assert_eq!(
        previous_expires_at,
        scenario.started_at + Duration::hours(24),
        "PostgreSQL expiry-risk proof must exercise threshold equality"
    );
    let risk_targets = repository
        .gateway_certificate_expiry_risk_targets(scenario.started_at + Duration::hours(24), 100)
        .await?;
    assert_eq!(risk_targets.len(), before_renewal.len());
    assert!(risk_targets.len() >= 2);
    let rollback_target = risk_targets[0].clone();
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(
            "alter table outbox_events add constraint gateway_certificate_expiry_risk_outbox_failure_probe check (event_key <> 'edge.gateway-certificate.expiry-at-risk')",
        )
        .await?;
    drop(connection);
    assert!(repository
        .mark_gateway_certificate_expiry_at_risk(
            rollback_target.route.organization_id,
            rollback_target.route.id,
            rollback_target.route.gateway_node_id,
            rollback_target.certificate.id,
            scenario.started_at,
        )
        .await
        .is_err());
    assert!(repository
        .find_gateway_certificate_expiry_risk(
            rollback_target.route.id,
            rollback_target.route.gateway_node_id,
        )
        .await?
        .is_none());
    assert!(
        expiry_risk_fact_rows(&database, scenario.organization_id, scenario.node_id)
            .await?
            .as_array()
            .ok_or("rolled-back certificate expiry-risk facts are not an array")?
            .is_empty()
    );
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(
            "alter table outbox_events drop constraint gateway_certificate_expiry_risk_outbox_failure_probe",
        )
        .await?;
    drop(connection);

    let concurrent_a = PostgresEdgeRepository::new(executor.clone());
    let concurrent_b = PostgresEdgeRepository::new(executor.clone());
    let concurrent_route = rollback_target.route.clone();
    let concurrent_certificate = rollback_target.certificate.clone();
    let (first_entry, second_entry) = tokio::join!(
        concurrent_a.mark_gateway_certificate_expiry_at_risk(
            concurrent_route.organization_id,
            concurrent_route.id,
            concurrent_route.gateway_node_id,
            concurrent_certificate.id,
            scenario.started_at,
        ),
        concurrent_b.mark_gateway_certificate_expiry_at_risk(
            concurrent_route.organization_id,
            concurrent_route.id,
            concurrent_route.gateway_node_id,
            concurrent_certificate.id,
            scenario.started_at,
        )
    );
    assert_eq!(
        [first_entry?, second_entry?]
            .into_iter()
            .filter(|changed| *changed)
            .count(),
        1,
        "concurrent expiry-risk entry must commit exactly once"
    );
    assert!(
        !repository
            .mark_gateway_certificate_expiry_at_risk(
                concurrent_route.organization_id,
                concurrent_route.id,
                concurrent_route.gateway_node_id,
                concurrent_certificate.id,
                scenario.started_at,
            )
            .await?
    );

    let first_report = reconciler.run_once(scenario.started_at).await?;
    assert_eq!(first_report.expiry_risk_targets, before_renewal.len() - 1);
    assert_eq!(
        first_report.projected_expiry_risks,
        before_renewal.len() - 1
    );
    assert_eq!(first_report.convergence_targets, 1);
    assert_eq!(first_report.staged_convergences, 1);
    assert!(first_report.failures.is_empty());
    for route in &before_renewal {
        let risk = repository
            .find_gateway_certificate_expiry_risk(route.id, scenario.node_id)
            .await?
            .ok_or("PostgreSQL expiry-risk projection disappeared")?;
        assert_eq!(risk.state, GatewayCertificateExpiryRiskState::AtRisk);
        assert_eq!(risk.active_certificate_id, previous_certificate_id);
        assert_eq!(risk.active_certificate_expires_at, previous_expires_at);
        assert_eq!(risk.generation, 1);
        assert_eq!(risk.updated_at, scenario.started_at);
    }
    let initial_risk_facts =
        expiry_risk_fact_rows(&database, scenario.organization_id, scenario.node_id).await?;
    let initial_risk_facts = initial_risk_facts
        .as_array()
        .ok_or("certificate expiry-risk facts are not an array")?;
    assert_eq!(initial_risk_facts.len(), before_renewal.len());
    assert!(!serde_json::to_string(initial_risk_facts)?.contains("BEGIN CERTIFICATE"));
    assert!(!serde_json::to_string(initial_risk_facts)?.contains("private-key"));
    for route in &before_renewal {
        let fact = initial_risk_facts
            .iter()
            .find(|event| {
                event["aggregateId"]
                    == expiry_risk_subject_id(route.id, scenario.node_id).to_string()
            })
            .ok_or("Route-local certificate expiry-risk fact is missing")?;
        assert_eq!(fact["eventKey"], "edge.gateway-certificate.expiry-at-risk");
        assert_eq!(fact["schemaVersion"], 1);
        assert_eq!(fact["aggregateVersion"], 1);
        let payload: GatewayCertificateExpiryRiskChanged =
            serde_json::from_value(fact["payload"].clone())?;
        assert_eq!(payload.route_id, route.id);
        assert_eq!(payload.node_id, scenario.node_id);
        assert_eq!(payload.active_certificate_id, previous_certificate_id);
        assert_eq!(payload.active_certificate_expires_at, previous_expires_at);
        assert_eq!(payload.state, GatewayCertificateExpiryRiskState::AtRisk);
    }
    let first = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        first.convergence.reason,
        GatewayCertificateConvergenceReason::Renewal
    );
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        before_renewal
    );
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );
    issue_certificate(
        repository.as_ref(),
        first
            .certificate
            .as_ref()
            .ok_or("renewal omitted replacement certificate")?,
        scenario.started_at + Duration::milliseconds(100),
    )
    .await?;
    let rejected = acknowledgement(
        &first,
        GatewayAckState::Rejected,
        scenario.started_at + Duration::milliseconds(200),
    );
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(
            "alter table outbox_events add constraint gateway_certificate_renewal_outbox_failure_probe check (event_key <> 'edge.gateway-certificate.renewal-failed')",
        )
        .await?;
    drop(connection);
    assert!(repository
        .project_gateway_acknowledgement(
            &rejected,
            rejected.acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .is_err());
    assert_eq!(
        repository
            .find_gateway_certificate_convergence(scenario.node_id, first.publication.revision)
            .await?
            .ok_or("rolled-back convergence disappeared")?
            .state,
        GatewayCertificateConvergenceState::Pending
    );
    assert!(
        renewal_fact_rows(&database, scenario.organization_id, scenario.node_id,)
            .await?
            .as_array()
            .ok_or("rolled-back certificate renewal facts are not an array")?
            .is_empty()
    );
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(
            "alter table outbox_events drop constraint gateway_certificate_renewal_outbox_failure_probe",
        )
        .await?;
    drop(connection);
    repository
        .project_gateway_acknowledgement(
            &rejected,
            rejected.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    assert_eq!(
        repository
            .find_gateway_certificate_convergence(scenario.node_id, first.publication.revision,)
            .await?
            .ok_or("rejected convergence disappeared")?
            .state,
        GatewayCertificateConvergenceState::Rejected
    );
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        before_renewal
    );
    assert_eq!(
        repository
            .gateway_scope(scenario.node_id)
            .await?
            .installed_revision,
        scope_before.installed_revision
    );
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );
    repository
        .project_gateway_acknowledgement(
            &rejected,
            rejected.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    for route in &before_renewal {
        let risk = repository
            .find_gateway_certificate_expiry_risk(route.id, scenario.node_id)
            .await?
            .ok_or("rejected renewal removed expiry risk")?;
        assert_eq!(risk.state, GatewayCertificateExpiryRiskState::AtRisk);
        assert_eq!(risk.generation, 1);
    }
    assert_eq!(
        expiry_risk_fact_rows(&database, scenario.organization_id, scenario.node_id)
            .await?
            .as_array()
            .ok_or("expiry-risk facts after rejection are not an array")?
            .len(),
        before_renewal.len()
    );

    let retry_at = scenario.started_at + Duration::seconds(1);
    let retry_report = reconciler.run_once(retry_at).await?;
    assert_eq!(retry_report.staged_convergences, 1);
    let short_renewal = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        short_renewal.convergence.reason,
        GatewayCertificateConvergenceReason::Renewal
    );
    let short_replacement = short_renewal
        .certificate
        .as_ref()
        .ok_or("short-lived renewal omitted replacement certificate")?
        .clone();
    let short_expires_at = retry_at + Duration::hours(12);
    issue_certificate_until(
        repository.as_ref(),
        &short_replacement,
        retry_at + Duration::milliseconds(100),
        short_expires_at,
    )
    .await?;
    let short_applied = acknowledgement(
        &short_renewal,
        GatewayAckState::Applied,
        retry_at + Duration::milliseconds(200),
    );
    repository
        .project_gateway_acknowledgement(
            &short_applied,
            short_applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    repository
        .project_gateway_acknowledgement(
            &short_applied,
            short_applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    let after_short_renewal = repository.active_routes(scenario.node_id).await?;
    assert!(after_short_renewal.iter().all(|route| {
        route.state == RouteState::Active
            && route.gateway_certificate_id == Some(short_replacement.id)
            && route.gateway_revision == Some(short_renewal.publication.revision)
    }));
    for route in &before_renewal {
        let risk = repository
            .find_gateway_certificate_expiry_risk(route.id, scenario.node_id)
            .await?
            .ok_or("short-lived Applied replacement removed expiry risk")?;
        assert_eq!(risk.state, GatewayCertificateExpiryRiskState::AtRisk);
        assert_eq!(risk.active_certificate_id, short_replacement.id);
        assert_eq!(risk.active_certificate_expires_at, short_expires_at);
        assert_eq!(risk.generation, 2);
    }
    assert_eq!(
        expiry_risk_fact_rows(&database, scenario.organization_id, scenario.node_id)
            .await?
            .as_array()
            .ok_or("short-lived expiry-risk facts are not an array")?
            .len(),
        before_renewal.len() * 2
    );

    authority.fail_revoke.store(true, Ordering::SeqCst);
    let safe_retry_at = retry_at + Duration::seconds(1);
    let safe_retry_report = reconciler.run_once(safe_retry_at).await?;
    assert_eq!(safe_retry_report.expiry_risk_targets, 0);
    assert_eq!(safe_retry_report.staged_convergences, 1);
    assert_eq!(safe_retry_report.obsolete_certificates, 1);
    assert_eq!(safe_retry_report.revoked_certificates, 0);
    assert_eq!(safe_retry_report.failures.len(), 1);
    assert_eq!(
        safe_retry_report.failures[0].error,
        "Gateway certificate authority is unavailable"
    );
    authority.fail_revoke.store(false, Ordering::SeqCst);
    let renewal = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        renewal.convergence.reason,
        GatewayCertificateConvergenceReason::Renewal
    );
    let replacement = renewal
        .certificate
        .as_ref()
        .ok_or("safe renewal omitted replacement certificate")?
        .clone();
    issue_certificate(
        repository.as_ref(),
        &replacement,
        safe_retry_at + Duration::milliseconds(100),
    )
    .await?;
    let applied = acknowledgement(
        &renewal,
        GatewayAckState::Applied,
        safe_retry_at + Duration::milliseconds(200),
    );
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(
            "alter table outbox_events add constraint gateway_certificate_expiry_clear_outbox_failure_probe check (event_key <> 'edge.gateway-certificate.expiry-risk-cleared')",
        )
        .await?;
    drop(connection);
    assert!(repository
        .project_gateway_acknowledgement(
            &applied,
            applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .is_err());
    assert_eq!(
        repository
            .find_gateway_certificate_convergence(scenario.node_id, renewal.publication.revision)
            .await?
            .ok_or("rolled-back safe convergence disappeared")?
            .state,
        GatewayCertificateConvergenceState::Pending
    );
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        after_short_renewal
    );
    for route in &before_renewal {
        let risk = repository
            .find_gateway_certificate_expiry_risk(route.id, scenario.node_id)
            .await?
            .ok_or("rolled-back clear removed expiry risk")?;
        assert_eq!(risk.state, GatewayCertificateExpiryRiskState::AtRisk);
        assert_eq!(risk.active_certificate_id, short_replacement.id);
        assert_eq!(risk.generation, 2);
    }
    assert_eq!(
        expiry_risk_fact_rows(&database, scenario.organization_id, scenario.node_id)
            .await?
            .as_array()
            .ok_or("rolled-back clear facts are not an array")?
            .len(),
        before_renewal.len() * 2
    );
    let connection = executor.pool().get().await?;
    connection
        .batch_execute(
            "alter table outbox_events drop constraint gateway_certificate_expiry_clear_outbox_failure_probe",
        )
        .await?;
    drop(connection);
    repository
        .project_gateway_acknowledgement(
            &applied,
            applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    repository
        .project_gateway_acknowledgement(
            &applied,
            applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    let after_renewal = repository.active_routes(scenario.node_id).await?;
    assert_eq!(after_renewal.len(), before_renewal.len());
    assert!(after_renewal.iter().all(|route| {
        route.state == RouteState::Active
            && route.gateway_certificate_id == Some(replacement.id)
            && route.gateway_revision == Some(renewal.publication.revision)
    }));
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );

    let renewal_facts =
        renewal_fact_rows(&database, scenario.organization_id, scenario.node_id).await?;
    let renewal_facts = renewal_facts
        .as_array()
        .ok_or("certificate renewal facts are not an array")?;
    assert_eq!(renewal_facts.len(), before_renewal.len() * 3);
    assert!(!serde_json::to_string(renewal_facts)?.contains("reload rejected"));
    let replacement_expires_at = repository
        .find_gateway_certificate(scenario.node_id, replacement.id)
        .await?
        .material
        .ok_or("renewed certificate lost its material")?
        .expires_at;
    let expiry_risk_facts =
        expiry_risk_fact_rows(&database, scenario.organization_id, scenario.node_id).await?;
    let expiry_risk_facts = expiry_risk_facts
        .as_array()
        .ok_or("certificate expiry-risk facts are not an array")?;
    assert_eq!(expiry_risk_facts.len(), before_renewal.len() * 3);
    let encoded_expiry_risks = serde_json::to_string(expiry_risk_facts)?;
    for private_fragment in [
        "BEGIN CERTIFICATE",
        "private-key",
        "reload rejected",
        "credential",
    ] {
        assert!(!encoded_expiry_risks.contains(private_fragment));
    }
    for route in &before_renewal {
        let risk = repository
            .find_gateway_certificate_expiry_risk(route.id, scenario.node_id)
            .await?
            .ok_or("cleared PostgreSQL expiry risk disappeared")?;
        assert_eq!(risk.state, GatewayCertificateExpiryRiskState::Clear);
        assert_eq!(risk.active_certificate_id, replacement.id);
        assert_eq!(risk.active_certificate_expires_at, replacement_expires_at);
        assert_eq!(
            risk.previous_at_risk_certificate_id,
            Some(short_replacement.id)
        );
        assert_eq!(
            risk.previous_at_risk_certificate_expires_at,
            Some(short_expires_at)
        );
        assert_eq!(risk.generation, 3);

        let subject_id = expiry_risk_subject_id(route.id, scenario.node_id);
        let subject_facts = expiry_risk_facts
            .iter()
            .filter(|event| event["aggregateId"] == subject_id.to_string())
            .collect::<Vec<_>>();
        assert_eq!(subject_facts.len(), 3);
        let cleared = subject_facts
            .iter()
            .find(|event| event["eventKey"] == "edge.gateway-certificate.expiry-risk-cleared")
            .ok_or("Route-local certificate expiry-risk clear fact is missing")?;
        let refreshed = subject_facts
            .iter()
            .find(|event| {
                event["eventKey"] == "edge.gateway-certificate.expiry-at-risk"
                    && event["aggregateVersion"] == 2
            })
            .ok_or("short-lived certificate expiry-risk refresh fact is missing")?;
        let refreshed_payload: GatewayCertificateExpiryRiskChanged =
            serde_json::from_value(refreshed["payload"].clone())?;
        assert_eq!(
            refreshed_payload.active_certificate_id,
            short_replacement.id
        );
        assert_eq!(
            refreshed_payload.active_certificate_expires_at,
            short_expires_at
        );
        assert_eq!(
            refreshed_payload.state,
            GatewayCertificateExpiryRiskState::AtRisk
        );
        assert_eq!(cleared["schemaVersion"], 1);
        assert_eq!(cleared["aggregateVersion"], 3);
        assert_eq!(
            cleared["correlationId"],
            renewal.publication.command_correlation_id.to_string()
        );
        let payload: GatewayCertificateExpiryRiskChanged =
            serde_json::from_value(cleared["payload"].clone())?;
        assert_eq!(payload.route_id, route.id);
        assert_eq!(payload.node_id, scenario.node_id);
        assert_eq!(payload.active_certificate_id, replacement.id);
        assert_eq!(
            payload.active_certificate_expires_at,
            replacement_expires_at
        );
        assert_eq!(payload.state, GatewayCertificateExpiryRiskState::Clear);
        assert_eq!(
            payload.previous_at_risk_certificate_id,
            Some(short_replacement.id)
        );
        assert_eq!(
            payload.previous_at_risk_certificate_expires_at,
            Some(short_expires_at)
        );
    }
    for route in &before_renewal {
        let subject_id = renewal_subject_id(route.id, scenario.node_id);
        let subject_facts = renewal_facts
            .iter()
            .filter(|event| event["aggregateId"] == subject_id.to_string())
            .collect::<Vec<_>>();
        assert_eq!(subject_facts.len(), 3);
        let failed_fact = subject_facts
            .iter()
            .find(|event| event["eventKey"] == "edge.gateway-certificate.renewal-failed")
            .ok_or("route renewal failure fact is missing")?;
        let renewed_fact = subject_facts
            .iter()
            .find(|event| {
                event["eventKey"] == "edge.gateway-certificate.renewed"
                    && event["aggregateVersion"] == renewal.publication.revision
            })
            .ok_or("route renewal recovery fact is missing")?;
        assert_eq!(failed_fact["schemaVersion"], 1);
        assert_eq!(failed_fact["aggregateVersion"], first.publication.revision);
        assert_eq!(
            failed_fact["correlationId"],
            first.publication.command_correlation_id.to_string()
        );
        assert_eq!(renewed_fact["schemaVersion"], 1);
        assert_eq!(
            renewed_fact["aggregateVersion"],
            renewal.publication.revision
        );
        assert_eq!(
            renewed_fact["correlationId"],
            renewal.publication.command_correlation_id.to_string()
        );
        let failed_payload: GatewayCertificateRenewalChanged =
            serde_json::from_value(failed_fact["payload"].clone())?;
        let renewed_payload: GatewayCertificateRenewalChanged =
            serde_json::from_value(renewed_fact["payload"].clone())?;
        assert_eq!(failed_payload.route_id, route.id);
        assert_eq!(failed_payload.project_id, route.project_id);
        assert_eq!(failed_payload.environment_id, route.environment_id);
        assert_eq!(failed_payload.node_id, scenario.node_id);
        assert_eq!(
            failed_payload.status,
            GatewayCertificateRenewalStatus::Failed
        );
        assert_eq!(
            failed_payload.failure_kind,
            Some(GatewayCertificateRenewalFailureKind::Rejected)
        );
        assert_eq!(
            failed_payload.active_certificate_id,
            previous_certificate_id
        );
        assert_eq!(
            failed_payload.active_certificate_expires_at,
            previous_expires_at
        );
        assert_eq!(renewed_payload.route_id, route.id);
        assert_eq!(
            renewed_payload.status,
            GatewayCertificateRenewalStatus::Renewed
        );
        assert_eq!(renewed_payload.failure_kind, None);
        assert_eq!(renewed_payload.active_certificate_id, replacement.id);
        assert_eq!(
            renewed_payload.active_certificate_expires_at,
            replacement_expires_at
        );
    }

    authority.fail_revoke.store(true, Ordering::SeqCst);
    let failed_revocation = reconciler
        .run_once(scenario.started_at + Duration::seconds(3))
        .await?;
    assert_eq!(failed_revocation.obsolete_certificates, 2);
    assert_eq!(failed_revocation.revoked_certificates, 0);
    assert_eq!(failed_revocation.failures.len(), 2);
    assert!(failed_revocation.failures.iter().all(|failure| {
        failure.error == "Gateway certificate authority is unavailable"
            && !failure.error.contains("postgres-provider-secret")
    }));
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Ready
    );
    authority.fail_revoke.store(false, Ordering::SeqCst);
    let revoked = reconciler
        .run_once(scenario.started_at + Duration::milliseconds(3_500))
        .await?;
    assert_eq!(revoked.revoked_certificates, 2);
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, previous_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Revoked
    );
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, short_replacement.id)
            .await?
            .state,
        GatewayCertificateState::Revoked
    );
    assert!(authority
        .revoked_serials
        .lock()
        .await
        .contains(&previous_serial));
    assert!(authority
        .revoked_serials
        .lock()
        .await
        .contains(&short_replacement.id.to_string()));

    let mut expected_rejected_route_ids = after_renewal
        .iter()
        .filter(|route| route.domain_claim_id == Some(scenario.domain_claim.id))
        .map(|route| route.id)
        .collect::<Vec<_>>();
    let mut expected_retained_route_ids = after_renewal
        .iter()
        .filter(|route| route.domain_claim_id != Some(scenario.domain_claim.id))
        .map(|route| route.id)
        .collect::<Vec<_>>();
    expected_rejected_route_ids.sort();
    expected_retained_route_ids.sort();
    assert!(!expected_rejected_route_ids.is_empty());
    assert!(!expected_retained_route_ids.is_empty());

    let expected_claim_version = scenario.domain_claim.aggregate_version;
    scenario.domain_claim.revoke(
        "integration ownership removed",
        scenario.started_at + Duration::seconds(4),
    )?;
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: scenario.domain_claim.clone(),
            expected_version: expected_claim_version,
            idempotency: IdempotencyRequest::new(
                format!("domain-claims/{}/revoke", scenario.domain_claim.id),
                "postgres-certificate-lifecycle",
                b"integration ownership removed",
            )?,
            event: DomainClaimChanged::envelope(&scenario.domain_claim, Uuid::now_v7())?,
        })
        .await?;
    let filtered_at = scenario.started_at + Duration::seconds(5);
    let filtered_report = reconciler.run_once(filtered_at).await?;
    assert_eq!(filtered_report.staged_convergences, 1);
    let filtered = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        filtered.convergence.reason,
        GatewayCertificateConvergenceReason::DomainRevocation
    );
    let mut rejected_route_ids = filtered
        .convergence
        .rejected_routes
        .iter()
        .map(|route| route.route_id)
        .collect::<Vec<_>>();
    let mut retained_route_ids = filtered
        .convergence
        .retained_routes
        .iter()
        .map(|route| route.route_id)
        .collect::<Vec<_>>();
    rejected_route_ids.sort();
    retained_route_ids.sort();
    assert_eq!(rejected_route_ids, expected_rejected_route_ids);
    assert_eq!(retained_route_ids, expected_retained_route_ids);
    let filtered_replacement = filtered
        .certificate
        .as_ref()
        .ok_or("filtered convergence omitted replacement certificate")?;
    assert_eq!(
        filtered.convergence.replacement_certificate_id,
        Some(filtered_replacement.id)
    );
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        after_renewal
    );
    issue_certificate(
        repository.as_ref(),
        filtered_replacement,
        filtered_at + Duration::milliseconds(100),
    )
    .await?;
    let filtered_ack = acknowledgement(
        &filtered,
        GatewayAckState::Applied,
        filtered_at + Duration::milliseconds(200),
    );
    repository
        .project_gateway_acknowledgement(
            &filtered_ack,
            filtered_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    let after_filter = repository.active_routes(scenario.node_id).await?;
    let mut active_route_ids = after_filter
        .iter()
        .map(|route| route.id)
        .collect::<Vec<_>>();
    active_route_ids.sort();
    assert_eq!(active_route_ids, expected_retained_route_ids);
    assert!(after_filter.iter().all(|route| {
        route.gateway_certificate_id == Some(filtered_replacement.id)
            && route.gateway_revision == Some(filtered.publication.revision)
    }));
    for route_id in expected_rejected_route_ids {
        let stored = repository
            .find_route(scenario.organization_id, route_id)
            .await?;
        assert_eq!(stored.state, RouteState::Rejected);
        assert_eq!(
            stored.failure.as_deref(),
            Some("domain ownership is no longer verified")
        );
    }

    let remaining_revoked_at = scenario.started_at + Duration::seconds(6);
    let mut remaining_claim_ids = after_filter
        .iter()
        .filter_map(|route| route.domain_claim_id)
        .collect::<Vec<_>>();
    remaining_claim_ids.sort();
    remaining_claim_ids.dedup();
    for claim_id in remaining_claim_ids {
        let mut claim = repository
            .find_domain_claim(scenario.organization_id, claim_id)
            .await?;
        let expected_version = claim.aggregate_version;
        let reason = "integration remaining ownership removed";
        claim.revoke(reason, remaining_revoked_at)?;
        repository
            .transition_domain_claim(TransitionDomainClaim {
                claim: claim.clone(),
                expected_version,
                idempotency: IdempotencyRequest::new(
                    format!("domain-claims/{claim_id}/revoke"),
                    format!("postgres-certificate-lifecycle-{claim_id}"),
                    reason.as_bytes(),
                )?,
                event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
            })
            .await?;
    }
    let route_less_at = scenario.started_at + Duration::seconds(7);
    let route_less_report = reconciler.run_once(route_less_at).await?;
    assert_eq!(route_less_report.staged_convergences, 1);
    assert_eq!(route_less_report.revoked_certificates, 1);
    let route_less = pending_for(repository.as_ref(), scenario.node_id).await?;
    assert_eq!(
        route_less.convergence.reason,
        GatewayCertificateConvergenceReason::DomainRevocation
    );
    assert!(route_less.convergence.retained_routes.is_empty());
    assert_eq!(
        route_less.convergence.rejected_routes.len(),
        after_filter.len()
    );
    assert_eq!(route_less.convergence.replacement_certificate_id, None);
    assert_eq!(route_less.certificate, None);
    assert_eq!(route_less.publication.certificate_request, None);
    assert!(!route_less.publication.acl.contains("routers \""));
    assert_eq!(
        repository.active_routes(scenario.node_id).await?,
        after_filter
    );
    let route_less_ack = acknowledgement(
        &route_less,
        GatewayAckState::Applied,
        route_less_at + Duration::milliseconds(100),
    );
    repository
        .project_gateway_acknowledgement(
            &route_less_ack,
            route_less_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    assert!(repository.active_routes(scenario.node_id).await?.is_empty());
    for route in after_filter {
        let stored = repository
            .find_route(scenario.organization_id, route.id)
            .await?;
        assert_eq!(stored.state, RouteState::Rejected);
        assert_eq!(
            stored.failure.as_deref(),
            Some("domain ownership is no longer verified")
        );
    }
    assert_eq!(
        repository
            .gateway_scope(scenario.node_id)
            .await?
            .installed_revision,
        Some(route_less.publication.revision)
    );

    let replacement_serial = replacement.id.to_string();
    let filtered_replacement_serial = filtered_replacement.id.to_string();
    let final_revocation = reconciler
        .run_once(scenario.started_at + Duration::seconds(8))
        .await?;
    assert_eq!(final_revocation.revoked_certificates, 1);
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, replacement.id)
            .await?
            .state,
        GatewayCertificateState::Revoked
    );
    assert_eq!(
        repository
            .find_gateway_certificate(scenario.node_id, filtered_replacement.id)
            .await?
            .state,
        GatewayCertificateState::Revoked
    );
    assert!(authority
        .revoked_serials
        .lock()
        .await
        .contains(&replacement_serial));
    assert!(authority
        .revoked_serials
        .lock()
        .await
        .contains(&filtered_replacement_serial));
    assert_eq!(
        renewal_fact_rows(&database, scenario.organization_id, scenario.node_id,)
            .await?
            .as_array()
            .ok_or("final certificate renewal facts are not an array")?
            .len(),
        before_renewal.len() * 3
    );
    assert_eq!(
        expiry_risk_fact_rows(&database, scenario.organization_id, scenario.node_id)
            .await?
            .as_array()
            .ok_or("final certificate expiry-risk facts are not an array")?
            .len(),
        before_renewal.len() * 3
    );
    assert!(!queue.publications.lock().await.is_empty());
    Ok(())
}

async fn renewal_fact_rows(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    node_id: NodeId,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(database
        .fetch_one_as(
            sql_query::<serde_json::Value>(
                "select coalesce(jsonb_agg(jsonb_build_object('eventKey', event_key, 'schemaVersion', schema_version, 'aggregateId', aggregate_id::text, 'aggregateVersion', aggregate_version, 'correlationId', correlation_id::text, 'payload', payload) order by aggregate_version, aggregate_id), '[]'::jsonb) from outbox_events where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and payload ->> 'node_id' = ")
            .bind(node_id.to_string())
            .append(" and event_key in ('edge.gateway-certificate.renewal-failed', 'edge.gateway-certificate.renewed')"),
        )
        .await?)
}

async fn expiry_risk_fact_rows(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    node_id: NodeId,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(database
        .fetch_one_as(
            sql_query::<serde_json::Value>(
                "select coalesce(jsonb_agg(jsonb_build_object('eventKey', event_key, 'schemaVersion', schema_version, 'aggregateId', aggregate_id::text, 'aggregateVersion', aggregate_version, 'correlationId', correlation_id::text, 'payload', payload) order by aggregate_version, aggregate_id), '[]'::jsonb) from outbox_events where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and payload ->> 'node_id' = ")
            .bind(node_id.to_string())
            .append(" and event_key in ('edge.gateway-certificate.expiry-at-risk', 'edge.gateway-certificate.expiry-risk-cleared')"),
        )
        .await?)
}

fn canonical_test_timestamp(value: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    value
        .with_nanosecond(value.nanosecond() / 1_000 * 1_000)
        .expect("canonical PostgreSQL test timestamp")
}

fn compiler() -> Result<GatewaySnapshotCompiler, String> {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
}

async fn pending_for(
    repository: &PostgresEdgeRepository,
    node_id: NodeId,
) -> Result<GatewayCertificateConvergenceResult, Box<dyn std::error::Error>> {
    repository
        .pending_gateway_certificate_convergences(100)
        .await?
        .into_iter()
        .find(|result| result.convergence.node_id == node_id)
        .ok_or_else(|| "Gateway certificate convergence was not pending".into())
}

async fn issue_certificate(
    repository: &PostgresEdgeRepository,
    certificate: &GatewayCertificate,
    issued_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    issue_certificate_until(
        repository,
        certificate,
        issued_at,
        issued_at + Duration::days(30),
    )
    .await
}

async fn issue_certificate_until(
    repository: &PostgresEdgeRepository,
    certificate: &GatewayCertificate,
    issued_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut issued = certificate.clone();
    let expected_version = issued.aggregate_version;
    issued.record_issued(
        format!("sha256:{}", "d".repeat(64)),
        GatewayCertificateMaterial {
            serial_number: issued.id.to_string(),
            fingerprint: format!("sha256:{}", "e".repeat(64)),
            certificate_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n"
                .into(),
            ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"
                .into(),
            issued_at,
            expires_at,
        },
        issued_at,
    )?;
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await?;
    Ok(())
}

fn acknowledgement(
    convergence: &GatewayCertificateConvergenceResult,
    state: GatewayAckState,
    acknowledged_at: chrono::DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: convergence.publication.command_id.as_uuid(),
        node_id: convergence.publication.node_id.as_uuid(),
        gateway_id: convergence.publication.node_id.as_uuid(),
        revision: convergence.publication.revision,
        snapshot_digest: convergence.publication.snapshot_digest.clone(),
        expires_at: convergence.publication.snapshot_expires_at,
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "reload rejected".into()),
        acknowledged_at,
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    }
}
