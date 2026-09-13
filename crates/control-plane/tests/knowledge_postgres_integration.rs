//! Focused K0.1-C4a/C5/C11 PostgreSQL recovery gates for Knowledge catalogs.
//!
//! Kept as a separate integration binary so KnowledgeBase/Pipeline catalog
//! certification does not depend on unrelated postgres_integration support
//! modules that currently drift from production types.

use a3s_cloud_control_plane::infrastructure::{
    connect_postgres, migrate_postgres, PostgresBootstrapError, PostgresMigrationReport,
};
use a3s_cloud_control_plane::modules::knowledge::{
    AppendKnowledgeBaseRevision, CreateExternalKnowledgeBinding, CreateKnowledgeBase,
    CreateKnowledgeChunk, CreateKnowledgeDocument, CreateKnowledgeIndexRevision,
    CreateKnowledgePipeline, CreateKnowledgeRetrievalPolicyRevision,
    ExternalKnowledgeBindingV1, IExternalKnowledgeBindingRepository, IKnowledgeBaseRepository,
    IKnowledgeChunkRepository, IKnowledgeDocumentRepository, IKnowledgeIndexRevisionRepository,
    IKnowledgePipelineRepository, IKnowledgeRetrievalPolicyRevisionRepository,
    KnowledgeBaseRevisionV1, KnowledgeChunkV1, KnowledgeDocumentV1, KnowledgeIndexRevisionV1,
    KnowledgePipelineReleaseV1, KnowledgeRetrievalPolicyRevisionV1,
    PostgresExternalKnowledgeBindingRepository, PostgresKnowledgeBaseRepository,
    PostgresKnowledgeChunkRepository, PostgresKnowledgeDocumentRepository,
    PostgresKnowledgeIndexRevisionRepository, PostgresKnowledgePipelineRepository,
    PostgresKnowledgeRetrievalPolicyRevisionRepository, PublishKnowledgePipelineRelease,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    KnowledgeBaseRevisionId, KnowledgePipelineReleaseId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use uuid::Uuid;

const BASE_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-base-revision.acl"
));
const PIPELINE_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-pipeline-release.acl"
));
const DOCUMENT_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-document.acl"
));
const CHUNK_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-chunk.acl"
));
const INDEX_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-index-revision.acl"
));
const POLICY_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-retrieval-policy-revision.acl"
));
const BINDING_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/external-knowledge-binding.acl"
));

#[tokio::test]
async fn knowledge_base_catalog_postgres_reconnects_and_fences_revision_head() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_knowledge_base_catalog_postgres)
        .await
        .expect("KnowledgeBase catalog PostgreSQL recovery gate");
}

#[tokio::test]
async fn knowledge_pipeline_catalog_postgres_reconnects_and_fences_release_head() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_knowledge_pipeline_catalog_postgres)
        .await
        .expect("KnowledgePipeline catalog PostgreSQL recovery gate");
}


#[tokio::test]
async fn knowledge_index_policy_binding_catalog_postgres_reconnects() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_knowledge_index_policy_binding_catalog_postgres)
        .await
        .expect("Knowledge index/policy/binding catalog PostgreSQL recovery gate");
}

#[tokio::test]
async fn knowledge_document_and_chunk_catalog_postgres_reconnects() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(
        &admin_url,
        exercise_knowledge_document_chunk_catalog_postgres,
    )
    .await
    
.map_err(|e| { eprintln!("DOCUMENT_CHUNK_GATE_ERR: {e:?}"); e })
.expect("KnowledgeDocument/Chunk catalog PostgreSQL recovery gate");
}

