use crate::modules::edge::domain::events::GatewayRolloutStaged;
use crate::modules::edge::domain::repositories::StageGatewayRolloutRollback;
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateState, GatewayPublication, GatewayReplicaRecoveryState,
    GatewayReplicaRolloutState, GatewayRollout, GatewayRolloutRollback,
    GatewayRolloutRollbackState, GatewayRolloutState, GatewayScope, GatewayScopeState, Route,
    RouteState,
};
use crate::modules::edge::infrastructure::{GatewaySnapshotCompiler, GatewaySnapshotMetadata};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, GatewayCertificateId, NodeCommandId, NodeId,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const COMMAND_ID_NAME: &[u8] = b"a3s-cloud.gateway-rollout.rollback.command.v1";
const CERTIFICATE_ID_NAME: &[u8] = b"a3s-cloud.gateway-rollout.rollback.certificate.v1";

#[derive(Debug, Clone)]
pub struct GatewayRollbackMemberSnapshotContext {
    pub scope: GatewayScopeState,
    pub active_routes: Vec<Route>,
    pub reusable_certificate: Option<GatewayCertificate>,
}

#[derive(Debug, Clone)]
pub struct CompileGatewayRolloutRollback {
    pub scope: GatewayScope,
    pub failed_rollout: GatewayRollout,
    pub rollback: GatewayRolloutRollback,
    pub member_contexts: Vec<GatewayRollbackMemberSnapshotContext>,
    pub issued_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CompiledGatewayRolloutRollback {
    pub scope: GatewayScope,
    pub failed_rollout: GatewayRollout,
    pub rollback: GatewayRolloutRollback,
    pub rollout: GatewayRollout,
    pub publications: Vec<GatewayPublication>,
    pub certificates: Vec<GatewayCertificate>,
    pub reused_certificates: Vec<GatewayCertificate>,
    pub expected_scope_versions: BTreeMap<NodeId, u64>,
}

impl CompiledGatewayRolloutRollback {
    pub fn stage_bundle(&self) -> Result<StageGatewayRolloutRollback, String> {
        let bundle = StageGatewayRolloutRollback {
            scope: self.scope.clone(),
            failed_rollout: self.failed_rollout.clone(),
            rollback: self.rollback.clone(),
            rollout: self.rollout.clone(),
            publications: self.publications.clone(),
            certificates: self.certificates.clone(),
            reused_certificates: self.reused_certificates.clone(),
            expected_scope_versions: self.expected_scope_versions.clone(),
            expected_rollback_version: self
                .rollback
                .aggregate_version
                .checked_sub(1)
                .ok_or_else(|| "Gateway rollback stage version is invalid".to_string())?,
            event: GatewayRolloutStaged::envelope(&self.scope, &self.rollout)
                .map_err(|error| error.to_string())?,
        };
        bundle.validate()?;
        Ok(bundle)
    }
}

#[derive(Debug, Clone)]
pub struct GatewayRolloutRollbackCompiler {
    snapshots: GatewaySnapshotCompiler,
    command_ttl: Duration,
    snapshot_ttl: Duration,
}

impl GatewayRolloutRollbackCompiler {
    pub fn new(
        snapshots: GatewaySnapshotCompiler,
        command_ttl: Duration,
        snapshot_ttl: Duration,
    ) -> Result<Self, String> {
        if command_ttl <= Duration::zero()
            || snapshot_ttl < command_ttl
            || snapshot_ttl > Duration::days(30)
        {
            return Err(
                "Gateway rollback validity requires a positive command TTL and a bounded snapshot TTL"
                    .into(),
            );
        }
        Ok(Self {
            snapshots,
            command_ttl,
            snapshot_ttl,
        })
    }

