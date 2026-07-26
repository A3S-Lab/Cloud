use super::models::{ReplicaRow, ReplicaSelection, RolloutRow, RolloutSelection};
use crate::modules::edge::domain::repositories::GatewayRolloutDispatchTarget;
use crate::modules::edge::domain::{
    GatewayPublication, GatewayPublicationState, GatewayReplicaRollout, GatewayReplicaRolloutState,
    GatewayRollout,
};
use crate::modules::edge::infrastructure::persistence::postgres::PublicationRow;
use crate::modules::edge::infrastructure::persistence::postgres_schema::{
    GatewayPublications, GatewayRolloutReplicas, GatewayRollouts,
};
use crate::modules::shared_kernel::domain::{
    GatewayCertificateId, OrganizationId, RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    orm_table, select_from, Database, DecodeError, Expression, FromRow, OrderDirection,
    PostgresDialect, PostgresExecutor, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    struct PendingGatewayRolloutIds => "pending_gateway_rollout_ids" {
        id: Uuid => "id",
        started_at: DateTime<Utc> => "started_at",
    }
}

struct PublicationSelection;

impl Selection for PublicationSelection {
    type Output = PublicationRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayPublications::node_id().expression(),
            GatewayPublications::revision().expression(),
            GatewayPublications::expected_revision().expression(),
            GatewayPublications::command_id().expression(),
            GatewayPublications::command_correlation_id().expression(),
            GatewayPublications::snapshot_digest().expression(),
            GatewayPublications::acl().expression(),
            GatewayPublications::state().expression(),
            GatewayPublications::failure().expression(),
            GatewayPublications::command_issued_at().expression(),
            GatewayPublications::command_not_after().expression(),
            GatewayPublications::snapshot_expires_at().expression(),
            GatewayPublications::acknowledged_at().expression(),
            GatewayPublications::certificate_request().expression(),
        ]
    }
}

struct RolloutReplicaPublicationSelection;

impl Selection for RolloutReplicaPublicationSelection {
    type Output = RolloutReplicaPublicationRow;

    fn expressions(self) -> Vec<Expression> {
        let mut expressions = PublicationSelection.expressions();
        expressions.extend(RolloutSelection.expressions());
        expressions.extend(ReplicaSelection.expressions());
        expressions
    }
}

struct RolloutReplicaPublicationRow {
    publication: PublicationRow,
    rollout: RolloutRow,
    replica: ReplicaRow,
}

impl FromRow for RolloutReplicaPublicationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            publication: PublicationRow::from_row(row)?,
            rollout: RolloutRow::from_row_at(row, 14)?,
            replica: ReplicaRow::from_row_at(row, 29)?,
        })
    }
}

pub(in crate::modules::edge::infrastructure::persistence) async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<GatewayRolloutDispatchTarget>, RepositoryError> {
    validate_batch_limit(limit)?;
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict("Gateway rollout dispatch limit exceeds supported range".into())
    })?;
    let pending_rollouts = select_from::<GatewayRollouts>()
        .select((GatewayRollouts::id(), GatewayRollouts::started_at()))
        .filter(
            GatewayRollouts::state()
                .eq("pending")
                .or(GatewayRollouts::state().eq("ready")),
        )
        .order_by(GatewayRollouts::started_at(), OrderDirection::Asc)
        .order_by(GatewayRollouts::id(), OrderDirection::Asc)
        .limit(limit)
        .as_cte::<PendingGatewayRolloutIds>();
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<PendingGatewayRolloutIds>()
                .with(pending_rollouts)
                .inner_join::<GatewayRollouts>(
                    PendingGatewayRolloutIds::id().eq_column(GatewayRollouts::id()),
                )
                .inner_join::<GatewayRolloutReplicas>(
                    GatewayRollouts::id().eq_column(GatewayRolloutReplicas::gateway_rollout_id()),
                )
                .inner_join::<GatewayPublications>(
                    GatewayRolloutReplicas::node_id()
                        .eq_column(GatewayPublications::node_id())
                        .and(
                            GatewayRolloutReplicas::revision()
                                .eq_column(GatewayPublications::revision()),
                        )
                        .and(
                            GatewayRolloutReplicas::command_id()
                                .eq_column(GatewayPublications::command_id()),
                        ),
                )
                .select(RolloutReplicaPublicationSelection)
                .order_by(PendingGatewayRolloutIds::started_at(), OrderDirection::Asc)
                .order_by(PendingGatewayRolloutIds::id(), OrderDirection::Asc)
                .order_by(GatewayRolloutReplicas::node_id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows;
    rebuild_dispatch_targets(rows)
}