async fn exercise_knowledge_base_catalog_postgres(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000201")?;
    let project_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000202")?;
    seed_knowledge_scope(&database, organization_id, project_id).await?;

    let revision =
        KnowledgeBaseRevisionV1::parse_acl(BASE_FIXTURE).map_err(std::io::Error::other)?;
    let repository = Arc::new(PostgresKnowledgeBaseRepository::new(executor.clone()));
    let created = repository
        .create(CreateKnowledgeBase {
            revision: revision.clone(),
            created_at: knowledge_timestamp(1_000),
        })
        .await?;
    assert_eq!(repository.list(1).await?.len(), 1);
    assert!(repository.list(0).await?.is_empty());
    assert_eq!(created.revision, revision);

    let mut successor_spec = revision.spec().clone();
    successor_spec.generation = 2;
    successor_spec.revision_id =
        KnowledgeBaseRevisionId::from_uuid(Uuid::from_u128(0x018f0000000070008000000000000303));
    successor_spec.name = "Product FAQ v2".into();
    let successor =
        KnowledgeBaseRevisionV1::from_spec(successor_spec).map_err(std::io::Error::other)?;
    let append = AppendKnowledgeBaseRevision {
        organization_id,
        knowledge_base_id: revision.spec().knowledge_base_id.as_uuid(),
        expected_revision_digest: revision.digest().as_str().to_string(),
        revision: successor.clone(),
        updated_at: knowledge_timestamp(1_001),
    };
    let (first, second) = tokio::join!(
        repository.append_revision(append.clone()),
        repository.append_revision(append),
    );
    let updated = match (first, second) {
        (Ok(updated), Err(RepositoryError::Conflict(message)))
        | (Err(RepositoryError::Conflict(message)), Ok(updated)) => {
            assert!(message.contains("stale"), "unexpected CAS error: {message}");
            updated
        }
        (first, second) => {
            return Err(format!(
                "exactly one KnowledgeBase head append must win: {first:?}; {second:?}"
            )
            .into())
        }
    };
    assert_eq!(updated.revision, successor);
    assert_eq!(updated.revision.spec().generation, 2);

    drop(repository);
    drop(database);
    drop(executor);

    let recovered_executor = connect_postgres(&url, 8).await?;
    let recovered = PostgresKnowledgeBaseRepository::new(recovered_executor);
    let recovered_head = recovered
        .find(
            organization_id,
            updated.revision.spec().knowledge_base_id.as_uuid(),
        )
        .await?
        .expect("KnowledgeBase head after reconnect");
    assert_eq!(recovered_head.revision, successor);
    assert_eq!(
        recovered
            .find_revision(
                organization_id,
                updated.revision.spec().knowledge_base_id.as_uuid(),
                revision.spec().revision_id.as_uuid(),
            )
            .await?
            .expect("historical revision after reconnect"),
        revision
    );
    assert!(recovered
        .find(
            Uuid::from_u128(organization_id.as_u128() ^ 1),
            updated.revision.spec().knowledge_base_id.as_uuid(),
        )
        .await?
        .is_none());
    Ok(())
}

async fn exercise_knowledge_pipeline_catalog_postgres(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000201")?;
    let project_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000202")?;
    seed_knowledge_scope(&database, organization_id, project_id).await?;

    let release =
        KnowledgePipelineReleaseV1::parse_acl(PIPELINE_FIXTURE).map_err(std::io::Error::other)?;
    let repository = Arc::new(PostgresKnowledgePipelineRepository::new(executor.clone()));
    let created = repository
        .create(CreateKnowledgePipeline {
            release: release.clone(),
            created_at: knowledge_timestamp(1_000),
        })
        .await?;
    assert_eq!(created.release, release);

    let mut successor_spec = release.spec().clone();
    successor_spec.release_id =
        KnowledgePipelineReleaseId::from_uuid(Uuid::from_u128(0x018f0000000070008000000000000310));
    successor_spec.name = "FAQ ingest v2".into();
    let successor =
        KnowledgePipelineReleaseV1::from_spec(successor_spec).map_err(std::io::Error::other)?;
    let publish = PublishKnowledgePipelineRelease {
        organization_id,
        pipeline_id: release.spec().pipeline_id.as_uuid(),
        expected_release_digest: release.digest().as_str().to_string(),
        release: successor.clone(),
        updated_at: knowledge_timestamp(1_001),
    };
    let (first, second) = tokio::join!(
        repository.publish_release(publish.clone()),
        repository.publish_release(publish),
    );
    let updated = match (first, second) {
        (Ok(updated), Err(RepositoryError::Conflict(message)))
        | (Err(RepositoryError::Conflict(message)), Ok(updated)) => {
            assert!(message.contains("stale"), "unexpected CAS error: {message}");
            updated
        }
        (first, second) => {
            return Err(format!(
                "exactly one KnowledgePipeline head publish must win: {first:?}; {second:?}"
            )
            .into())
        }
    };
    assert_eq!(updated.release, successor);

    drop(repository);
    drop(database);
    drop(executor);

    let recovered_executor = connect_postgres(&url, 8).await?;
    let recovered = PostgresKnowledgePipelineRepository::new(recovered_executor);
    let recovered_head = recovered
        .find(
            organization_id,
            updated.release.spec().pipeline_id.as_uuid(),
        )
        .await?
        .expect("KnowledgePipeline head after reconnect");
    assert_eq!(recovered_head.release, successor);
    assert_eq!(
        recovered
            .find_release(
                organization_id,
                updated.release.spec().pipeline_id.as_uuid(),
                release.spec().release_id.as_uuid(),
            )
            .await?
            .expect("historical release after reconnect"),
        release
    );
    Ok(())
}

