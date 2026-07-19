use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_idempotency, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{CreateDomainClaimWrite, TransitionDomainClaim};
use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, DomainNamePattern, GatewayCertificate,
    GatewayCertificateMaterial, GatewayCertificateState,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, IdempotencyRequest, IdempotentWrite,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{DomainEventEnvelope, GatewayCertificateRequest};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) const SELECT_DOMAIN_CLAIMS: &str = "select id, organization_id, project_id, environment_id, pattern, challenge_dns_name, challenge_value, state, failure, aggregate_version, created_at, updated_at, verified_at, revoked_at from domain_claims";
pub(super) const SELECT_CERTIFICATES: &str = "select id, organization_id, node_id, domain_claim_ids, gateway_revision, gateway_command_id, snapshot_digest, request, state, csr_digest, serial_number, fingerprint, certificate_pem, ca_bundle_pem, issued_at, expires_at, failure, aggregate_version, created_at, updated_at, ready_at, revoked_at from gateway_certificates";

pub(super) struct DomainClaimRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    pattern: String,
    challenge_dns_name: String,
    challenge_value: String,
    state: String,
    failure: Option<String>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    verified_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for DomainClaimRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            pattern: decode(row, 4)?,
            challenge_dns_name: decode(row, 5)?,
            challenge_value: decode(row, 6)?,
            state: decode(row, 7)?,
            failure: decode(row, 8)?,
            aggregate_version: decode(row, 9)?,
            created_at: decode(row, 10)?,
            updated_at: decode(row, 11)?,
            verified_at: decode(row, 12)?,
            revoked_at: decode(row, 13)?,
        })
    }
}

impl DomainClaimRow {
    pub(super) fn claim(self) -> Result<DomainClaim, RepositoryError> {
        let pattern =
            DomainNamePattern::parse(self.pattern).map_err(stored("domain claim pattern"))?;
        if self.challenge_dns_name != pattern.challenge_dns_name()
            || self.challenge_value.len() < 32
            || self.challenge_value.len() > 512
            || self.challenge_value.contains(['\0', '\r', '\n'])
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
        {
            return Err(RepositoryError::Storage(
                "stored domain claim is inconsistent".into(),
            ));
        }
        let state = DomainClaimState::parse(&self.state).map_err(stored("domain claim state"))?;
        let state_consistent = match state {
            DomainClaimState::Pending => {
                self.failure.is_none() && self.verified_at.is_none() && self.revoked_at.is_none()
            }
            DomainClaimState::Verified => {
                self.failure.is_none() && self.verified_at.is_some() && self.revoked_at.is_none()
            }
            DomainClaimState::Rejected => {
                self.failure.is_some() && self.verified_at.is_none() && self.revoked_at.is_none()
            }
            DomainClaimState::Revoked => {
                self.failure.is_some() && self.verified_at.is_some() && self.revoked_at.is_some()
            }
        };
        if !state_consistent {
            return Err(RepositoryError::Storage(
                "stored domain claim transition is inconsistent".into(),
            ));
        }
        Ok(DomainClaim {
            id: DomainClaimId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            pattern,
            challenge_dns_name: self.challenge_dns_name,
            challenge_value: self.challenge_value,
            state,
            failure: self.failure,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            verified_at: self.verified_at,
            revoked_at: self.revoked_at,
        })
    }
}

pub(super) struct CertificateRow {
    id: Uuid,
    organization_id: Uuid,
    node_id: Uuid,
    domain_claim_ids: serde_json::Value,
    gateway_revision: u64,
    gateway_command_id: Uuid,
    snapshot_digest: String,
    request: serde_json::Value,
    state: String,
    csr_digest: Option<String>,
    serial_number: Option<String>,
    fingerprint: Option<String>,
    certificate_pem: Option<String>,
    ca_bundle_pem: Option<String>,
    issued_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    failure: Option<String>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    ready_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for CertificateRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            node_id: decode(row, 2)?,
            domain_claim_ids: decode(row, 3)?,
            gateway_revision: decode(row, 4)?,
            gateway_command_id: decode(row, 5)?,
            snapshot_digest: decode(row, 6)?,
            request: decode(row, 7)?,
            state: decode(row, 8)?,
            csr_digest: decode(row, 9)?,
            serial_number: decode(row, 10)?,
            fingerprint: decode(row, 11)?,
            certificate_pem: decode(row, 12)?,
            ca_bundle_pem: decode(row, 13)?,
            issued_at: decode(row, 14)?,
            expires_at: decode(row, 15)?,
            failure: decode(row, 16)?,
            aggregate_version: decode(row, 17)?,
            created_at: decode(row, 18)?,
            updated_at: decode(row, 19)?,
            ready_at: decode(row, 20)?,
            revoked_at: decode(row, 21)?,
        })
    }
}

