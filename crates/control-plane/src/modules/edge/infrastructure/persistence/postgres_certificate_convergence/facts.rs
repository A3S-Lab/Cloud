use super::support::persist_route_convergence;
use super::*;
use crate::infrastructure::OutboxEvents;
use crate::modules::edge::domain::events::{
    GatewayCertificateExpiryChanged, GatewayCertificateRenewalChanged,
};

struct ExpiryOutboxRow {
    event_key: String,
    schema_version: u32,
    organization_id: Uuid,
    aggregate_id: Uuid,
    aggregate_version: u64,
    occurred_at: DateTime<Utc>,
    correlation_id: Uuid,
    causation_id: Option<Uuid>,
    payload: serde_json::Value,
}

struct ExpiryOutboxSelection;

impl Selection for ExpiryOutboxSelection {
    type Output = ExpiryOutboxRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            OutboxEvents::event_key().expression(),
            OutboxEvents::schema_version().expression(),
            OutboxEvents::organization_id().expression(),
            OutboxEvents::aggregate_id().expression(),
            OutboxEvents::aggregate_version().expression(),
            OutboxEvents::occurred_at().expression(),
            OutboxEvents::correlation_id().expression(),
            OutboxEvents::causation_id().expression(),
            OutboxEvents::payload().expression(),
        ]
    }
}

impl FromRow for ExpiryOutboxRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            event_key: decode(row, 0)?,
            schema_version: decode(row, 1)?,
            organization_id: decode(row, 2)?,
            aggregate_id: decode(row, 3)?,
            aggregate_version: decode(row, 4)?,
            occurred_at: decode(row, 5)?,
            correlation_id: decode(row, 6)?,
            causation_id: decode(row, 7)?,
            payload: decode(row, 8)?,
        })
    }
}

impl ExpiryOutboxRow {
    fn envelope(self, event_id: Uuid) -> a3s_cloud_contracts::DomainEventEnvelope {
        a3s_cloud_contracts::DomainEventEnvelope {
            event_id,
            event_key: self.event_key,
            schema_version: self.schema_version,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: self.organization_id,
            },
            aggregate_id: self.aggregate_id,
            aggregate_version: self.aggregate_version,
            occurred_at: self.occurred_at,
            correlation_id: self.correlation_id,
            causation_id: self.causation_id,
            payload: self.payload,
        }
    }
}

pub(crate) async fn persist_acknowledgement(
    transaction: &PostgresTransaction,
    convergence: &GatewayCertificateConvergence,
    publication: &GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    let events = certificate_events(transaction, convergence, publication).await?;
    if convergence.state == GatewayCertificateConvergenceState::Applied {
        persist_route_convergence(transaction, convergence).await?;
    }
    require_one_row(
        "Gateway certificate convergence acknowledgement",
        execute(
            transaction,
            update_table::<GatewayCertificateConvergences>()
                .set(
                    GatewayCertificateConvergences::state(),
                    convergence.state.as_str(),
                )
                .set(
                    GatewayCertificateConvergences::failure(),
                    convergence.failure.clone(),
                )
                .set(
                    GatewayCertificateConvergences::acknowledged_at(),
                    convergence.acknowledged_at,
                )
                .filter(GatewayCertificateConvergences::node_id().eq(convergence.node_id.as_uuid()))
                .filter(
                    GatewayCertificateConvergences::gateway_revision()
                        .eq(convergence.gateway_revision),
                )
                .filter(GatewayCertificateConvergences::state().eq("pending")),
        )
        .await?,
    )?;
    for event in events {
        store_outbox(transaction, &event).await?;
    }
    Ok(())
}

async fn certificate_events(
    transaction: &PostgresTransaction,
    convergence: &GatewayCertificateConvergence,
    publication: &GatewayPublication,
) -> Result<Vec<a3s_cloud_contracts::DomainEventEnvelope>, PostgresPersistenceError> {
    if convergence.reason != GatewayCertificateConvergenceReason::Renewal {
        return Ok(Vec::new());
    }
    let active_certificate_id = match convergence.state {
        GatewayCertificateConvergenceState::Applied => convergence.replacement_certificate_id,
        GatewayCertificateConvergenceState::Rejected
        | GatewayCertificateConvergenceState::Unavailable => {
            Some(convergence.previous_certificate_id)
        }
        GatewayCertificateConvergenceState::Pending => return Ok(Vec::new()),
    }
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Gateway certificate renewal omitted its active certificate".into(),
        )
    })?;
    let active_certificate = fetch_optional::<CertificateRow, _>(
        transaction,
        select_from::<GatewayCertificates>()
            .select(CertificateSelection)
            .filter(GatewayCertificates::id().eq(active_certificate_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("active Gateway renewal certificate disappeared".into())
    })?
    .certificate()?;
    let mut routes = Vec::with_capacity(convergence.retained_routes.len());
    for version in &convergence.retained_routes {
        let route = match super::super::postgres_rollout_routes::route_projection(
            transaction,
            version.route_id,
            convergence.node_id,
        )
        .await?
        {
            Some(route) => route,
            None => fetch_optional::<RouteRow, _>(
                transaction,
                select_from::<Routes>()
                    .select(RouteSelection)
                    .filter(Routes::id().eq(version.route_id.as_uuid()))
                    .for_update(),
            )
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "Gateway certificate renewal Route disappeared".into(),
                )
            })?
            .route()?,
        };
        routes.push(route);
    }
    let mut events = GatewayCertificateRenewalChanged::envelopes(
        convergence,
        publication,
        &active_certificate,
        &routes,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    events.extend(
        GatewayCertificateExpiryChanged::envelopes(
            convergence,
            publication,
            &active_certificate,
            &routes,
        )
        .map_err(PostgresPersistenceError::Invariant)?,
    );
    Ok(events)
}

pub(super) fn retained_routes(
    active: &[Route],
    convergence: &GatewayCertificateConvergence,
) -> Result<Vec<Route>, RepositoryError> {
    let active = active
        .iter()
        .map(|route| (route.id, route.clone()))
        .collect::<BTreeMap<_, _>>();
    convergence
        .retained_routes
        .iter()
        .map(|version| {
            active.get(&version.route_id).cloned().ok_or_else(|| {
                RepositoryError::Storage(
                    "Gateway certificate convergence retained Route disappeared".into(),
                )
            })
        })
        .collect()
}

pub(super) async fn store_expiry_event_once(
    transaction: &PostgresTransaction,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
) -> Result<(), PostgresPersistenceError> {
    let existing = fetch_optional::<ExpiryOutboxRow, _>(
        transaction,
        select_from::<OutboxEvents>()
            .select(ExpiryOutboxSelection)
            .filter(OutboxEvents::event_id().eq(event.event_id))
            .for_update(),
    )
    .await?;
    let Some(existing) = existing else {
        return store_outbox(transaction, event).await;
    };
    let existing = existing.envelope(event.event_id);
    if !GatewayCertificateExpiryChanged::same_firing_identity(&existing, event)
        .map_err(PostgresPersistenceError::Invariant)?
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway certificate expiry firing event identity already exists".into(),
        ));
    }
    Ok(())
}