async fn exercise_knowledge_document_chunk_catalog_postgres(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000201")?;
    let project_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000202")?;
    seed_knowledge_scope(&database, organization_id, project_id).await?;

    let revision =
        KnowledgeBaseRevisionV1::parse_acl(BASE_FIXTURE).map_err(std::io::Error::other)?;
    let bases = PostgresKnowledgeBaseRepository::new(executor.clone());
    bases
        .create(CreateKnowledgeBase {
            revision: revision.clone(),
            created_at: knowledge_timestamp(1_000),
        })
        .await?;

    let document =
        KnowledgeDocumentV1::parse_acl(DOCUMENT_FIXTURE).map_err(std::io::Error::other)?;
    let documents = Arc::new(PostgresKnowledgeDocumentRepository::new(executor.clone()));
    let created_document = documents
        .create(CreateKnowledgeDocument {
            document: document.clone(),
            created_at: knowledge_timestamp(1_010),
        })
        .await?;
    assert_eq!(created_document.document, document);
    let duplicate = documents
        .create(CreateKnowledgeDocument {
            document: document.clone(),
            created_at: knowledge_timestamp(1_011),
        })
        .await;
    assert!(matches!(duplicate, Err(RepositoryError::Conflict(_))));
    assert_eq!(
        documents
            .list_for_knowledge_base(
                organization_id,
                document.spec().knowledge_base_id.as_uuid(),
                8,
            )
            .await?
            .len(),
        1
    );

    let chunk = KnowledgeChunkV1::parse_acl(CHUNK_FIXTURE).map_err(std::io::Error::other)?;
    let chunks = Arc::new(PostgresKnowledgeChunkRepository::new(executor.clone()));
    let created_chunk = chunks
        .create(CreateKnowledgeChunk {
            chunk: chunk.clone(),
            created_at: knowledge_timestamp(1_020),
        })
        .await?;
    assert_eq!(created_chunk.chunk, chunk);
    assert_eq!(
        chunks
            .list_for_document(organization_id, document.spec().document_id.as_uuid(), 8)
            .await?
            .len(),
        1
    );

    drop(chunks);
    drop(documents);
    drop(bases);
    drop(database);
    drop(executor);

    let recovered_executor = connect_postgres(&url, 8).await?;
    let recovered_documents = PostgresKnowledgeDocumentRepository::new(recovered_executor.clone());
    let recovered_chunks = PostgresKnowledgeChunkRepository::new(recovered_executor);
    let recovered_document = recovered_documents
        .find(organization_id, document.spec().document_id.as_uuid())
        .await?
        .expect("KnowledgeDocument after reconnect");
    assert_eq!(recovered_document.document, document);
    let recovered_chunk = recovered_chunks
        .find(organization_id, chunk.spec().chunk_id.as_uuid())
        .await?
        .expect("KnowledgeChunk after reconnect");
    assert_eq!(recovered_chunk.chunk, chunk);
    assert!(recovered_documents
        .find(
            Uuid::from_u128(organization_id.as_u128() ^ 1),
            document.spec().document_id.as_uuid(),
        )
        .await?
        .is_none());
    Ok(())
}