impl CertificateRow {
    pub(super) fn certificate(self) -> Result<GatewayCertificate, RepositoryError> {
        let raw_claim_ids: Vec<Uuid> = serde_json::from_value(self.domain_claim_ids)
            .map_err(|error| stored("certificate domain claim IDs")(error.to_string()))?;
        let domain_claim_ids = raw_claim_ids
            .into_iter()
            .map(DomainClaimId::from_uuid)
            .collect::<Vec<_>>();
        if domain_claim_ids.is_empty() || domain_claim_ids.windows(2).any(|ids| ids[0] >= ids[1]) {
            return Err(RepositoryError::Storage(
                "stored Gateway certificate domain claims are invalid".into(),
            ));
        }
        let request: GatewayCertificateRequest = serde_json::from_value(self.request)
            .map_err(|error| stored("certificate request")(error.to_string()))?;
        request.validate().map_err(stored("certificate request"))?;
        if request.certificate_id != self.id
            || self.gateway_revision == 0
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
        {
            return Err(RepositoryError::Storage(
                "stored Gateway certificate identity is inconsistent".into(),
            ));
        }
        let material = match (
            self.serial_number,
            self.fingerprint,
            self.certificate_pem,
            self.ca_bundle_pem,
            self.issued_at,
            self.expires_at,
        ) {
            (
                Some(serial_number),
                Some(fingerprint),
                Some(certificate_pem),
                Some(ca_bundle_pem),
                Some(issued_at),
                Some(expires_at),
            ) => {
                let material = GatewayCertificateMaterial {
                    serial_number,
                    fingerprint,
                    certificate_pem,
                    ca_bundle_pem,
                    issued_at,
                    expires_at,
                };
                material
                    .validate()
                    .map_err(stored("certificate material"))?;
                Some(material)
            }
            (None, None, None, None, None, None) => None,
            _ => {
                return Err(RepositoryError::Storage(
                    "stored Gateway certificate material is partial".into(),
                ))
            }
        };
        let state =
            GatewayCertificateState::parse(&self.state).map_err(stored("certificate state"))?;
        let state_consistent = match state {
            GatewayCertificateState::Provisioning => {
                self.csr_digest.is_none()
                    && material.is_none()
                    && self.failure.is_none()
                    && self.ready_at.is_none()
                    && self.revoked_at.is_none()
            }
            GatewayCertificateState::Issued => {
                self.csr_digest.is_some()
                    && material.is_some()
                    && self.failure.is_none()
                    && self.ready_at.is_none()
                    && self.revoked_at.is_none()
            }
            GatewayCertificateState::Ready => {
                self.csr_digest.is_some()
                    && material.is_some()
                    && self.failure.is_none()
                    && self.ready_at.is_some()
                    && self.revoked_at.is_none()
            }
            GatewayCertificateState::Failed => {
                self.failure.is_some() && self.ready_at.is_none() && self.revoked_at.is_none()
            }
            GatewayCertificateState::Revoked => {
                self.csr_digest.is_some()
                    && material.is_some()
                    && self.failure.is_some()
                    && self.ready_at.is_some()
                    && self.revoked_at.is_some()
            }
        };
        if !state_consistent {
            return Err(RepositoryError::Storage(
                "stored Gateway certificate transition is inconsistent".into(),
            ));
        }
        Ok(GatewayCertificate {
            id: GatewayCertificateId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            node_id: NodeId::from_uuid(self.node_id),
            domain_claim_ids,
            gateway_revision: self.gateway_revision,
            gateway_command_id: NodeCommandId::from_uuid(self.gateway_command_id),
            snapshot_digest: self.snapshot_digest,
            request,
            state,
            csr_digest: self.csr_digest,
            material,
            failure: self.failure,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            ready_at: self.ready_at,
            revoked_at: self.revoked_at,
        })
    }
}