    pub fn compile(
        &self,
        request: CompileGatewayRolloutRollback,
    ) -> Result<CompiledGatewayRolloutRollback, String> {
        request.scope.validate()?;
        request.failed_rollout.validate()?;
        request.rollback.validate()?;
        validate_source(&request.scope, &request.failed_rollout, &request.rollback)?;
        let desired_nodes = request
            .scope
            .member_node_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let contexts = canonical_contexts(request.member_contexts, &desired_nodes)?;
        let issued_at = canonical_timestamp(request.issued_at);
        if issued_at < request.rollback.required_at {
            return Err("Gateway rollback issue time predates its durable requirement".into());
        }
        let command_not_after = issued_at
            .checked_add_signed(self.command_ttl)
            .ok_or_else(|| "Gateway rollback command expiry exceeds supported time".to_string())?;
        let snapshot_expires_at = issued_at
            .checked_add_signed(self.snapshot_ttl)
            .ok_or_else(|| "Gateway rollback snapshot expiry exceeds supported time".to_string())?;

        let mut publications = Vec::with_capacity(contexts.len());
        let mut certificates = Vec::new();
        let mut reused_certificates = Vec::new();
        let mut expected_scope_versions = BTreeMap::new();
        for context in contexts {
            validate_member_context(&request.scope, &request.failed_rollout, &context)?;
            let node_id = context.scope.node_id;
            let revision = context.scope.next_revision()?;
            let (certificate_id, reusable) = self.select_certificate(
                &request.rollback,
                request.scope.organization_id,
                &context,
                issued_at,
            )?;
            let metadata = GatewaySnapshotMetadata::new(
                node_id,
                revision,
                context.scope.installed_revision,
                issued_at,
                snapshot_expires_at,
            );
            let snapshot = match &reusable {
                Some(certificate) => self.snapshots.compile_certificate_reuse(
                    metadata,
                    certificate.request.clone(),
                    &context.active_routes,
                )?,
                None => self.snapshots.compile_certificate_convergence(
                    metadata,
                    certificate_id,
                    &context.active_routes,
                )?,
            };
            let command_id = deterministic_member_id(
                request.rollback.rollback_rollout_id.as_uuid(),
                node_id,
                COMMAND_ID_NAME,
            );
            let publication = GatewayPublication::stage(
                node_id,
                NodeCommandId::from_uuid(command_id),
                request.rollback.rollback_rollout_id.as_uuid(),
                snapshot,
                issued_at,
                command_not_after,
            )?;
            if let Some(reusable) = reusable {
                validate_reusable_certificate(
                    &reusable,
                    &publication,
                    &context.active_routes,
                    issued_at,
                )?;
                reused_certificates.push(reusable);
            } else if let Some(certificate_id) = certificate_id {
                let certificate_request =
                    publication.certificate_request.clone().ok_or_else(|| {
                        "Gateway rollback TLS snapshot omitted its certificate request".to_string()
                    })?;
                let certificate = GatewayCertificate::provision(
                    certificate_id,
                    request.scope.organization_id,
                    node_id,
                    domain_claim_ids(&context.active_routes)?,
                    revision,
                    NodeCommandId::from_uuid(command_id),
                    publication.snapshot_digest.clone(),
                    certificate_request,
                    issued_at,
                )?;
                certificates.push(certificate);
            }
            expected_scope_versions.insert(node_id, context.scope.aggregate_version);
            publications.push(publication);
        }

        publications.sort_by_key(|publication| publication.node_id);
        certificates.sort_by_key(|certificate| certificate.node_id);
        reused_certificates.sort_by_key(|certificate| certificate.node_id);
        let rollout = GatewayRollout::stage_rollback(
            request.rollback.rollback_rollout_id,
            &request.scope,
            request.rollback.rollback_generation,
            &publications,
            issued_at,
        )?;
        let mut rollback = request.rollback;
        rollback.stage(&rollout)?;
        Ok(CompiledGatewayRolloutRollback {
            scope: request.scope,
            failed_rollout: request.failed_rollout,
            rollback,
            rollout,
            publications,
            certificates,
            reused_certificates,
            expected_scope_versions,
        })
    }