struct DispatchGroup {
    rollout: RolloutRow,
    rows: Vec<(ReplicaRow, PublicationRow)>,
}

fn rebuild_dispatch_targets(
    rows: Vec<RolloutReplicaPublicationRow>,
) -> Result<Vec<GatewayRolloutDispatchTarget>, RepositoryError> {
    let mut groups = Vec::<DispatchGroup>::new();
    for row in rows {
        match groups.last_mut() {
            Some(group) if group.rollout.id == row.rollout.id => {
                if group.rollout != row.rollout {
                    return Err(RepositoryError::Storage(
                        "joined Gateway rollout dispatch rows contain inconsistent aggregate data"
                            .into(),
                    ));
                }
                group.rows.push((row.replica, row.publication));
            }
            _ => groups.push(DispatchGroup {
                rollout: row.rollout,
                rows: vec![(row.replica, row.publication)],
            }),
        }
    }
    groups.into_iter().map(rebuild_dispatch_target).collect()
}

fn rebuild_dispatch_target(
    group: DispatchGroup,
) -> Result<GatewayRolloutDispatchTarget, RepositoryError> {
    let organization_id = OrganizationId::from_uuid(group.rollout.organization_id);
    let mut replicas = Vec::with_capacity(group.rows.len());
    let mut publications = Vec::with_capacity(group.rows.len());
    for (replica, publication) in group.rows {
        replicas.push(replica.replica()?);
        publications.push(publication.publication()?);
    }
    let rollout = group.rollout.rollout(replicas)?;
    publications.sort_by_key(|publication| publication.node_id);
    if rollout.replicas.len() != publications.len() {
        return Err(RepositoryError::Storage(
            "Gateway rollout dispatch publication count is inconsistent".into(),
        ));
    }
    let mut pending_publications = Vec::new();
    for (replica, publication) in rollout.replicas.iter().zip(publications) {
        validate_rollout_publication(&rollout, replica, &publication)?;
        if replica.state == GatewayReplicaRolloutState::Pending {
            pending_publications.push(publication);
        }
    }
    let target = GatewayRolloutDispatchTarget {
        organization_id,
        rollout,
        publications: pending_publications,
    };
    target.validate().map_err(RepositoryError::Storage)?;
    Ok(target)
}

fn validate_rollout_publication(
    rollout: &GatewayRollout,
    replica: &GatewayReplicaRollout,
    publication: &GatewayPublication,
) -> Result<(), RepositoryError> {
    publication
        .snapshot()
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    let expected_publication_state = match replica.state {
        GatewayReplicaRolloutState::Pending => GatewayPublicationState::Pending,
        GatewayReplicaRolloutState::Applied => GatewayPublicationState::Applied,
        GatewayReplicaRolloutState::Rejected => GatewayPublicationState::Rejected,
        GatewayReplicaRolloutState::Unavailable => GatewayPublicationState::Unavailable,
    };
    if publication.node_id != replica.node_id
        || publication.revision != replica.revision
        || publication.command_id != replica.command_id
        || publication.command_correlation_id != rollout.correlation_id
        || publication.snapshot_digest != replica.snapshot_digest
        || publication.snapshot_expires_at != replica.snapshot_expires_at
        || publication
            .certificate_request
            .as_ref()
            .map(|request| GatewayCertificateId::from_uuid(request.certificate_id))
            != replica.gateway_certificate_id
        || publication.state != expected_publication_state
        || publication.failure != replica.failure
        || publication.acknowledged_at != replica.acknowledged_at
        || publication.command_issued_at != rollout.started_at
    {
        return Err(RepositoryError::Storage(
            "Gateway rollout publication projection is inconsistent".into(),
        ));
    }
    Ok(())
}

fn validate_batch_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway rollout dispatch batch limit is invalid".into(),
        ));
    }
    Ok(())
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
