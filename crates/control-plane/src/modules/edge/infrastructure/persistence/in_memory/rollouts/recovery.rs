use super::{
    super::State,
    queries::{certificate_binding, find},
    validate_batch_limit,
};
use crate::modules::edge::domain::repositories::GatewayReplicaRecoveryTarget;
use crate::modules::edge::domain::{
    GatewayPublication, GatewayPublicationState, GatewayReplicaRecoveryState, GatewayRollout,
    GatewayRolloutRollback, GatewayRolloutRollbackState, GatewayRolloutState, Route,
};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, NodeCommandId, NodeId, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::NodeGatewaySnapshotObservation;
use chrono::{DateTime, Utc};

pub(in super::super) fn mark_unavailable(
    state: &mut State,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    let mut next = state.clone();
    let mut rollout = find(&next, organization_id, rollout_id)?;
    if rollout.aggregate_version != expected_version {
        return Err(RepositoryError::Conflict(
            "Gateway rollout changed before unavailability was recorded".into(),
        ));
    }
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Conflict("Gateway rollout does not contain this member".into())
        })?;
    if next.rollout_publications.get(&(node_id, replica.revision)) != Some(&rollout_id) {
        return Err(RepositoryError::Storage(
            "Gateway rollout publication ownership is inconsistent".into(),
        ));
    }
    let mut publication = next
        .publications
        .get(&(node_id, replica.revision))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("Gateway rollout unavailable publication disappeared".into())
        })?;
    if publication.command_id != replica.command_id
        || publication.snapshot_digest != replica.snapshot_digest
        || publication.snapshot_expires_at != replica.snapshot_expires_at
        || publication.state != GatewayPublicationState::Pending
    {
        return Err(RepositoryError::Storage(
            "Gateway rollout unavailable publication is inconsistent".into(),
        ));
    }
    publication
        .mark_unavailable(failure, observed_at)
        .map_err(RepositoryError::Conflict)?;
    rollout
        .mark_unavailable(node_id, failure, observed_at)
        .map_err(RepositoryError::Conflict)?;

    if let Some((mut certificate, reused)) =
        certificate_binding(&next, &rollout, &publication, rollout.started_at)?
    {
        if !reused {
            certificate
                .mark_delivery_unavailable(failure, observed_at)
                .map_err(RepositoryError::Conflict)?;
            next.certificates.insert(certificate.id, certificate);
        }
    }

    project_route_unavailability(&mut next, &rollout, node_id, failure, observed_at)?;
    project_terminal_rollback(&mut next, &rollout)?;
    next.publications
        .insert((node_id, replica.revision), publication);
    next.rollouts.insert(rollout_id, rollout.clone());
    *state = next;
    Ok(rollout)
}