pub(super) async fn replay_domain_claim_write(
    executor: &PostgresExecutor,
    idempotency: &IdempotencyRequest,
) -> Result<Option<DomainClaim>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                Ok(idempotency_replay::<DomainClaim>(transaction, &idempotency)
                    .await?
                    .map(|replay| replay.value))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn create_domain_claim(
    executor: &PostgresExecutor,
    bundle: CreateDomainClaimWrite,
) -> Result<IdempotentWrite<DomainClaim>, RepositoryError> {
    validate_domain_event(&bundle.claim, &bundle.event)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(mut replay) =
                    idempotency_replay::<DomainClaim>(transaction, &bundle.idempotency).await?
                {
                    replay.replayed = true;
                    return Ok(replay);
                }
                execute(
                    transaction,
                    sql_query::<()>("lock table domain_claims in share row exclusive mode"),
                )
                .await?;
                let rows = fetch_all::<DomainClaimRow, _>(
                    transaction,
                    sql_query::<DomainClaimRow>(SELECT_DOMAIN_CLAIMS)
                        .append(" where state in ('pending', 'verified') for update"),
                )
                .await?;
                for existing in rows {
                    if existing
                        .claim()?
                        .pattern
                        .conflicts_with(&bundle.claim.pattern)
                    {
                        return Err(RepositoryError::Conflict(
                            "domain pattern overlaps an existing ownership claim".into(),
                        )
                        .into());
                    }
                }
                insert_domain_claim(transaction, &bundle.claim).await?;
                store_outbox(transaction, &bundle.event).await?;
                store_idempotency(transaction, &bundle.idempotency, &bundle.claim).await?;
                Ok(IdempotentWrite {
                    value: bundle.claim,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn transition_domain_claim(
    executor: &PostgresExecutor,
    bundle: TransitionDomainClaim,
) -> Result<IdempotentWrite<DomainClaim>, RepositoryError> {
    validate_domain_event(&bundle.claim, &bundle.event)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(mut replay) =
                    idempotency_replay::<DomainClaim>(transaction, &bundle.idempotency).await?
                {
                    replay.replayed = true;
                    return Ok(replay);
                }
                let existing = fetch_optional::<DomainClaimRow, _>(
                    transaction,
                    sql_query::<DomainClaimRow>(SELECT_DOMAIN_CLAIMS)
                        .append(" where organization_id = ")
                        .bind(bundle.claim.organization_id.as_uuid())
                        .append(" and id = ")
                        .bind(bundle.claim.id.as_uuid())
                        .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?
                .claim()?;
                validate_domain_transition(&existing, &bundle.claim, bundle.expected_version)?;
                require_one_row(
                    "domain claim transition",
                    execute(
                        transaction,
                        sql_query::<()>("update domain_claims set state = ")
                            .bind(bundle.claim.state.as_str())
                            .append(", failure = ")
                            .bind(bundle.claim.failure.as_deref())
                            .append(", aggregate_version = ")
                            .bind(bundle.claim.aggregate_version)
                            .append(", updated_at = ")
                            .bind(bundle.claim.updated_at)
                            .append(", verified_at = ")
                            .bind(bundle.claim.verified_at)
                            .append(", revoked_at = ")
                            .bind(bundle.claim.revoked_at)
                            .append(" where id = ")
                            .bind(bundle.claim.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(bundle.expected_version),
                    )
                    .await?,
                )?;
                store_outbox(transaction, &bundle.event).await?;
                store_idempotency(transaction, &bundle.idempotency, &bundle.claim).await?;
                Ok(IdempotentWrite {
                    value: bundle.claim,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find_domain_claim(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    claim_id: DomainClaimId,
) -> Result<DomainClaim, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<DomainClaimRow>(SELECT_DOMAIN_CLAIMS)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(claim_id.as_uuid()),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)?
        .claim()
}

pub(super) async fn list_domain_claims(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<Vec<DomainClaim>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<DomainClaimRow>(SELECT_DOMAIN_CLAIMS)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and environment_id = ")
                .bind(environment_id.as_uuid())
                .append(" order by created_at, id"),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(DomainClaimRow::claim)
        .collect()
}

pub(super) async fn find_gateway_certificate(
    executor: &PostgresExecutor,
    node_id: NodeId,
    certificate_id: GatewayCertificateId,
) -> Result<GatewayCertificate, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<CertificateRow>(SELECT_CERTIFICATES)
                .append(" where node_id = ")
                .bind(node_id.as_uuid())
                .append(" and id = ")
                .bind(certificate_id.as_uuid()),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)?
        .certificate()
}

pub(super) async fn list_gateway_certificates(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
) -> Result<Vec<GatewayCertificate>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<CertificateRow>(SELECT_CERTIFICATES)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" order by created_at, id"),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(CertificateRow::certificate)
        .collect()
}

pub(super) async fn transition_gateway_certificate(
    executor: &PostgresExecutor,
    certificate: GatewayCertificate,
    expected_version: u64,
) -> Result<GatewayCertificate, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let existing = fetch_optional::<CertificateRow, _>(
                    transaction,
                    sql_query::<CertificateRow>(SELECT_CERTIFICATES)
                        .append(" where id = ")
                        .bind(certificate.id.as_uuid())
                        .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?
                .certificate()?;
                validate_gateway_certificate_transition(&existing, &certificate, expected_version)?;
                update_certificate(transaction, &certificate, expected_version).await?;
                Ok(certificate)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn insert_domain_claim(
    transaction: &a3s_orm::PostgresTransaction,
    claim: &DomainClaim,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into domain_claims (id, organization_id, project_id, environment_id, pattern, challenge_dns_name, challenge_value, state, failure, aggregate_version, created_at, updated_at, verified_at, revoked_at) values (",
        )
        .bind(claim.id.as_uuid())
        .append(", ")
        .bind(claim.organization_id.as_uuid())
        .append(", ")
        .bind(claim.project_id.as_uuid())
        .append(", ")
        .bind(claim.environment_id.as_uuid())
        .append(", ")
        .bind(claim.pattern.as_str())
        .append(", ")
        .bind(claim.challenge_dns_name.as_str())
        .append(", ")
        .bind(claim.challenge_value.as_str())
        .append(", ")
        .bind(claim.state.as_str())
        .append(", ")
        .bind(claim.failure.as_deref())
        .append(", ")
        .bind(claim.aggregate_version)
        .append(", ")
        .bind(claim.created_at)
        .append(", ")
        .bind(claim.updated_at)
        .append(", ")
        .bind(claim.verified_at)
        .append(", ")
        .bind(claim.revoked_at)
        .append(")"),
    )
    .await;
    map_domain_insert(result)
}

pub(super) async fn insert_certificate(
    transaction: &a3s_orm::PostgresTransaction,
    certificate: &GatewayCertificate,
) -> Result<(), PostgresPersistenceError> {
    let claim_ids = serde_json::to_value(
        certificate
            .domain_claim_ids
            .iter()
            .map(|id| id.as_uuid())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
    let request = serde_json::to_value(&certificate.request)
        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into gateway_certificates (id, organization_id, node_id, domain_claim_ids, gateway_revision, gateway_command_id, snapshot_digest, request, state, csr_digest, serial_number, fingerprint, certificate_pem, ca_bundle_pem, issued_at, expires_at, failure, aggregate_version, created_at, updated_at, ready_at, revoked_at) values (",
        )
        .bind(certificate.id.as_uuid())
        .append(", ")
        .bind(certificate.organization_id.as_uuid())
        .append(", ")
        .bind(certificate.node_id.as_uuid())
        .append(", ")
        .bind(claim_ids)
        .append(", ")
        .bind(certificate.gateway_revision)
        .append(", ")
        .bind(certificate.gateway_command_id.as_uuid())
        .append(", ")
        .bind(certificate.snapshot_digest.as_str())
        .append(", ")
        .bind(request)
        .append(", ")
        .bind(certificate.state.as_str())
        .append(", ")
        .bind(certificate.csr_digest.as_deref())
        .append(", ")
        .bind(certificate.material.as_ref().map(|value| value.serial_number.as_str()))
        .append(", ")
        .bind(certificate.material.as_ref().map(|value| value.fingerprint.as_str()))
        .append(", ")
        .bind(certificate.material.as_ref().map(|value| value.certificate_pem.as_str()))
        .append(", ")
        .bind(certificate.material.as_ref().map(|value| value.ca_bundle_pem.as_str()))
        .append(", ")
        .bind(certificate.material.as_ref().map(|value| value.issued_at))
        .append(", ")
        .bind(certificate.material.as_ref().map(|value| value.expires_at))
        .append(", ")
        .bind(certificate.failure.as_deref())
        .append(", ")
        .bind(certificate.aggregate_version)
        .append(", ")
        .bind(certificate.created_at)
        .append(", ")
        .bind(certificate.updated_at)
        .append(", ")
        .bind(certificate.ready_at)
        .append(", ")
        .bind(certificate.revoked_at)
        .append(")"),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("Gateway certificate", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Gateway certificate identity or revision already exists".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn update_certificate(
    transaction: &a3s_orm::PostgresTransaction,
    certificate: &GatewayCertificate,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let material = certificate.material.as_ref();
    require_one_row(
        "Gateway certificate transition",
        execute(
            transaction,
            sql_query::<()>("update gateway_certificates set state = ")
                .bind(certificate.state.as_str())
                .append(", csr_digest = ")
                .bind(certificate.csr_digest.as_deref())
                .append(", serial_number = ")
                .bind(material.map(|value| value.serial_number.as_str()))
                .append(", fingerprint = ")
                .bind(material.map(|value| value.fingerprint.as_str()))
                .append(", certificate_pem = ")
                .bind(material.map(|value| value.certificate_pem.as_str()))
                .append(", ca_bundle_pem = ")
                .bind(material.map(|value| value.ca_bundle_pem.as_str()))
                .append(", issued_at = ")
                .bind(material.map(|value| value.issued_at))
                .append(", expires_at = ")
                .bind(material.map(|value| value.expires_at))
                .append(", failure = ")
                .bind(certificate.failure.as_deref())
                .append(", aggregate_version = ")
                .bind(certificate.aggregate_version)
                .append(", updated_at = ")
                .bind(certificate.updated_at)
                .append(", ready_at = ")
                .bind(certificate.ready_at)
                .append(", revoked_at = ")
                .bind(certificate.revoked_at)
                .append(" where id = ")
                .bind(certificate.id.as_uuid())
                .append(" and aggregate_version = ")
                .bind(expected_version),
        )
        .await?,
    )
}

fn map_domain_insert(
    result: Result<u64, PostgresPersistenceError>,
) -> Result<(), PostgresPersistenceError> {
    match result {
        Ok(rows) => require_one_row("domain claim", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "domain pattern overlaps an existing ownership claim".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

fn validate_domain_event(
    claim: &DomainClaim,
    event: &DomainEventEnvelope,
) -> Result<(), RepositoryError> {
    if event.organization_id != claim.organization_id.as_uuid()
        || event.aggregate_id != claim.id.as_uuid()
        || event.aggregate_version != claim.aggregate_version
        || event.correlation_id.is_nil()
        || event.event_id.is_nil()
        || event.schema_version == 0
        || event.event_key.trim().is_empty()
    {
        return Err(RepositoryError::Conflict(
            "domain claim event does not match its aggregate".into(),
        ));
    }
    Ok(())
}

fn validate_domain_transition(
    existing: &DomainClaim,
    next: &DomainClaim,
    expected_version: u64,
) -> Result<(), RepositoryError> {
    if existing.aggregate_version != expected_version
        || next.aggregate_version != expected_version.saturating_add(1)
        || existing.id != next.id
        || existing.organization_id != next.organization_id
        || existing.project_id != next.project_id
        || existing.environment_id != next.environment_id
        || existing.pattern != next.pattern
        || existing.challenge_dns_name != next.challenge_dns_name
        || existing.challenge_value != next.challenge_value
        || existing.created_at != next.created_at
    {
        return Err(RepositoryError::Conflict(
            "domain claim changed while applying its transition".into(),
        ));
    }
    Ok(())
}

fn validate_gateway_certificate_transition(
    existing: &GatewayCertificate,
    next: &GatewayCertificate,
    expected_version: u64,
) -> Result<(), RepositoryError> {
    let transition_is_valid = match (existing.state, next.state) {
        (GatewayCertificateState::Provisioning, GatewayCertificateState::Issued) => {
            next.csr_digest.is_some()
                && next.material.is_some()
                && next.failure.is_none()
                && next.ready_at.is_none()
                && next.revoked_at.is_none()
        }
        (GatewayCertificateState::Provisioning, GatewayCertificateState::Failed) => {
            next.csr_digest.is_some()
                && next.material.is_none()
                && next.failure.is_some()
                && next.ready_at.is_none()
                && next.revoked_at.is_none()
        }
        (GatewayCertificateState::Ready, GatewayCertificateState::Revoked) => {
            next.csr_digest == existing.csr_digest
                && next.material == existing.material
                && next.failure.is_some()
                && next.ready_at == existing.ready_at
                && next.revoked_at.is_some()
        }
        _ => false,
    };
    if existing.aggregate_version != expected_version
        || next.aggregate_version != expected_version.saturating_add(1)
        || !transition_is_valid
        || existing.id != next.id
        || existing.organization_id != next.organization_id
        || existing.node_id != next.node_id
        || existing.domain_claim_ids != next.domain_claim_ids
        || existing.gateway_revision != next.gateway_revision
        || existing.gateway_command_id != next.gateway_command_id
        || existing.snapshot_digest != next.snapshot_digest
        || existing.request != next.request
        || existing.created_at != next.created_at
        || next.updated_at < existing.updated_at
    {
        return Err(RepositoryError::Conflict(
            "Gateway certificate changed while applying its transition".into(),
        ));
    }
    Ok(())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
