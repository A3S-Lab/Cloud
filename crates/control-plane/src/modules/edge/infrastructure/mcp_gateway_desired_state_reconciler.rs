use super::{
    CompileMcpGatewaySnapshot, GatewaySnapshotCompiler, GatewaySnapshotMetadata,
    IMcpGatewayProjectionSetPlanner, IMcpGatewaySnapshotRepository,
    McpGatewayNodeProjectionAssembler, McpGatewaySnapshotAnchor,
    McpGatewaySnapshotReconciliationState, PlanMcpGatewayProjectionSet, StageMcpGatewaySnapshot,
};
use crate::modules::edge::domain::repositories::MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY;
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateState, GatewayPublicationState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayScopeId, NodeCommandId, NodeId, RepositoryError,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewayDesiredStateReconciliationFailure {
    pub gateway_scope_id: GatewayScopeId,
    pub node_id: NodeId,
    pub operation: &'static str,
    pub error: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpGatewayDesiredStateReconciliationReport {
    pub scopes: usize,
    pub gateway_nodes: usize,
    pub pending_publications: usize,
    pub unchanged_snapshots: usize,
    pub retry_deferred: usize,
    pub staged_snapshots: usize,
    pub failures: Vec<McpGatewayDesiredStateReconciliationFailure>,
}

pub struct McpGatewayDesiredStateReconciler {
    repository: Arc<dyn IMcpGatewaySnapshotRepository>,
    planner: Arc<dyn IMcpGatewayProjectionSetPlanner>,
    node_assembler: McpGatewayNodeProjectionAssembler,
    compiler: GatewaySnapshotCompiler,
    interval: Duration,
    command_ttl: ChronoDuration,
    certificate_renewal_window: ChronoDuration,
    empty_snapshot_ttl: ChronoDuration,
    retry_delay: ChronoDuration,
    batch_size: usize,
    scope_cursor: Mutex<Option<GatewayScopeId>>,
}

impl McpGatewayDesiredStateReconciler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Arc<dyn IMcpGatewaySnapshotRepository>,
        planner: Arc<dyn IMcpGatewayProjectionSetPlanner>,
        compiler: GatewaySnapshotCompiler,
        interval: Duration,
        command_ttl: ChronoDuration,
        certificate_renewal_window: ChronoDuration,
        empty_snapshot_ttl: ChronoDuration,
        retry_delay: ChronoDuration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero()
            || command_ttl <= ChronoDuration::zero()
            || certificate_renewal_window <= ChronoDuration::zero()
            || certificate_renewal_window > ChronoDuration::days(30)
            || empty_snapshot_ttl <= command_ttl
            || empty_snapshot_ttl > ChronoDuration::days(7)
            || retry_delay <= ChronoDuration::zero()
            || retry_delay > ChronoDuration::days(1)
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err(
                "MCP Gateway desired-state reconciliation requires bounded timing and batch size"
                    .into(),
            );
        }
        Ok(Self {
            repository,
            planner,
            node_assembler: McpGatewayNodeProjectionAssembler::default(),
            compiler,
            interval,
            command_ttl,
            certificate_renewal_window,
            empty_snapshot_ttl,
            retry_delay,
            batch_size,
            scope_cursor: Mutex::new(None),
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<McpGatewayDesiredStateReconciliationReport, RepositoryError> {
        let now = canonical_timestamp(now);
        let certificate_renew_before = now
            .checked_add_signed(self.certificate_renewal_window)
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "MCP Gateway certificate renewal window exceeds supported time".into(),
                )
            })?;
        let after_gateway_scope_id = *self
            .scope_cursor
            .lock()
            .map_err(|_| RepositoryError::Storage("MCP Gateway scope cursor is poisoned".into()))?;
        let mut scopes = self
            .repository
            .mcp_gateway_reconciliation_scopes(now, after_gateway_scope_id, self.batch_size)
            .await?;
        if scopes.is_empty() && after_gateway_scope_id.is_some() {
            scopes = self
                .repository
                .mcp_gateway_reconciliation_scopes(now, None, self.batch_size)
                .await?;
        }
        if scopes
            .windows(2)
            .any(|scopes| scopes[0].scope.id >= scopes[1].scope.id)
            || after_gateway_scope_id.is_some_and(|cursor| {
                scopes.first().is_some_and(|scope| scope.scope.id <= cursor)
                    && scopes.last().is_some_and(|scope| scope.scope.id > cursor)
            })
        {
            return Err(RepositoryError::Storage(
                "MCP Gateway reconciliation scope scan is not cursor ordered".into(),
            ));
        }
        *self.scope_cursor.lock().map_err(|_| {
            RepositoryError::Storage("MCP Gateway scope cursor is poisoned".into())
        })? = scopes.last().map(|scope| scope.scope.id);
        let mut report = McpGatewayDesiredStateReconciliationReport {
            scopes: scopes.len(),
            ..McpGatewayDesiredStateReconciliationReport::default()
        };
        let mut nodes = BTreeMap::new();
        for reconciliation_scope in scopes {
            reconciliation_scope
                .validate()
                .map_err(RepositoryError::Storage)?;
            for node_id in reconciliation_scope.node_ids {
                nodes
                    .entry(node_id)
                    .or_insert(reconciliation_scope.scope.id);
            }
        }
        report.gateway_nodes = nodes.len();
        for (node_id, trigger_scope_id) in nodes {
            let state = match self
                .repository
                .mcp_gateway_snapshot_reconciliation_state(node_id)
                .await
            {
                Ok(state) => state,
                Err(_) => {
                    report.failures.push(failure(
                        trigger_scope_id,
                        node_id,
                        "read-state",
                        "MCP Gateway reconciliation state read failed",
                    ));
                    continue;
                }
            };
            if state.validate().is_err() {
                report.failures.push(failure(
                    trigger_scope_id,
                    node_id,
                    "validate-state",
                    "MCP Gateway reconciliation state is inconsistent",
                ));
                continue;
            }
            if state.pending_publication {
                report.pending_publications += 1;
                continue;
            }
            let certificate_requires_replacement = match self
                .repository
                .mcp_gateway_installed_certificate(node_id)
                .await
            {
                Ok(certificate) => {
                    match certificate_requires_replacement(
                        certificate.as_ref(),
                        certificate_renew_before,
                    ) {
                        Ok(requires_replacement) => requires_replacement,
                        Err(_) => {
                            report.failures.push(failure(
                                trigger_scope_id,
                                node_id,
                                "validate-certificate",
                                "installed MCP Gateway certificate is inconsistent",
                            ));
                            continue;
                        }
                    }
                }
                Err(_) => {
                    report.failures.push(failure(
                        trigger_scope_id,
                        node_id,
                        "read-certificate",
                        "installed MCP Gateway certificate read failed",
                    ));
                    continue;
                }
            };
            let active_scopes = match self
                .repository
                .mcp_gateway_active_scopes(node_id, now)
                .await
            {
                Ok(scopes) => scopes,
                Err(_) => {
                    report.failures.push(failure(
                        trigger_scope_id,
                        node_id,
                        "read-scopes",
                        "complete physical-node MCP scope read failed",
                    ));
                    continue;
                }
            };
            let anchor = match active_scopes.first() {
                Some(scope) => McpGatewaySnapshotAnchor::from_scope(scope),
                None => match &state.latest_mcp_snapshot {
                    Some(latest) => latest.anchor(),
                    None => {
                        report.unchanged_snapshots += 1;
                        continue;
                    }
                },
            };
            let mut planned_scopes = Vec::with_capacity(active_scopes.len());
            let mut planned_route_count = 0_usize;
            let mut planning_failed = false;
            for scope in active_scopes {
                match self
                    .planner
                    .plan(PlanMcpGatewayProjectionSet {
                        scope,
                        gateway_node_id: node_id,
                        observed_at: now,
                    })
                    .await
                {
                    Ok(planned) => {
                        planned_route_count = match planned_route_count
                            .checked_add(planned.observed_route_versions().len())
                        {
                            Some(count) if count <= MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY => count,
                            _ => {
                                report.failures.push(failure(
                                    trigger_scope_id,
                                    node_id,
                                    "plan",
                                    "physical Gateway exceeds the complete MCP route bound",
                                ));
                                planning_failed = true;
                                break;
                            }
                        };
                        planned_scopes.push(planned);
                    }
                    Err(_) => {
                        report.failures.push(failure(
                            trigger_scope_id,
                            node_id,
                            "plan",
                            "complete MCP Gateway desired-state planning failed",
                        ));
                        planning_failed = true;
                        break;
                    }
                }
            }
            if planning_failed {
                continue;
            }
            let planned = match self
                .node_assembler
                .assemble(anchor, node_id, now, planned_scopes)
            {
                Ok(planned) => planned,
                Err(_) => {
                    report.failures.push(failure(
                        trigger_scope_id,
                        node_id,
                        "assemble",
                        "physical-node MCP desired-state assembly failed",
                    ));
                    continue;
                }
            };
            let has_mcp_routes = planned.projection().is_some();
            if !has_mcp_routes {
                if let Some(decision) = empty_precompile_decision(&state, now, self.retry_delay) {
                    record_decision(&mut report, decision);
                    continue;
                }
            }
            let inputs = match self.repository.mcp_gateway_snapshot_inputs(node_id).await {
                Ok(inputs) => inputs,
                Err(_) => {
                    report.failures.push(failure(
                        trigger_scope_id,
                        node_id,
                        "read-inputs",
                        "complete Gateway snapshot input read failed",
                    ));
                    continue;
                }
            };
            if inputs.validate(node_id).is_err() {
                report.failures.push(failure(
                    trigger_scope_id,
                    node_id,
                    "validate-inputs",
                    "complete Gateway snapshot inputs are inconsistent",
                ));
                continue;
            }
            let installed_revision = inputs.physical_scope.installed_revision;
            let expires_at = match planned.projection() {
                Some(projection) => projection.projection().expires_at,
                None => match now.checked_add_signed(self.empty_snapshot_ttl) {
                    Some(expires_at) => expires_at,
                    None => {
                        report.failures.push(failure(
                            trigger_scope_id,
                            node_id,
                            "compile",
                            "empty Gateway snapshot expiry exceeds supported time",
                        ));
                        continue;
                    }
                },
            };
            let certificate_id = (!inputs.active_routes.is_empty() || has_mcp_routes)
                .then(crate::modules::shared_kernel::domain::GatewayCertificateId::new);
            let candidate =
                match self
                    .compiler
                    .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
                        metadata: GatewaySnapshotMetadata::new(
                            node_id,
                            match inputs.physical_scope.next_revision() {
                                Ok(revision) => revision,
                                Err(_) => {
                                    report.failures.push(failure(
                                        trigger_scope_id,
                                        node_id,
                                        "compile",
                                        "physical Gateway revision is exhausted",
                                    ));
                                    continue;
                                }
                            },
                            inputs.physical_scope.installed_revision,
                            now,
                            expires_at,
                        ),
                        physical_scope: inputs.physical_scope.clone(),
                        certificate_id,
                        active_routes: inputs.active_routes,
                        mcp: planned,
                    }) {
                    Ok(candidate) => candidate,
                    Err(_) => {
                        report.failures.push(failure(
                            trigger_scope_id,
                            node_id,
                            "compile",
                            "complete MCP Gateway snapshot compilation failed",
                        ));
                        continue;
                    }
                };
            let decision = reconciliation_decision(
                &state,
                candidate.desired_state_digest(),
                has_mcp_routes,
                installed_revision,
                certificate_requires_replacement,
                now,
                self.retry_delay,
            );
            if decision != ReconciliationDecision::Stage {
                record_decision(&mut report, decision);
                continue;
            }
            let desired_command_deadline = match now.checked_add_signed(self.command_ttl) {
                Some(deadline) => deadline,
                None => {
                    report.failures.push(failure(
                        trigger_scope_id,
                        node_id,
                        "stage",
                        "MCP Gateway command deadline exceeds supported time",
                    ));
                    continue;
                }
            };
            let command_not_after = desired_command_deadline.min(expires_at);
            let stage = match StageMcpGatewaySnapshot::new(
                candidate,
                NodeCommandId::new(),
                Uuid::now_v7(),
                command_not_after,
            ) {
                Ok(stage) => stage,
                Err(_) => {
                    report.failures.push(failure(
                        trigger_scope_id,
                        node_id,
                        "stage",
                        "MCP Gateway publication bundle is invalid",
                    ));
                    continue;
                }
            };
            match self.repository.stage_mcp_gateway_snapshot(stage).await {
                Ok(_) => report.staged_snapshots += 1,
                Err(_) => report.failures.push(failure(
                    trigger_scope_id,
                    node_id,
                    "persist",
                    "MCP Gateway publication staging failed",
                )),
            }
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
                                    gateway_scope_id = %failure.gateway_scope_id,
                                    gateway_node_id = %failure.node_id,
                                    operation = failure.operation,
                                    error = failure.error,
                                    "MCP Gateway desired-state reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "MCP Gateway desired-state reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }
}

