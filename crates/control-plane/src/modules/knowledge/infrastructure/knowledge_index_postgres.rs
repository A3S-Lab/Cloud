use crate::infrastructure::{
    execute, fetch_all, fetch_optional, is_foreign_key_violation, is_unique_violation,
    require_one_row, transaction_error, PostgresPersistenceError,
};
use crate::modules::knowledge::domain::{
    CreateExternalKnowledgeBinding, CreateKnowledgeIndexRevision,
    CreateKnowledgeRetrievalPolicyRevision, ExternalKnowledgeBindingRecord,
    ExternalKnowledgeBindingV1, IExternalKnowledgeBindingRepository,
    IKnowledgeIndexRevisionRepository, IKnowledgeRetrievalPolicyRevisionRepository,
    KnowledgeIndexRevisionRecord, KnowledgeIndexRevisionV1,
    KnowledgeRetrievalPolicyRevisionRecord, KnowledgeRetrievalPolicyRevisionV1,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_INDEX: &str = "select organization_id, project_id, knowledge_base_revision_id, index_revision_id, index_digest, index_acl, created_at from knowledge_index_revisions";
const SELECT_POLICY: &str = "select organization_id, project_id, knowledge_base_revision_id, policy_revision_id, policy_digest, policy_acl, created_at from knowledge_retrieval_policy_revisions";
const SELECT_BINDING: &str = "select organization_id, project_id, knowledge_base_id, binding_id, binding_digest, binding_acl, created_at from external_knowledge_bindings";

#[derive(Clone)]
pub struct PostgresKnowledgeIndexRevisionRepository {
    executor: PostgresExecutor,
}

impl PostgresKnowledgeIndexRevisionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IKnowledgeIndexRevisionRepository for PostgresKnowledgeIndexRevisionRepository {
    async fn create(
        &self,
        request: CreateKnowledgeIndexRevision,
    ) -> Result<KnowledgeIndexRevisionRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = KnowledgeIndexRevisionRecord::new(
                        request.index_revision,
                        request.created_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                    insert_index(transaction, &record).await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        index_revision_id: Uuid,
    ) -> Result<Option<KnowledgeIndexRevisionRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_index(transaction, organization_id, index_revision_id).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeIndexRevisionRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict("KnowledgeIndexRevision list limit is out of bounds".into())
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<KnowledgeIndexRow, _>(
                        transaction,
                        sql_query::<KnowledgeIndexRow>(SELECT_INDEX)
                            .append(" where organization_id = ")
                            .bind(organization_id)
                            .append(" and knowledge_base_revision_id = ")
                            .bind(knowledge_base_revision_id)
                            .append(" order by index_revision_id asc limit ")
                            .bind(limit),
                    )
                    .await?;
                    rows.into_iter().map(decode_index).collect()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[derive(Clone)]
pub struct PostgresKnowledgeRetrievalPolicyRevisionRepository {
    executor: PostgresExecutor,
}

impl PostgresKnowledgeRetrievalPolicyRevisionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IKnowledgeRetrievalPolicyRevisionRepository
    for PostgresKnowledgeRetrievalPolicyRevisionRepository
{
    async fn create(
        &self,
        request: CreateKnowledgeRetrievalPolicyRevision,
    ) -> Result<KnowledgeRetrievalPolicyRevisionRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = KnowledgeRetrievalPolicyRevisionRecord::new(
                        request.policy_revision,
                        request.created_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                    insert_policy(transaction, &record).await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        policy_revision_id: Uuid,
    ) -> Result<Option<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_policy(transaction, organization_id, policy_revision_id).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict(
                "KnowledgeRetrievalPolicyRevision list limit is out of bounds".into(),
            )
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<KnowledgePolicyRow, _>(
                        transaction,
                        sql_query::<KnowledgePolicyRow>(SELECT_POLICY)
                            .append(" where organization_id = ")
                            .bind(organization_id)
                            .append(" and knowledge_base_revision_id = ")
                            .bind(knowledge_base_revision_id)
                            .append(" order by policy_revision_id asc limit ")
                            .bind(limit),
                    )
                    .await?;
                    rows.into_iter().map(decode_policy).collect()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[derive(Clone)]
pub struct PostgresExternalKnowledgeBindingRepository {
    executor: PostgresExecutor,
}

impl PostgresExternalKnowledgeBindingRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IExternalKnowledgeBindingRepository for PostgresExternalKnowledgeBindingRepository {
    async fn create(
        &self,
        request: CreateExternalKnowledgeBinding,
    ) -> Result<ExternalKnowledgeBindingRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = ExternalKnowledgeBindingRecord::new(
                        request.binding,
                        request.created_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                    insert_binding(transaction, &record).await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        binding_id: Uuid,
    ) -> Result<Option<ExternalKnowledgeBindingRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { load_binding(transaction, organization_id, binding_id).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_for_knowledge_base(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ExternalKnowledgeBindingRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict("ExternalKnowledgeBinding list limit is out of bounds".into())
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<KnowledgeBindingRow, _>(
                        transaction,
                        sql_query::<KnowledgeBindingRow>(SELECT_BINDING)
                            .append(" where organization_id = ")
                            .bind(organization_id)
                            .append(" and knowledge_base_id = ")
                            .bind(knowledge_base_id)
                            .append(" order by binding_id asc limit ")
                            .bind(limit),
                    )
                    .await?;
                    rows.into_iter().map(decode_binding).collect()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_index(
    transaction: &PostgresTransaction,
    record: &KnowledgeIndexRevisionRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.index_revision.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_index_revisions (organization_id, project_id, knowledge_base_revision_id, index_revision_id, index_digest, index_acl, created_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.knowledge_base_revision_id.as_uuid())
            .append(", ")
            .bind(spec.index_revision_id.as_uuid())
            .append(", ")
            .bind(record.index_revision.digest().as_str())
            .append(", ")
            .bind(record.index_revision.canonical_acl())
            .append(", ")
            .bind(record.created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgeIndexRevision", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgeIndexRevision already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn insert_policy(
    transaction: &PostgresTransaction,
    record: &KnowledgeRetrievalPolicyRevisionRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.policy_revision.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_retrieval_policy_revisions (organization_id, project_id, knowledge_base_revision_id, policy_revision_id, policy_digest, policy_acl, created_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.knowledge_base_revision_id.as_uuid())
            .append(", ")
            .bind(spec.policy_revision_id.as_uuid())
            .append(", ")
            .bind(record.policy_revision.digest().as_str())
            .append(", ")
            .bind(record.policy_revision.canonical_acl())
            .append(", ")
            .bind(record.created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgeRetrievalPolicyRevision", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgeRetrievalPolicyRevision already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn insert_binding(
    transaction: &PostgresTransaction,
    record: &ExternalKnowledgeBindingRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.binding.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into external_knowledge_bindings (organization_id, project_id, knowledge_base_id, binding_id, binding_digest, binding_acl, created_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.knowledge_base_id.as_uuid())
            .append(", ")
            .bind(spec.binding_id.as_uuid())
            .append(", ")
            .bind(record.binding.digest().as_str())
            .append(", ")
            .bind(record.binding.canonical_acl())
            .append(", ")
            .bind(record.created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("ExternalKnowledgeBinding", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("ExternalKnowledgeBinding already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn load_index(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    index_revision_id: Uuid,
) -> Result<Option<KnowledgeIndexRevisionRecord>, PostgresPersistenceError> {
    let row = fetch_optional::<KnowledgeIndexRow, _>(
        transaction,
        sql_query::<KnowledgeIndexRow>(SELECT_INDEX)
            .append(" where organization_id = ")
            .bind(organization_id)
            .append(" and index_revision_id = ")
            .bind(index_revision_id),
    )
    .await?;
    row.map(decode_index).transpose()
}

async fn load_policy(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    policy_revision_id: Uuid,
) -> Result<Option<KnowledgeRetrievalPolicyRevisionRecord>, PostgresPersistenceError> {
    let row = fetch_optional::<KnowledgePolicyRow, _>(
        transaction,
        sql_query::<KnowledgePolicyRow>(SELECT_POLICY)
            .append(" where organization_id = ")
            .bind(organization_id)
            .append(" and policy_revision_id = ")
            .bind(policy_revision_id),
    )
    .await?;
    row.map(decode_policy).transpose()
}

async fn load_binding(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    binding_id: Uuid,
) -> Result<Option<ExternalKnowledgeBindingRecord>, PostgresPersistenceError> {
    let row = fetch_optional::<KnowledgeBindingRow, _>(
        transaction,
        sql_query::<KnowledgeBindingRow>(SELECT_BINDING)
            .append(" where organization_id = ")
            .bind(organization_id)
            .append(" and binding_id = ")
            .bind(binding_id),
    )
    .await?;
    row.map(decode_binding).transpose()
}

fn decode_index(row: KnowledgeIndexRow) -> Result<KnowledgeIndexRevisionRecord, PostgresPersistenceError> {
    let index_revision = KnowledgeIndexRevisionV1::restore(&row.index_acl, &row.index_digest)
        .map_err(PostgresPersistenceError::Invariant)?;
    let spec = index_revision.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.project_id.as_uuid() != row.project_id
        || spec.knowledge_base_revision_id.as_uuid() != row.knowledge_base_revision_id
        || spec.index_revision_id.as_uuid() != row.index_revision_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeIndexRevision projection drifted".into(),
        ));
    }
    KnowledgeIndexRevisionRecord::new(index_revision, row.created_at)
        .map_err(PostgresPersistenceError::Invariant)
}

fn decode_policy(
    row: KnowledgePolicyRow,
) -> Result<KnowledgeRetrievalPolicyRevisionRecord, PostgresPersistenceError> {
    let policy_revision =
        KnowledgeRetrievalPolicyRevisionV1::restore(&row.policy_acl, &row.policy_digest)
            .map_err(PostgresPersistenceError::Invariant)?;
    let spec = policy_revision.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.project_id.as_uuid() != row.project_id
        || spec.knowledge_base_revision_id.as_uuid() != row.knowledge_base_revision_id
        || spec.policy_revision_id.as_uuid() != row.policy_revision_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeRetrievalPolicyRevision projection drifted".into(),
        ));
    }
    KnowledgeRetrievalPolicyRevisionRecord::new(policy_revision, row.created_at)
        .map_err(PostgresPersistenceError::Invariant)
}

fn decode_binding(
    row: KnowledgeBindingRow,
) -> Result<ExternalKnowledgeBindingRecord, PostgresPersistenceError> {
    let binding = ExternalKnowledgeBindingV1::restore(&row.binding_acl, &row.binding_digest)
        .map_err(PostgresPersistenceError::Invariant)?;
    let spec = binding.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.project_id.as_uuid() != row.project_id
        || spec.knowledge_base_id.as_uuid() != row.knowledge_base_id
        || spec.binding_id.as_uuid() != row.binding_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "ExternalKnowledgeBinding projection drifted".into(),
        ));
    }
    ExternalKnowledgeBindingRecord::new(binding, row.created_at)
        .map_err(PostgresPersistenceError::Invariant)
}

struct KnowledgeIndexRow {
    organization_id: Uuid,
    project_id: Uuid,
    knowledge_base_revision_id: Uuid,
    index_revision_id: Uuid,
    index_digest: String,
    index_acl: String,
    created_at: DateTime<Utc>,
}

impl FromRow for KnowledgeIndexRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            knowledge_base_revision_id: decode(row, 2)?,
            index_revision_id: decode(row, 3)?,
            index_digest: decode(row, 4)?,
            index_acl: decode(row, 5)?,
            created_at: decode(row, 6)?,
        })
    }
}