async fn exercise_knowledge_index_policy_binding_catalog_postgres(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000201")?;
    let project_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000202")?;
    seed_knowledge_scope(&database, organization_id, project_id).await?;

    let revision =
        KnowledgeBaseRevisionV1::parse_acl(BASE_FIXTURE).map_err(std::io::Error::other)?;
    let bases = PostgresKnowledgeBaseRepository::new(executor.clone());
    bases
        .create(CreateKnowledgeBase {
            revision: revision.clone(),
            created_at: knowledge_timestamp(1_000),
        })
        .await?;

    let index =
        KnowledgeIndexRevisionV1::parse_acl(INDEX_FIXTURE).map_err(std::io::Error::other)?;
    let indexes = Arc::new(PostgresKnowledgeIndexRevisionRepository::new(executor.clone()));
    let created_index = indexes
        .create(CreateKnowledgeIndexRevision {
            index_revision: index.clone(),
            created_at: knowledge_timestamp(1_010),
        })
        .await?;
    assert_eq!(created_index.index_revision, index);
    let duplicate_index = indexes
        .create(CreateKnowledgeIndexRevision {
            index_revision: index.clone(),
            created_at: knowledge_timestamp(1_011),
        })
        .await;
    assert!(matches!(duplicate_index, Err(RepositoryError::Conflict(_))));
    assert_eq!(
        indexes
            .list_for_knowledge_base_revision(
                organization_id,
                index.spec().knowledge_base_revision_id.as_uuid(),
                8,
            )
            .await?
            .len(),
        1
    );

    let policy = KnowledgeRetrievalPolicyRevisionV1::parse_acl(POLICY_FIXTURE)
        .map_err(std::io::Error::other)?;
    let policies = Arc::new(PostgresKnowledgeRetrievalPolicyRevisionRepository::new(
        executor.clone(),
    ));
    let created_policy = policies
        .create(CreateKnowledgeRetrievalPolicyRevision {
            policy_revision: policy.clone(),
            created_at: knowledge_timestamp(1_020),
        })
        .await?;
    assert_eq!(created_policy.policy_revision, policy);
    let duplicate_policy = policies
        .create(CreateKnowledgeRetrievalPolicyRevision {
            policy_revision: policy.clone(),
            created_at: knowledge_timestamp(1_021),
        })
        .await;
    assert!(matches!(duplicate_policy, Err(RepositoryError::Conflict(_))));
    assert_eq!(
        policies
            .list_for_knowledge_base_revision(
                organization_id,
                policy.spec().knowledge_base_revision_id.as_uuid(),
                8,
            )
            .await?
            .len(),
        1
    );

    let binding =
        ExternalKnowledgeBindingV1::parse_acl(BINDING_FIXTURE).map_err(std::io::Error::other)?;
    let bindings = Arc::new(PostgresExternalKnowledgeBindingRepository::new(executor.clone()));
    let created_binding = bindings
        .create(CreateExternalKnowledgeBinding {
            binding: binding.clone(),
            created_at: knowledge_timestamp(1_030),
        })
        .await?;
    assert_eq!(created_binding.binding, binding);
    let duplicate_binding = bindings
        .create(CreateExternalKnowledgeBinding {
            binding: binding.clone(),
            created_at: knowledge_timestamp(1_031),
        })
        .await;
    assert!(matches!(duplicate_binding, Err(RepositoryError::Conflict(_))));
    assert_eq!(
        bindings
            .list_for_knowledge_base(
                organization_id,
                binding.spec().knowledge_base_id.as_uuid(),
                8,
            )
            .await?
            .len(),
        1
    );

    drop(bindings);
    drop(policies);
    drop(indexes);
    drop(bases);
    drop(database);
    drop(executor);

    let recovered_executor = connect_postgres(&url, 8).await?;
    let recovered_indexes =
        PostgresKnowledgeIndexRevisionRepository::new(recovered_executor.clone());
    let recovered_policies =
        PostgresKnowledgeRetrievalPolicyRevisionRepository::new(recovered_executor.clone());
    let recovered_bindings =
        PostgresExternalKnowledgeBindingRepository::new(recovered_executor);
    let recovered_index = recovered_indexes
        .find(organization_id, index.spec().index_revision_id.as_uuid())
        .await?
        .expect("KnowledgeIndexRevision after reconnect");
    assert_eq!(recovered_index.index_revision, index);
    let recovered_policy = recovered_policies
        .find(organization_id, policy.spec().policy_revision_id.as_uuid())
        .await?
        .expect("KnowledgeRetrievalPolicyRevision after reconnect");
    assert_eq!(recovered_policy.policy_revision, policy);
    let recovered_binding = recovered_bindings
        .find(organization_id, binding.spec().binding_id.as_uuid())
        .await?
        .expect("ExternalKnowledgeBinding after reconnect");
    assert_eq!(recovered_binding.binding, binding);
    assert!(recovered_indexes
        .find(
            Uuid::from_u128(organization_id.as_u128() ^ 1),
            index.spec().index_revision_id.as_uuid(),
        )
        .await?
        .is_none());
    Ok(())
}

fn knowledge_timestamp(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("canonical timestamp")
}