pub(in super::super) fn pending_recoveries(
    state: &State,
    limit: usize,
) -> Result<Vec<GatewayReplicaRecoveryTarget>, RepositoryError> {
    validate_batch_limit(limit)?;
    let mut candidates = state
        .rollouts
        .values()
        .flat_map(|rollout| {
            rollout.replicas.iter().filter_map(move |replica| {
                replica
                    .recovery
                    .as_ref()
                    .filter(|recovery| {
                        matches!(
                            recovery.state,
                            GatewayReplicaRecoveryState::Required
                                | GatewayReplicaRecoveryState::Observing
                        )
                    })
                    .map(|_| (rollout, replica))
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(rollout, replica)| (rollout.started_at, rollout.id, replica.node_id));
    candidates.truncate(limit);

    let mut targets = Vec::with_capacity(candidates.len());
    for (rollout, replica) in candidates {
        let scope = state
            .gateway_scopes
            .get(&rollout.gateway_scope_id)
            .ok_or_else(|| RepositoryError::Storage("Gateway rollout scope disappeared".into()))?;
        let publication = recovery_publication(state, rollout, replica.node_id)?;
        let prior_publication = publication
            .expected_revision
            .map(|revision| {
                state
                    .publications
                    .get(&(replica.node_id, revision))
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "Gateway replica recovery prior publication disappeared".into(),
                        )
                    })
            })
            .transpose()?;
        let target = GatewayReplicaRecoveryTarget {
            organization_id: scope.organization_id,
            rollout: rollout.clone(),
            publication,
            prior_publication,
        };
        target.validate().map_err(RepositoryError::Storage)?;
        targets.push(target);
    }
    Ok(targets)
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn stage_recovery_observation(
    state: &mut State,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    command_id: NodeCommandId,
    issued_at: DateTime<Utc>,
    not_after: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    transition_recovery(
        state,
        organization_id,
        rollout_id,
        node_id,
        expected_version,
        |rollout, _, _| {
            rollout.stage_recovery_observation(node_id, command_id, issued_at, not_after)
        },
        "Gateway rollout changed before recovery observation was staged",
    )
}

pub(in super::super) fn record_recovery_observation(
    state: &mut State,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    observation: NodeGatewaySnapshotObservation,
) -> Result<GatewayRollout, RepositoryError> {
    let mut next = state.clone();
    let mut rollout = find(&next, organization_id, rollout_id)?;
    if rollout.aggregate_version != expected_version {
        return Err(RepositoryError::Conflict(
            "Gateway rollout changed before recovery observation was recorded".into(),
        ));
    }
    let candidate = recovery_publication(&next, &rollout, node_id)?;
    let prior = candidate
        .expected_revision
        .map(|revision| {
            next.publications
                .get(&(node_id, revision))
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Gateway replica recovery prior publication disappeared".into(),
                    )
                })
        })
        .transpose()?;
    let changed = rollout
        .record_recovery_observation(node_id, &candidate, prior.as_ref(), observation)
        .map_err(RepositoryError::Conflict)?;
    if changed {
        let recovery = rollout
            .replicas
            .iter()
            .find(|replica| replica.node_id == node_id)
            .and_then(|replica| replica.recovery.as_ref())
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "Gateway replica recovery disappeared after observation".into(),
                )
            })?;
        match recovery.state {
            GatewayReplicaRecoveryState::Observed => {
                let installed_revision = recovery
                    .observation
                    .as_ref()
                    .and_then(|observation| observation.applied.as_ref())
                    .map(|applied| applied.revision);
                let physical_scope = next.scopes.get_mut(&node_id).ok_or_else(|| {
                    RepositoryError::Storage(
                        "physical Gateway scope disappeared during recovery".into(),
                    )
                })?;
                if physical_scope.installed_revision != installed_revision {
                    physical_scope.installed_revision = installed_revision;
                    physical_scope.aggregate_version = physical_scope
                        .aggregate_version
                        .checked_add(1)
                        .ok_or_else(|| {
                            RepositoryError::Storage(
                                "physical Gateway scope version space is exhausted".into(),
                            )
                        })?;
                }
            }
            GatewayReplicaRecoveryState::Diverged => {
                diverge_required_rollback(
                    &mut next,
                    rollout.id,
                    recovery.failure.as_deref().unwrap_or(
                        "Gateway physical state diverged during exact rollback recovery",
                    ),
                    recovery.updated_at,
                )?;
            }
            GatewayReplicaRecoveryState::Required | GatewayReplicaRecoveryState::Observing => {}
        }
    }
    next.rollouts.insert(rollout.id, rollout.clone());
    *state = next;
    Ok(rollout)
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn record_recovery_failure(
    state: &mut State,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    command_id: NodeCommandId,
    failure: &str,
    retryable: bool,
    failed_at: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    let failure = failure.to_owned();
    transition_recovery(
        state,
        organization_id,
        rollout_id,
        node_id,
        expected_version,
        move |rollout, _, _| {
            rollout
                .record_recovery_command_failure(node_id, command_id, failure, retryable, failed_at)
        },
        "Gateway rollout changed before recovery failure was recorded",
    )
}

fn transition_recovery(
    state: &mut State,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    transition: impl FnOnce(
        &mut GatewayRollout,
        &GatewayPublication,
        Option<&GatewayPublication>,
    ) -> Result<bool, String>,
    version_conflict: &'static str,
) -> Result<GatewayRollout, RepositoryError> {
    let mut next = state.clone();
    let mut rollout = find(&next, organization_id, rollout_id)?;
    if rollout.aggregate_version != expected_version {
        return Err(RepositoryError::Conflict(version_conflict.into()));
    }
    let candidate = recovery_publication(&next, &rollout, node_id)?;
    let prior = candidate
        .expected_revision
        .map(|revision| {
            next.publications
                .get(&(node_id, revision))
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Gateway replica recovery prior publication disappeared".into(),
                    )
                })
        })
        .transpose()?;
    transition(&mut rollout, &candidate, prior.as_ref()).map_err(RepositoryError::Conflict)?;
    next.rollouts.insert(rollout.id, rollout.clone());
    *state = next;
    Ok(rollout)
}