struct KnowledgePolicyRow {
    organization_id: Uuid,
    project_id: Uuid,
    knowledge_base_revision_id: Uuid,
    policy_revision_id: Uuid,
    policy_digest: String,
    policy_acl: String,
    created_at: DateTime<Utc>,
}

impl FromRow for KnowledgePolicyRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            knowledge_base_revision_id: decode(row, 2)?,
            policy_revision_id: decode(row, 3)?,
            policy_digest: decode(row, 4)?,
            policy_acl: decode(row, 5)?,
            created_at: decode(row, 6)?,
        })
    }
}

struct KnowledgeBindingRow {
    organization_id: Uuid,
    project_id: Uuid,
    knowledge_base_id: Uuid,
    binding_id: Uuid,
    binding_digest: String,
    binding_acl: String,
    created_at: DateTime<Utc>,
}

impl FromRow for KnowledgeBindingRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            knowledge_base_id: decode(row, 2)?,
            binding_id: decode(row, 3)?,
            binding_digest: decode(row, 4)?,
            binding_acl: decode(row, 5)?,
            created_at: decode(row, 6)?,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn index_policy_binding_repositories_restore_acl_through_contract_parser() {
        let source = include_str!("knowledge_index_postgres.rs");
        assert!(source.contains("KnowledgeIndexRevisionV1::restore"));
        assert!(source.contains("KnowledgeRetrievalPolicyRevisionV1::restore"));
        assert!(source.contains("ExternalKnowledgeBindingV1::restore"));
    }
}
