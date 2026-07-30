use super::{
    advance_physical_scope, insert_rollout, lock_by_id, lock_physical_scope, lock_rollback,
    persist_rollback_stage,
};
use crate::infrastructure::{
    fetch_optional, idempotency_replay, store_idempotency, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    GatewayRolloutResult, GatewayRolloutRollbackResult, StageGatewayRollout,
    StageGatewayRolloutRollback,
};
use crate::modules::edge::domain::GatewayRolloutRollbackState;
use crate::modules::edge::infrastructure::persistence::postgres::{
    insert_publication, insert_route, PublicationRow, PublicationSelection,
};
use crate::modules::edge::infrastructure::persistence::postgres_gateway_scopes;
use crate::modules::edge::infrastructure::persistence::postgres_rollout_routes;
use crate::modules::edge::infrastructure::persistence::postgres_schema::{
    GatewayCertificates, GatewayPublications, GatewayRolloutRollbacks, GatewayRollouts,
};
use crate::modules::edge::infrastructure::persistence::postgres_tls::{
    insert_certificate, CertificateRow, CertificateSelection,
};
use crate::modules::edge::infrastructure::{
    GatewayManagedSnapshotComposition, StageManagedGatewayRollout,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_orm::{select_from, PostgresExecutor};
use std::collections::BTreeMap;
use uuid::Uuid;

pub(in super::super) async fn stage(
    executor: &PostgresExecutor,
    bundle: StageGatewayRollout,
) -> Result<GatewayRolloutResult, RepositoryError> {
    stage_impl(executor, bundle, None).await
}

pub(in super::super) async fn stage_managed(
    executor: &PostgresExecutor,
    stage: StageManagedGatewayRollout,
) -> Result<GatewayRolloutResult, RepositoryError> {
    let (bundle, compositions) = stage.into_parts();
    stage_impl(executor, bundle, Some(compositions)).await
}

async fn stage_impl(
    executor: &PostgresExecutor,
    bundle: StageGatewayRollout,
    compositions: Option<
        BTreeMap<crate::modules::shared_kernel::domain::NodeId, GatewayManagedSnapshotComposition>,
    >,
) -> Result<GatewayRolloutResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    if let Some(compositions) = &compositions {
        for publication in &bundle.publications {
            compositions
                .get(&publication.node_id)
                .ok_or_else(|| {
                    RepositoryError::Conflict(
                        "managed Gateway rollout omitted a member composition".into(),
                    )
                })?
                .validate_for(publication)
                .map_err(RepositoryError::Conflict)?;
        }
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(mut replay) =
                    idempotency_replay::<GatewayRolloutResult>(transaction, &bundle.idempotency)
                        .await?
                {
                    replay.value.replayed = true;
                    return Ok(replay.value);
                }
                let stored_scope =
                    postgres_gateway_scopes::load_for_share(transaction, bundle.scope.id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                if stored_scope != bundle.scope {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope changed while staging its rollout".into(),
                    )
                    .into());
                }
                if fetch_optional::<Uuid, _>(
                    transaction,
                    select_from::<GatewayRollouts>()
                        .select(GatewayRollouts::id())
                        .filter(GatewayRollouts::gateway_scope_id().eq(bundle.scope.id.as_uuid()))
                        .filter(
                            GatewayRollouts::state()
                                .eq("pending")
                                .or(GatewayRollouts::state().eq("ready")),
                        )
                        .for_update(),
                )
                .await?
                .is_some()
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope already has an active rollout".into(),
                    )
                    .into());
                }
                if fetch_optional::<Uuid, _>(
                    transaction,
                    select_from::<GatewayRolloutRollbacks>()
                        .select(GatewayRolloutRollbacks::failed_rollout_id())
                        .filter(
                            GatewayRolloutRollbacks::gateway_scope_id()
                                .eq(bundle.scope.id.as_uuid()),
                        )
                        .filter(GatewayRolloutRollbacks::state().ne("succeeded"))
                        .for_update(),
                )
                .await?
                .is_some()
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope has an unresolved exact rollback".into(),
                    )
                    .into());
                }

                let mut physical_scopes = Vec::with_capacity(bundle.publications.len());
                for publication in &bundle.publications {
                    let current = match &compositions {
                        Some(compositions) => {
                            super::super::postgres_mcp_gateway_snapshots::lock_managed_composition(
                                transaction,
                                compositions.get(&publication.node_id).ok_or_else(|| {
                                    RepositoryError::Conflict(
                                        "managed Gateway rollout omitted a member composition"
                                            .into(),
                                    )
                                })?,
                            )
                            .await?
                        }
                        None => lock_physical_scope(transaction, publication.node_id).await?,
                    };
                    let expected_version = bundle
                        .expected_scope_versions
                        .get(&publication.node_id)
                        .copied()
                        .ok_or_else(|| {
                            RepositoryError::Conflict(
                                "Gateway rollout omitted a physical scope version".into(),
                            )
                        })?;
                    if current.aggregate_version != expected_version {
                        return Err(RepositoryError::Conflict(
                            "physical Gateway scope changed while staging its rollout".into(),
                        )
                        .into());
                    }
                    if fetch_optional::<u64, _>(
                        transaction,
                        select_from::<GatewayPublications>()
                            .select(GatewayPublications::revision())
                            .filter(
                                GatewayPublications::node_id().eq(publication.node_id.as_uuid()),
                            )
                            .filter(GatewayPublications::state().eq("pending"))
                            .for_update(),
                    )
                    .await?
                    .is_some()
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway rollout member already has a pending complete snapshot".into(),
                        )
                        .into());
                    }
                    if publication.revision
                        != current.next_revision().map_err(RepositoryError::Conflict)?
                        || publication.expected_revision != current.installed_revision
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway rollout publication does not advance its physical revision"
                                .into(),
                        )
                        .into());
                    }
                    physical_scopes.push(current);
                }

                for publication in &bundle.publications {
                    insert_publication(transaction, publication).await?;
                }
                for certificate in &bundle.certificates {
                    insert_certificate(transaction, certificate).await?;
                }
                if let Some(compositions) = &compositions {
                    for publication in &bundle.publications {
                        super::super::postgres_mcp_gateway_snapshots::persist_managed_composition(
                            transaction,
                            compositions.get(&publication.node_id).ok_or_else(|| {
                                RepositoryError::Conflict(
                                    "managed Gateway rollout omitted a member composition".into(),
                                )
                            })?,
                            publication,
                        )
                        .await?;
                    }
                }
                for (publication, current) in bundle.publications.iter().zip(physical_scopes.iter())
                {
                    advance_physical_scope(transaction, publication, current).await?;
                }
                insert_rollout(transaction, &bundle.scope, &bundle.rollout).await?;
                if let Some(primary_route) = bundle
                    .route_replicas
                    .iter()
                    .find(|route| route.gateway_node_id == bundle.scope.node_id)
                {
                    insert_route(transaction, primary_route).await?;
                    for route in &bundle.route_replicas {
                        postgres_rollout_routes::insert(transaction, &bundle, route).await?;
                    }
                }

                store_outbox(transaction, &bundle.event).await?;
                if let Some(route_event) = &bundle.route_event {
                    store_outbox(transaction, route_event).await?;
                }
                let StageGatewayRollout {
                    rollout,
                    route_replicas,
                    mut publications,
                    mut certificates,
                    idempotency,
                    ..
                } = bundle;
                publications.sort_by_key(|publication| publication.node_id);
                certificates.sort_by_key(|certificate| certificate.node_id);
                let result = GatewayRolloutResult {
                    rollout,
                    route_replicas,
                    publications,
                    certificates,
                    replayed: false,
                };
                store_idempotency(transaction, &idempotency, &result).await?;
                Ok(result)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn stage_rollback(
    executor: &PostgresExecutor,
    bundle: StageGatewayRolloutRollback,
) -> Result<GatewayRolloutRollbackResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let stored_scope =
                    postgres_gateway_scopes::load_for_share(transaction, bundle.scope.id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                if stored_scope != bundle.scope {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope changed before exact rollback staging".into(),
                    )
                    .into());
                }
                let (_, failed) = lock_by_id(transaction, bundle.failed_rollout.id)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if failed != bundle.failed_rollout {
                    return Err(RepositoryError::Conflict(
                        "failed Gateway rollout changed before exact rollback staging".into(),
                    )
                    .into());
                }
                let stored_rollback = lock_rollback(transaction, bundle.failed_rollout.id)
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Conflict("Gateway rollback intent is not durable".into())
                    })?;
                if stored_rollback == bundle.rollback {
                    let (_, stored_rollout) = lock_by_id(transaction, bundle.rollout.id)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "staged Gateway rollback lost its rollout".into(),
                            )
                        })?;
                    if stored_rollout != bundle.rollout {
                        return Err(PostgresPersistenceError::Invariant(
                            "staged Gateway rollback rollout changed".into(),
                        ));
                    }
                    validate_stored_rollback_evidence(transaction, &bundle).await?;
                    return Ok(GatewayRolloutRollbackResult {
                        rollback: stored_rollback,
                        rollout: stored_rollout,
                        publications: bundle.publications,
                        certificates: bundle.certificates,
                        reused_certificates: bundle.reused_certificates,
                        replayed: true,
                    });
                }
                if stored_rollback.aggregate_version != bundle.expected_rollback_version
                    || stored_rollback.state != GatewayRolloutRollbackState::Required
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway rollback intent changed before staging".into(),
                    )
                    .into());
                }
                let mut expected_staged = stored_rollback.clone();
                expected_staged
                    .stage(&bundle.rollout)
                    .map_err(RepositoryError::Conflict)?;
                if expected_staged != bundle.rollback {
                    return Err(RepositoryError::Conflict(
                        "Gateway rollback staged projection changed".into(),
                    )
                    .into());
                }
                if fetch_optional::<Uuid, _>(
                    transaction,
                    select_from::<GatewayRollouts>()
                        .select(GatewayRollouts::id())
                        .filter(GatewayRollouts::gateway_scope_id().eq(bundle.scope.id.as_uuid()))
                        .filter(
                            GatewayRollouts::state()
                                .eq("pending")
                                .or(GatewayRollouts::state().eq("ready")),
                        )
                        .for_update(),
                )
                .await?
                .is_some()
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope acquired another active rollout before rollback staging"
                            .into(),
                    )
                    .into());
                }

                let mut physical_scopes = Vec::with_capacity(bundle.publications.len());
                for publication in &bundle.publications {
                    let current = lock_physical_scope(transaction, publication.node_id).await?;
                    if bundle
                        .expected_scope_versions
                        .get(&publication.node_id)
                        .copied()
                        != Some(current.aggregate_version)
                        || publication.revision
                            != current.next_revision().map_err(RepositoryError::Conflict)?
                        || publication.expected_revision != current.installed_revision
                    {
                        return Err(RepositoryError::Conflict(
                            "physical Gateway state changed before exact rollback staging".into(),
                        )
                        .into());
                    }
                    if fetch_optional::<u64, _>(
                        transaction,
                        select_from::<GatewayPublications>()
                            .select(GatewayPublications::revision())
                            .filter(
                                GatewayPublications::node_id().eq(publication.node_id.as_uuid()),
                            )
                            .filter(GatewayPublications::state().eq("pending"))
                            .for_update(),
                    )
                    .await?
                    .is_some()
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway rollback member acquired another pending snapshot".into(),
                        )
                        .into());
                    }
                    physical_scopes.push(current);
                }
                for certificate in &bundle.reused_certificates {
                    let stored = fetch_optional::<CertificateRow, _>(
                        transaction,
                        select_from::<GatewayCertificates>()
                            .select(CertificateSelection)
                            .filter(GatewayCertificates::id().eq(certificate.id.as_uuid()))
                            .for_update(),
                    )
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Conflict(
                            "reused Gateway rollback certificate disappeared".into(),
                        )
                    })?
                    .certificate()?;
                    if stored != *certificate {
                        return Err(RepositoryError::Conflict(
                            "reused Gateway rollback certificate changed before staging".into(),
                        )
                        .into());
                    }
                }

                for publication in &bundle.publications {
                    insert_publication(transaction, publication).await?;
                }
                for certificate in &bundle.certificates {
                    insert_certificate(transaction, certificate).await?;
                }
                for (publication, current) in bundle.publications.iter().zip(&physical_scopes) {
                    advance_physical_scope(transaction, publication, current).await?;
                }
                insert_rollout(transaction, &bundle.scope, &bundle.rollout).await?;
                persist_rollback_stage(
                    transaction,
                    &bundle.rollback,
                    bundle.expected_rollback_version,
                )
                .await?;
                store_outbox(transaction, &bundle.event).await?;
                let StageGatewayRolloutRollback {
                    rollback,
                    rollout,
                    mut publications,
                    mut certificates,
                    mut reused_certificates,
                    ..
                } = bundle;
                publications.sort_by_key(|publication| publication.node_id);
                certificates.sort_by_key(|certificate| certificate.node_id);
                reused_certificates.sort_by_key(|certificate| certificate.node_id);
                Ok(GatewayRolloutRollbackResult {
                    rollback,
                    rollout,
                    publications,
                    certificates,
                    reused_certificates,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn validate_stored_rollback_evidence(
    transaction: &a3s_orm::PostgresTransaction,
    bundle: &StageGatewayRolloutRollback,
) -> Result<(), PostgresPersistenceError> {
    for publication in &bundle.publications {
        let stored = fetch_optional::<PublicationRow, _>(
            transaction,
            select_from::<GatewayPublications>()
                .select(PublicationSelection)
                .filter(GatewayPublications::node_id().eq(publication.node_id.as_uuid()))
                .filter(GatewayPublications::revision().eq(publication.revision))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "staged Gateway rollback publication disappeared".into(),
            )
        })?
        .publication()?;
        if stored != *publication {
            return Err(PostgresPersistenceError::Invariant(
                "staged Gateway rollback publication changed".into(),
            ));
        }
    }
    for certificate in bundle
        .certificates
        .iter()
        .chain(&bundle.reused_certificates)
    {
        let stored = fetch_optional::<CertificateRow, _>(
            transaction,
            select_from::<GatewayCertificates>()
                .select(CertificateSelection)
                .filter(GatewayCertificates::id().eq(certificate.id.as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "staged Gateway rollback certificate disappeared".into(),
            )
        })?
        .certificate()?;
        if stored != *certificate {
            return Err(PostgresPersistenceError::Invariant(
                "staged Gateway rollback certificate changed".into(),
            ));
        }
    }
    Ok(())
}