fn recovery_publication(
    state: &State,
    rollout: &GatewayRollout,
    node_id: NodeId,
) -> Result<GatewayPublication, RepositoryError> {
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .ok_or_else(|| {
            RepositoryError::Conflict("Gateway rollout does not contain this member".into())
        })?;
    if state.rollout_publications.get(&(node_id, replica.revision)) != Some(&rollout.id) {
        return Err(RepositoryError::Storage(
            "Gateway replica recovery publication ownership is inconsistent".into(),
        ));
    }
    state
        .publications
        .get(&(node_id, replica.revision))
        .filter(|publication| publication.command_id == replica.command_id)
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Gateway replica recovery candidate publication disappeared".into(),
            )
        })
}

fn project_route_unavailability(
    state: &mut State,
    rollout: &GatewayRollout,
    node_id: NodeId,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<(), RepositoryError> {
    let key = (rollout.id, node_id);
    let Some(mut projection) = state.rollout_route_projections.get(&key).cloned() else {
        if state
            .rollout_route_projections
            .keys()
            .any(|(rollout_id, _)| *rollout_id == rollout.id)
        {
            return Err(RepositoryError::Storage(
                "Gateway Route rollout omitted an unavailable member projection".into(),
            ));
        }
        return Ok(());
    };
    let mut logical = state.routes.get(&projection.id).cloned().ok_or_else(|| {
        RepositoryError::Storage("Gateway Route rollout logical Route disappeared".into())
    })?;
    validate_logical_projection(&logical, &projection, rollout)?;
    projection
        .mark_unavailable_from_gateway_rollout(failure, observed_at)
        .map_err(RepositoryError::Conflict)?;
    if rollout.state.terminal() && !rollout.serves_traffic().map_err(RepositoryError::Storage)? {
        logical
            .reject_from_gateway_rollout(
                "Gateway rollout did not reach its readiness threshold",
                rollout_observed_at(rollout),
            )
            .map_err(RepositoryError::Conflict)?;
    }
    state.rollout_route_projections.insert(key, projection);
    state.routes.insert(logical.id, logical);
    Ok(())
}

fn validate_logical_projection(
    logical: &Route,
    projection: &Route,
    rollout: &GatewayRollout,
) -> Result<(), RepositoryError> {
    if logical.id != projection.id
        || logical.organization_id != projection.organization_id
        || logical.project_id != projection.project_id
        || logical.environment_id != projection.environment_id
        || logical.gateway_scope_id != rollout.gateway_scope_id
        || projection.gateway_scope_id != rollout.gateway_scope_id
        || logical.hostname != projection.hostname
        || logical.path_prefix != projection.path_prefix
        || logical.domain_claim_id != projection.domain_claim_id
        || logical.domain_pattern != projection.domain_pattern
        || logical.workload_id != projection.workload_id
        || logical.target.workload_revision_id != projection.target.workload_revision_id
        || logical.target.runtime_unit_id != projection.target.runtime_unit_id
        || logical.target.runtime_generation != projection.target.runtime_generation
        || logical.target.port_name != projection.target.port_name
        || logical.created_at != projection.created_at
    {
        return Err(RepositoryError::Storage(
            "Gateway rollout logical and physical Route projections diverged".into(),
        ));
    }
    Ok(())
}

fn rollout_observed_at(rollout: &GatewayRollout) -> DateTime<Utc> {
    rollout
        .replicas
        .iter()
        .filter_map(|replica| replica.acknowledged_at)
        .max()
        .unwrap_or(rollout.started_at)
}

pub(in super::super) fn project_terminal_rollback(
    state: &mut State,
    rollout: &GatewayRollout,
) -> Result<(), RepositoryError> {
    if !rollout.state.terminal() {
        return Ok(());
    }
    if let Some(failed_rollout_id) = state
        .rollout_rollbacks
        .values()
        .find(|rollback| rollback.rollback_rollout_id == rollout.id)
        .map(|rollback| rollback.failed_rollout_id)
    {
        let mut rollback = state
            .rollout_rollbacks
            .get(&failed_rollout_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("Gateway rollback intent disappeared".into())
            })?;
        match rollout.state {
            GatewayRolloutState::Succeeded => {
                rollback
                    .succeed(rollout)
                    .map_err(RepositoryError::Conflict)?;
                release_failed_route_ownership(state, failed_rollout_id)?;
            }
            GatewayRolloutState::Degraded => {
                rollback
                    .diverge(
                        rollout,
                        "Gateway exact rollback did not receive every member acknowledgement",
                    )
                    .map_err(RepositoryError::Conflict)?;
            }
            GatewayRolloutState::Pending | GatewayRolloutState::Ready => unreachable!(),
        }
        state.rollout_rollbacks.insert(failed_rollout_id, rollback);
        return Ok(());
    }
    if rollout.state == GatewayRolloutState::Degraded
        && !rollout.serves_traffic().map_err(RepositoryError::Storage)?
    {
        let required =
            GatewayRolloutRollback::required(rollout).map_err(RepositoryError::Storage)?;
        match state.rollout_rollbacks.get(&rollout.id) {
            Some(existing) if existing == &required => {}
            Some(_) => {
                return Err(RepositoryError::Storage(
                    "Gateway rollout has conflicting exact rollback intent".into(),
                ))
            }
            None => {
                state.rollout_rollbacks.insert(rollout.id, required);
            }
        }
    }
    Ok(())
}