    fn select_certificate(
        &self,
        rollback: &GatewayRolloutRollback,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        context: &GatewayRollbackMemberSnapshotContext,
        issued_at: DateTime<Utc>,
    ) -> Result<(Option<GatewayCertificateId>, Option<GatewayCertificate>), String> {
        if context.active_routes.is_empty() {
            return Ok((None, None));
        }
        let route_certificate_ids = context
            .active_routes
            .iter()
            .map(|route| {
                route.gateway_certificate_id.ok_or_else(|| {
                    "active Gateway rollback Route omitted its certificate".to_string()
                })
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if route_certificate_ids.len() == 1 {
            let certificate_id = *route_certificate_ids
                .first()
                .ok_or_else(|| "active Gateway rollback certificate set is empty".to_string())?;
            if let Some(certificate) = context
                .reusable_certificate
                .as_ref()
                .filter(|certificate| certificate.id == certificate_id)
            {
                let retained_claims = domain_claim_ids(&context.active_routes)?
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                let stored_claims = certificate
                    .domain_claim_ids
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>();
                let reuse_probe = self.snapshots.compile_certificate_reuse(
                    GatewaySnapshotMetadata::new(
                        context.scope.node_id,
                        context.scope.next_revision()?,
                        context.scope.installed_revision,
                        issued_at,
                        issued_at + self.snapshot_ttl,
                    ),
                    certificate.request.clone(),
                    &context.active_routes,
                );
                if certificate.state == GatewayCertificateState::Ready
                    && certificate.organization_id == organization_id
                    && certificate.node_id == context.scope.node_id
                    && certificate.request.certificate_id == certificate_id.as_uuid()
                    && retained_claims.is_subset(&stored_claims)
                    && reuse_probe.is_ok()
                    && certificate.material.as_ref().is_some_and(|material| {
                        material.validate().is_ok()
                            && material.issued_at <= issued_at
                            && material.expires_at > issued_at
                    })
                {
                    return Ok((Some(certificate_id), Some(certificate.clone())));
                }
            }
        }
        Ok((
            Some(GatewayCertificateId::from_uuid(deterministic_member_id(
                rollback.rollback_rollout_id.as_uuid(),
                context.scope.node_id,
                CERTIFICATE_ID_NAME,
            ))),
            None,
        ))
    }
}

fn validate_source(
    scope: &GatewayScope,
    failed: &GatewayRollout,
    rollback: &GatewayRolloutRollback,
) -> Result<(), String> {
    if failed.id != rollback.failed_rollout_id
        || failed.gateway_scope_id != scope.id
        || failed.gateway_scope_id != rollback.gateway_scope_id
        || failed.membership_generation != scope.membership_generation
        || failed.membership_generation != rollback.membership_generation
        || failed.generation != rollback.failed_generation
        || failed.state != GatewayRolloutState::Degraded
        || failed.serves_traffic()?
        || rollback.state != GatewayRolloutRollbackState::Required
        || rollback.aggregate_version == 0
        || failed.replicas.iter().any(|replica| match replica.state {
            GatewayReplicaRolloutState::Pending => true,
            GatewayReplicaRolloutState::Applied | GatewayReplicaRolloutState::Rejected => false,
            GatewayReplicaRolloutState::Unavailable => replica
                .recovery
                .as_ref()
                .is_none_or(|recovery| recovery.state != GatewayReplicaRecoveryState::Observed),
        })
    {
        return Err("Gateway rollback source is incomplete or still physically ambiguous".into());
    }
    let source_nodes = failed
        .replicas
        .iter()
        .map(|replica| replica.node_id)
        .collect::<BTreeSet<_>>();
    let desired_nodes = scope
        .member_node_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if source_nodes != desired_nodes {
        return Err("Gateway rollback source does not cover exact scope membership".into());
    }
    Ok(())
}

fn validate_member_context(
    scope: &GatewayScope,
    failed: &GatewayRollout,
    context: &GatewayRollbackMemberSnapshotContext,
) -> Result<(), String> {
    let replica = failed
        .replicas
        .iter()
        .find(|replica| replica.node_id == context.scope.node_id)
        .ok_or_else(|| "Gateway rollback context has no failed member".to_string())?;
    let observed_revision = match replica.state {
        GatewayReplicaRolloutState::Applied => Some(replica.revision),
        GatewayReplicaRolloutState::Rejected => context.scope.installed_revision,
        GatewayReplicaRolloutState::Unavailable => replica
            .recovery
            .as_ref()
            .and_then(|recovery| recovery.observation.as_ref())
            .and_then(|observation| observation.applied.as_ref())
            .map(|applied| applied.revision),
        GatewayReplicaRolloutState::Pending => {
            return Err("Gateway rollback member remains pending".into())
        }
    };
    if context.scope.last_issued_revision != replica.revision
        || context.scope.installed_revision != observed_revision
        || context.active_routes.iter().any(|route| {
            route.organization_id != scope.organization_id
                || route.project_id != scope.project_id
                || route.environment_id != scope.environment_id
                || route.gateway_scope_id != scope.id
                || route.gateway_node_id != context.scope.node_id
                || route.state != RouteState::Active
                || route.failure.is_some()
                || route.validate_target_binding().is_err()
        })
    {
        return Err(
            "Gateway rollback member context does not match exact observed physical state".into(),
        );
    }
    Ok(())
}

fn canonical_contexts(
    mut contexts: Vec<GatewayRollbackMemberSnapshotContext>,
    desired_nodes: &BTreeSet<NodeId>,
) -> Result<Vec<GatewayRollbackMemberSnapshotContext>, String> {
    contexts.sort_by_key(|context| context.scope.node_id);
    if contexts.len() != desired_nodes.len()
        || contexts
            .windows(2)
            .any(|contexts| contexts[0].scope.node_id == contexts[1].scope.node_id)
        || contexts
            .iter()
            .map(|context| context.scope.node_id)
            .collect::<BTreeSet<_>>()
            != *desired_nodes
    {
        return Err(
            "Gateway rollback contexts must cover every desired member exactly once".into(),
        );
    }
    Ok(contexts)
}

fn validate_reusable_certificate(
    certificate: &GatewayCertificate,
    publication: &GatewayPublication,
    routes: &[Route],
    issued_at: DateTime<Utc>,
) -> Result<(), String> {
    let request = publication
        .certificate_request
        .as_ref()
        .ok_or_else(|| "reused Gateway certificate publication omitted its request".to_string())?;
    let retained_claims = domain_claim_ids(routes)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let stored_claims = certificate
        .domain_claim_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let request_names = request
        .dns_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if certificate.id.as_uuid() != request.certificate_id
        || certificate.node_id != publication.node_id
        || certificate.state != GatewayCertificateState::Ready
        || &certificate.request != request
        || !retained_claims.is_subset(&stored_claims)
        || routes.iter().any(|route| {
            route.organization_id != certificate.organization_id
                || route
                    .domain_pattern
                    .as_ref()
                    .is_none_or(|pattern| !request_names.contains(pattern.as_str()))
        })
        || certificate.material.as_ref().is_none_or(|material| {
            material.validate().is_err()
                || material.issued_at > issued_at
                || material.expires_at <= issued_at
        })
    {
        return Err(
            "reused Gateway rollback certificate is not valid for its exact snapshot".into(),
        );
    }
    Ok(())
}

fn domain_claim_ids(routes: &[Route]) -> Result<Vec<DomainClaimId>, String> {
    let mut ids = routes
        .iter()
        .map(|route| {
            route
                .domain_claim_id
                .ok_or_else(|| "active Gateway rollback Route omitted its DomainClaim".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err("TLS Gateway rollback requires at least one DomainClaim".into());
    }
    Ok(ids)
}

fn deterministic_member_id(rollout_id: Uuid, node_id: NodeId, kind: &[u8]) -> Uuid {
    let mut name = Vec::with_capacity(kind.len() + 16);
    name.extend_from_slice(kind);
    name.extend_from_slice(node_id.as_uuid().as_bytes());
    Uuid::new_v5(&rollout_id, &name)
}
