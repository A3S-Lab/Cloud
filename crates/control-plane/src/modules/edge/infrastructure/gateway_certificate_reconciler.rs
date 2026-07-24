use super::GatewaySnapshotMetadata;
use crate::modules::edge::domain::events::GatewayCertificateConvergenceStaged;
use crate::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget, IEdgeRepository,
    StageGatewayCertificateConvergence,
};
use crate::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, IGatewayCertificateAuthority, IGatewayCommandQueue,
};
use crate::modules::edge::domain::{
    DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayCertificateConvergenceReason, GatewayCertificateState, GatewayPublication,
    GatewayRouteVersion,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, NodeCommandId, NodeId, RepositoryError,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const OBSOLETE_CERTIFICATE_REASON: &str = "superseded by installed Gateway certificate";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayCertificateReconciliationFailure {
    pub node_id: NodeId,
    pub certificate_id: GatewayCertificateId,
    pub operation: &'static str,
    pub error: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayCertificateReconciliationReport {
    pub pending_convergences: usize,
    pub convergence_targets: usize,
    pub staged_convergences: usize,
    pub dispatched_commands: usize,
    pub replayed_commands: usize,
    pub obsolete_certificates: usize,
    pub revoked_certificates: usize,
    pub failures: Vec<GatewayCertificateReconciliationFailure>,
}

pub struct GatewayCertificateReconciler {
    repository: Arc<dyn IEdgeRepository>,
    commands: Arc<dyn IGatewayCommandQueue>,
    certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
    compiler: super::GatewaySnapshotCompiler,
    interval: Duration,
    renewal_window: ChronoDuration,
    command_ttl: ChronoDuration,
    batch_size: usize,
}

impl GatewayCertificateReconciler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Arc<dyn IEdgeRepository>,
        commands: Arc<dyn IGatewayCommandQueue>,
        certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
        compiler: super::GatewaySnapshotCompiler,
        interval: Duration,
        renewal_window: ChronoDuration,
        command_ttl: ChronoDuration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero()
            || renewal_window <= ChronoDuration::zero()
            || command_ttl <= ChronoDuration::zero()
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err(
                "Gateway certificate reconciliation requires positive bounded timing and batch size"
                    .into(),
            );
        }
        Ok(Self {
            repository,
            commands,
            certificate_authority,
            compiler,
            interval,
            renewal_window,
            command_ttl,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<GatewayCertificateReconciliationReport, RepositoryError> {
        let now = canonical_timestamp(now);
        let renew_before = now.checked_add_signed(self.renewal_window).ok_or_else(|| {
            RepositoryError::Conflict(
                "Gateway certificate renewal window exceeds supported time".into(),
            )
        })?;
        let mut report = GatewayCertificateReconciliationReport::default();

        let pending = self
            .repository
            .pending_gateway_certificate_convergences(self.batch_size)
            .await?;
        report.pending_convergences = pending.len();
        for convergence in pending {
            self.dispatch(&convergence, &mut report).await;
        }

        let targets = self
            .repository
            .gateway_certificate_convergence_targets(renew_before, self.batch_size)
            .await?;
        report.convergence_targets = targets.len();
        for target in targets {
            let node_id = target.scope.node_id;
            let certificate_id = target.certificate.id;
            let bundle = match self.compile_convergence(target, now, renew_before) {
                Ok(bundle) => bundle,
                Err(_) => {
                    report.failures.push(failure(
                        node_id,
                        certificate_id,
                        "compile",
                        "Gateway certificate convergence compilation failed",
                    ));
                    continue;
                }
            };
            let staged = match self
                .repository
                .stage_gateway_certificate_convergence(bundle)
                .await
            {
                Ok(staged) => staged,
                Err(_) => {
                    report.failures.push(failure(
                        node_id,
                        certificate_id,
                        "stage",
                        "Gateway certificate convergence staging failed",
                    ));
                    continue;
                }
            };
            report.staged_convergences += 1;
            self.dispatch(&staged, &mut report).await;
        }

        let obsolete = self
            .repository
            .obsolete_gateway_certificates(self.batch_size)
            .await?;
        report.obsolete_certificates = obsolete.len();
        for certificate in obsolete {
            self.revoke_obsolete(certificate, now, &mut report).await;
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    gateway_node_id = %failure.node_id,
                                    gateway_certificate_id = %failure.certificate_id,
                                    operation = failure.operation,
                                    error = failure.error,
                                    "Gateway certificate reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Gateway certificate reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }

    async fn dispatch(
        &self,
        convergence: &GatewayCertificateConvergenceResult,
        report: &mut GatewayCertificateReconciliationReport,
    ) {
        match self.commands.enqueue(&convergence.publication).await {
            Ok(dispatch) => {
                report.dispatched_commands += 1;
                report.replayed_commands += usize::from(dispatch.replayed);
            }
            Err(_) => report.failures.push(failure(
                convergence.convergence.node_id,
                convergence
                    .convergence
                    .replacement_certificate_id
                    .unwrap_or(convergence.convergence.previous_certificate_id),
                "dispatch",
                "Gateway command dispatch failed",
            )),
        }
    }

    fn compile_convergence(
        &self,
        target: GatewayCertificateConvergenceTarget,
        now: DateTime<Utc>,
        renew_before: DateTime<Utc>,
    ) -> Result<StageGatewayCertificateConvergence, RepositoryError> {
        target.validate().map_err(RepositoryError::Storage)?;
        if target.certificate.updated_at > now
            || target
                .routes
                .iter()
                .any(|status| status.route.updated_at > now)
        {
            return Err(RepositoryError::Conflict(
                "Gateway certificate reconciliation time predates its target".into(),
            ));
        }
        let revision = target
            .scope
            .next_revision()
            .map_err(RepositoryError::Conflict)?;
        let command_id = deterministic_command_id(target.scope.node_id, revision);
        let mut retained_routes = Vec::new();
        let mut retained_versions = Vec::new();
        let mut rejected_versions = Vec::new();
        for status in &target.routes {
            let version = GatewayRouteVersion::new(status.route.id, status.route.aggregate_version)
                .map_err(RepositoryError::Conflict)?;
            if status.domain_claim_state == DomainClaimState::Verified {
                retained_routes.push(status.route.clone());
                retained_versions.push(version);
            } else {
                rejected_versions.push(version);
            }
        }
        let reason = convergence_reason(&target, renew_before, !rejected_versions.is_empty())?;
        let replacement_certificate_id = (!retained_routes.is_empty())
            .then(|| deterministic_certificate_id(target.scope.node_id, revision));
        let command_not_after = now.checked_add_signed(self.command_ttl).ok_or_else(|| {
            RepositoryError::Conflict(
                "Gateway certificate convergence command expiry exceeds supported time".into(),
            )
        })?;
        let snapshot_expires_at = now
            .checked_add_signed(ChronoDuration::hours(24))
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "Gateway certificate convergence snapshot expiry exceeds supported time".into(),
                )
            })?;
        let snapshot = self
            .compiler
            .compile_certificate_convergence(
                GatewaySnapshotMetadata::new(
                    target.scope.node_id,
                    revision,
                    target.scope.installed_revision,
                    now,
                    snapshot_expires_at,
                ),
                replacement_certificate_id,
                &retained_routes,
            )
            .map_err(RepositoryError::Conflict)?;
        let publication = GatewayPublication::stage(
            target.scope.node_id,
            command_id,
            command_id.as_uuid(),
            snapshot,
            now,
            command_not_after,
        )
        .map_err(RepositoryError::Conflict)?;
        let certificate = match (
            replacement_certificate_id,
            publication.certificate_request.clone(),
        ) {
            (Some(certificate_id), Some(request)) => {
                let mut domain_claim_ids = retained_routes
                    .iter()
                    .filter_map(|route| route.domain_claim_id)
                    .collect::<Vec<_>>();
                domain_claim_ids.sort();
                domain_claim_ids.dedup();
                Some(
                    GatewayCertificate::provision(
                        certificate_id,
                        target.certificate.organization_id,
                        target.scope.node_id,
                        domain_claim_ids,
                        revision,
                        command_id,
                        publication.snapshot_digest.clone(),
                        request,
                        now,
                    )
                    .map_err(RepositoryError::Conflict)?,
                )
            }
            (None, None) => None,
            _ => {
                return Err(RepositoryError::Storage(
                    "Gateway convergence certificate request is inconsistent".into(),
                ))
            }
        };
        let convergence = GatewayCertificateConvergence::stage(
            target.certificate.organization_id,
            target.scope.node_id,
            revision,
            command_id,
            target.certificate.id,
            replacement_certificate_id,
            publication.snapshot_digest.clone(),
            retained_versions,
            rejected_versions,
            reason,
            now,
        )
        .map_err(RepositoryError::Conflict)?;
        let event = GatewayCertificateConvergenceStaged::envelope(&convergence, &publication)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        Ok(StageGatewayCertificateConvergence {
            convergence,
            certificate,
            publication,
            expected_scope_version: target.scope.aggregate_version,
            event,
        })
    }

    async fn revoke_obsolete(
        &self,
        certificate: GatewayCertificate,
        now: DateTime<Utc>,
        report: &mut GatewayCertificateReconciliationReport,
    ) {
        let mut revoked = certificate.clone();
        let expected_version = revoked.aggregate_version;
        if revoked.revoke(OBSOLETE_CERTIFICATE_REASON, now).is_err() {
            report.failures.push(failure(
                certificate.node_id,
                certificate.id,
                "project-revocation",
                "Gateway certificate revocation projection failed",
            ));
            return;
        }
        if let Err(error) = self.certificate_authority.revoke(&certificate).await {
            report.failures.push(failure(
                certificate.node_id,
                certificate.id,
                "provider-revocation",
                authority_failure(&error),
            ));
            return;
        }
        match self
            .repository
            .transition_gateway_certificate(revoked, expected_version)
            .await
        {
            Ok(_) => report.revoked_certificates += 1,
            Err(_) => report.failures.push(failure(
                certificate.node_id,
                certificate.id,
                "persist-revocation",
                "Gateway certificate revocation projection failed",
            )),
        }
    }
}

fn convergence_reason(
    target: &GatewayCertificateConvergenceTarget,
    renew_before: DateTime<Utc>,
    has_rejected_routes: bool,
) -> Result<GatewayCertificateConvergenceReason, RepositoryError> {
    if has_rejected_routes {
        return Ok(GatewayCertificateConvergenceReason::DomainRevocation);
    }
    if target.certificate.state == GatewayCertificateState::Revoked {
        return Ok(GatewayCertificateConvergenceReason::CertificateRevocation);
    }
    let expires_at = target
        .certificate
        .material
        .as_ref()
        .map(|material| material.expires_at)
        .ok_or_else(|| {
            RepositoryError::Storage("installed Gateway certificate has no material".into())
        })?;
    if expires_at <= renew_before {
        Ok(GatewayCertificateConvergenceReason::Renewal)
    } else {
        Ok(GatewayCertificateConvergenceReason::ProjectionRepair)
    }
}

pub(super) fn deterministic_command_id(node_id: NodeId, revision: u64) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &node_id.as_uuid(),
        format!("a3s-cloud:gateway-certificate-convergence:{revision}:command").as_bytes(),
    ))
}

pub(super) fn deterministic_certificate_id(node_id: NodeId, revision: u64) -> GatewayCertificateId {
    GatewayCertificateId::from_uuid(Uuid::new_v5(
        &node_id.as_uuid(),
        format!("a3s-cloud:gateway-certificate-convergence:{revision}:certificate").as_bytes(),
    ))
}

const fn authority_failure(error: &GatewayCertificateAuthorityError) -> &'static str {
    match error {
        GatewayCertificateAuthorityError::InvalidRequest(_) => {
            "Gateway certificate revocation request is invalid"
        }
        GatewayCertificateAuthorityError::Rejected(_) => {
            "Gateway certificate revocation was rejected"
        }
        GatewayCertificateAuthorityError::Unavailable(_) => {
            "Gateway certificate authority is unavailable"
        }
    }
}

const fn failure(
    node_id: NodeId,
    certificate_id: GatewayCertificateId,
    operation: &'static str,
    error: &'static str,
) -> GatewayCertificateReconciliationFailure {
    GatewayCertificateReconciliationFailure {
        node_id,
        certificate_id,
        operation,
        error,
    }
}