fn release_failed_route_ownership(
    state: &mut State,
    failed_rollout_id: GatewayRolloutId,
) -> Result<(), RepositoryError> {
    let mut projections = state
        .rollout_route_projections
        .iter()
        .filter(|((rollout_id, _), _)| *rollout_id == failed_rollout_id)
        .map(|(_, route)| route.clone())
        .collect::<Vec<_>>();
    if projections.is_empty() {
        return Ok(());
    }
    let failed = state
        .rollouts
        .get(&failed_rollout_id)
        .ok_or_else(|| RepositoryError::Storage("failed Gateway rollout disappeared".into()))?;
    projections.sort_by_key(|route| route.gateway_node_id);
    if projections.len() != failed.replicas.len()
        || projections
            .iter()
            .map(|route| route.gateway_node_id)
            .ne(failed.replicas.iter().map(|replica| replica.node_id))
    {
        return Err(RepositoryError::Storage(
            "failed Gateway rollout physical Route ownership is incomplete".into(),
        ));
    }
    for projection in projections {
        let logical = state.routes.get(&projection.id).ok_or_else(|| {
            RepositoryError::Storage("failed Gateway rollout logical Route disappeared".into())
        })?;
        if logical.state != crate::modules::edge::domain::RouteState::Rejected {
            return Err(RepositoryError::Storage(
                "Gateway rollback cannot release ownership for a non-rejected logical Route".into(),
            ));
        }
        let ownership = (
            projection.gateway_node_id,
            projection.hostname.as_str().to_owned(),
            projection.path_prefix.as_str().to_owned(),
        );
        match state.ownership.get(&ownership) {
            Some(route_id) if *route_id == projection.id => {
                state.ownership.remove(&ownership);
            }
            Some(_) => {
                return Err(RepositoryError::Storage(
                    "Gateway rollback physical Route ownership changed identity".into(),
                ))
            }
            None => {
                return Err(RepositoryError::Storage(
                    "Gateway rollback physical Route ownership was released too early".into(),
                ))
            }
        }
    }
    Ok(())
}

fn diverge_required_rollback(
    state: &mut State,
    failed_rollout_id: GatewayRolloutId,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<(), RepositoryError> {
    let Some(mut rollback) = state.rollout_rollbacks.get(&failed_rollout_id).cloned() else {
        return Err(RepositoryError::Storage(
            "Gateway physical divergence omitted its rollback intent".into(),
        ));
    };
    if rollback.state == GatewayRolloutRollbackState::Required {
        rollback
            .diverge_before_staging(failure, observed_at)
            .map_err(RepositoryError::Conflict)?;
        state.rollout_rollbacks.insert(failed_rollout_id, rollback);
    }
    Ok(())
}