fn empty_precompile_decision(
    state: &McpGatewaySnapshotReconciliationState,
    now: DateTime<Utc>,
    retry_delay: ChronoDuration,
) -> Option<ReconciliationDecision> {
    if state.pending_publication {
        return Some(ReconciliationDecision::Pending);
    }
    match &state.latest_mcp_snapshot {
        None => Some(ReconciliationDecision::Unchanged),
        Some(latest) if latest.mcp_route_count > 0 => None,
        Some(latest) => Some(match latest.publication.state {
            GatewayPublicationState::Pending => ReconciliationDecision::Pending,
            GatewayPublicationState::Applied => ReconciliationDecision::Unchanged,
            GatewayPublicationState::Rejected | GatewayPublicationState::Unavailable => {
                retry_decision(latest, now, retry_delay)
            }
        }),
    }
}

fn record_decision(
    report: &mut McpGatewayDesiredStateReconciliationReport,
    decision: ReconciliationDecision,
) {
    match decision {
        ReconciliationDecision::Pending => report.pending_publications += 1,
        ReconciliationDecision::Unchanged => report.unchanged_snapshots += 1,
        ReconciliationDecision::RetryDeferred => report.retry_deferred += 1,
        ReconciliationDecision::Stage => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReconciliationDecision {
    Stage,
    Pending,
    Unchanged,
    RetryDeferred,
}

pub(super) fn reconciliation_decision(
    state: &McpGatewaySnapshotReconciliationState,
    desired_state_digest: &crate::modules::shared_kernel::domain::Sha256Digest,
    has_mcp_routes: bool,
    installed_revision: Option<u64>,
    certificate_requires_replacement: bool,
    now: DateTime<Utc>,
    retry_delay: ChronoDuration,
) -> ReconciliationDecision {
    if state.pending_publication {
        return ReconciliationDecision::Pending;
    }
    let Some(latest) = &state.latest_mcp_snapshot else {
        return if has_mcp_routes {
            ReconciliationDecision::Stage
        } else {
            ReconciliationDecision::Unchanged
        };
    };
    if has_mcp_routes != (latest.mcp_route_count > 0) {
        return ReconciliationDecision::Stage;
    }
    if !has_mcp_routes {
        return match latest.publication.state {
            GatewayPublicationState::Pending => ReconciliationDecision::Pending,
            GatewayPublicationState::Applied => ReconciliationDecision::Unchanged,
            GatewayPublicationState::Rejected | GatewayPublicationState::Unavailable => {
                retry_decision(latest, now, retry_delay)
            }
        };
    }
    if &latest.desired_state_digest != desired_state_digest {
        return ReconciliationDecision::Stage;
    }
    if has_mcp_routes && certificate_requires_replacement {
        return ReconciliationDecision::Stage;
    }
    match latest.publication.state {
        GatewayPublicationState::Pending => ReconciliationDecision::Pending,
        GatewayPublicationState::Applied => {
            if has_mcp_routes && installed_revision != Some(latest.publication.revision) {
                ReconciliationDecision::Stage
            } else {
                ReconciliationDecision::Unchanged
            }
        }
        GatewayPublicationState::Rejected | GatewayPublicationState::Unavailable => {
            retry_decision(latest, now, retry_delay)
        }
    }
}

fn certificate_requires_replacement(
    certificate: Option<&GatewayCertificate>,
    renew_before: DateTime<Utc>,
) -> Result<bool, String> {
    let Some(certificate) = certificate else {
        return Ok(false);
    };
    match certificate.state {
        GatewayCertificateState::Revoked => Ok(true),
        GatewayCertificateState::Ready => certificate
            .material
            .as_ref()
            .map(|material| material.expires_at <= renew_before)
            .ok_or_else(|| "ready MCP Gateway certificate omitted material".to_string()),
        GatewayCertificateState::Provisioning
        | GatewayCertificateState::Issued
        | GatewayCertificateState::Failed => {
            Err("installed MCP Gateway certificate is not ready".into())
        }
    }
}

fn retry_decision(
    latest: &crate::modules::edge::infrastructure::McpGatewaySnapshotStatus,
    now: DateTime<Utc>,
    retry_delay: ChronoDuration,
) -> ReconciliationDecision {
    let retry_at = latest
        .publication
        .acknowledged_at
        .and_then(|acknowledged_at| acknowledged_at.checked_add_signed(retry_delay));
    if retry_at.is_some_and(|retry_at| retry_at <= now) {
        ReconciliationDecision::Stage
    } else {
        ReconciliationDecision::RetryDeferred
    }
}

fn failure(
    gateway_scope_id: GatewayScopeId,
    node_id: NodeId,
    operation: &'static str,
    error: &'static str,
) -> McpGatewayDesiredStateReconciliationFailure {
    McpGatewayDesiredStateReconciliationFailure {
        gateway_scope_id,
        node_id,
        operation,
        error,
    }
}