async fn seed_knowledge_scope(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: Uuid,
    project_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", 'Knowledge integration organization', 'knowledge-integration-organization', 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", ")
            .bind(project_id)
            .append(", 'Knowledge integration project', 'knowledge-integration-project', 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    Ok(())
}

async fn migrate_and_connect_for_test(
    url: &str,
    max_connections: usize,
) -> Result<PostgresExecutor, PostgresBootstrapError> {
    migrate_for_test(url, max_connections).await?;
    connect_postgres(url, max_connections).await
}

async fn migrate_for_test(
    url: &str,
    max_connections: usize,
) -> Result<PostgresMigrationReport, PostgresBootstrapError> {
    let serving_role = serving_role_for_url(url)?;
    migrate_postgres(url, max_connections, &serving_role).await
}

fn serving_role_for_url(database_url: &str) -> Result<String, PostgresBootstrapError> {
    let parsed = url::Url::parse(database_url).map_err(|error| {
        PostgresBootstrapError::ComponentConfiguration(format!(
            "invalid isolated PostgreSQL URL: {error}"
        ))
    })?;
    let database_name = parsed.path().trim_start_matches('/');
    if database_name.is_empty() || database_name.contains('/') {
        return Err(PostgresBootstrapError::ComponentConfiguration(
            "isolated PostgreSQL URL must name exactly one database".into(),
        ));
    }
    Ok(format!("{database_name}_serving"))
}

struct IsolatedPostgresDatabase {
    admin_url: String,
    database_name: String,
    database_url: String,
    serving_role: String,
}

impl IsolatedPostgresDatabase {
    async fn create(admin_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let database_name = format!("a3s_cloud_test_{}", Uuid::new_v4().simple());
        let mut database_url = url::Url::parse(admin_url)?;
        database_url.set_path(&format!("/{database_name}"));
        let serving_role = format!("{database_name}_serving");

        let admin = PostgresExecutor::connect_no_tls(admin_url, 2)?;
        let connection = admin.pool().get().await?;
        connection
            .batch_execute(&format!("create database \"{database_name}\""))
            .await?;
        if let Err(source) = connection
            .batch_execute(&format!(
                "create role \"{serving_role}\" nologin nosuperuser nocreatedb nocreaterole noreplication"
            ))
            .await
        {
            let cleanup = connection
                .batch_execute(&format!(
                    "drop database if exists \"{database_name}\" with (force)"
                ))
                .await;
            return match cleanup {
                Ok(()) => Err(source.into()),
                Err(cleanup_error) => Err(std::io::Error::other(format!(
                    "could not create isolated serving role: {source}; database cleanup also failed: {cleanup_error}"
                ))
                .into()),
            };
        }

        Ok(Self {
            admin_url: admin_url.to_owned(),
            database_name,
            database_url: database_url.to_string(),
            serving_role,
        })
    }

    fn url(&self) -> &str {
        &self.database_url
    }

    async fn cleanup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let admin = PostgresExecutor::connect_no_tls(&self.admin_url, 2)?;
        let connection = admin.pool().get().await?;
        let database_cleanup = connection
            .batch_execute(&format!(
                "drop database if exists \"{}\" with (force)",
                self.database_name
            ))
            .await;
        let role_cleanup = connection
            .batch_execute(&format!("drop role if exists \"{}\"", self.serving_role))
            .await;
        match (database_cleanup, role_cleanup) {
            (Ok(()), Ok(())) => {}
            (Err(database_error), Ok(())) => return Err(database_error.into()),
            (Ok(()), Err(role_error)) => return Err(role_error.into()),
            (Err(database_error), Err(role_error)) => {
                return Err(std::io::Error::other(format!(
                    "isolated database cleanup failed: {database_error}; role cleanup also failed: {role_error}"
                ))
                .into());
            }
        }
        let row = connection
            .query_one(
                "select exists(select 1 from pg_database where datname = $1), exists(select 1 from pg_roles where rolname = $2)",
                &[&self.database_name, &self.serving_role],
            )
            .await?;
        let database_still_exists: bool = row.get(0);
        let role_still_exists: bool = row.get(1);
        if database_still_exists || role_still_exists {
            return Err(std::io::Error::other(format!(
                "isolated PostgreSQL resources still exist after cleanup (database={}, role={})",
                self.database_name, self.serving_role
            ))
            .into());
        }
        Ok(())
    }
}

async fn run_isolated_postgres<F, Fut>(
    admin_url: &str,
    exercise: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    let isolated = IsolatedPostgresDatabase::create(admin_url).await?;
    let result = AssertUnwindSafe(Box::pin(exercise(isolated.url().to_owned())))
        .catch_unwind()
        .await;
    let cleanup = isolated.cleanup().await;

    match result {
        Ok(Ok(())) => cleanup,
        Ok(Err(test_error)) => {
            if let Err(cleanup_error) = cleanup {
                return Err(std::io::Error::other(format!(
                    "PostgreSQL integration test failed: {test_error}; isolated database cleanup also failed: {cleanup_error}"
                ))
                .into());
            }
            Err(test_error)
        }
        Err(panic_payload) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!(
                    "isolated PostgreSQL database cleanup failed after test panic: {cleanup_error}"
                );
            }
            std::panic::resume_unwind(panic_payload)
        }
    }
}
