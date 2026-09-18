//! Minimal parent surface for the A0.4 real Box Agent release path.
//!
//! Excludes Agent Code recovery scenario/approval suites so this focused
//! integration binary does not compile unrelated broken support.

#![allow(unused_imports)]

use crate::migrate_and_connect_for_test;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCommandAck, NodeCommandEnvelope, NodeCommandLeaseRequest,
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodeHeartbeat, NodeObservationBatch,
    RuntimeObservationReport, RuntimeServiceEndpoint,
};
#[cfg(feature = "persistence-conformance")]
use a3s_cloud_control_plane::conformance::workload_organization_access_for_conformance;
use a3s_cloud_control_plane::modules::agents::{
    AssetsAgentReleaseAdmissionAdapter, NATIVE_CODE_AGENT_PROVIDER_KIND,
};
use a3s_cloud_control_plane::modules::artifacts::{
    HostedArtifactQueryService, PostgresBuildRunRepository,
};
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetCreated, AssetKind, AssetRelease, AssetReleaseDrafted, AssetReleaseVersion,
    CreateAssetReleaseWrite, CreateAssetWrite, HostedAssetBuildRequested, IAssetRepository,
    PostgresAssetRepository,
};
use a3s_cloud_control_plane::modules::fleet::domain::entities::{
    EnrollmentToken, NodeCommandDraft,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::secrets::PostgresSecretRepository;
use a3s_cloud_control_plane::modules::shared_kernel::application::{
    ApplicationError, ApplicationResult,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, AssetId, AssetReleaseId, AuthorizationDecisionRef,
    EnrollmentTokenId, EnvironmentId, GitCommitSha, IdempotencyRequest, NodeCommandId, NodeId,
    OrganizationId, PrincipalId, ProjectId, RepositoryError, ResourceName, Sha256Digest,
};
use a3s_cloud_control_plane::modules::workloads::{
    project_runtime_spec, CreateAgentWorkloadDeployment, CreateAgentWorkloadDeploymentHandler,
    Deployment, DeploymentReplicaBinding, FleetWorkloadsNodePoolAccessAdapter, IWorkloadRepository,
    IWorkloadRuntimeTargetRepository, PostgresWorkloadRepository,
    ProjectsWorkloadsEnvironmentAccessAdapter, SecretsWorkloadsSecretBindingAccessAdapter,
    ServiceProcess, ServiceResources, SourceWorkloadTemplate, Workload, WorkloadRevision,
};
use a3s_orm::{
    sql_query, Database, DatabaseError, PostgresDialect, PostgresError, PostgresExecutor,
};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, NetworkMode, ResourceControl, RuntimeApplyRequest,
    RuntimeCapabilities, RuntimeEvidence, RuntimeFeature, RuntimeHealthObservation,
    RuntimeHealthState, RuntimeObservation, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
    TransportProtocol,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn idempotency(scope: &str, key: &str, body: &[u8]) -> Result<IdempotencyRequest, String> {
    IdempotencyRequest::new(scope, key, body)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    value - Duration::nanoseconds(i64::from(value.nanosecond() % 1_000))
}

async fn insert_scope(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    created_at: DateTime<Utc>,
) -> TestResult {
    let scope_suffix = organization_id.as_uuid().simple().to_string();
    let organization_name = format!("Agent recovery tenant {scope_suffix}");
    let organization_name_key = format!("agent-recovery-tenant-{scope_suffix}");
    let project_name = format!("Agent recovery project {scope_suffix}");
    let project_name_key = format!("agent-recovery-project-{scope_suffix}");
    let environment_name = format!("Agent recovery environment {scope_suffix}");
    let environment_name_key = format!("agent-recovery-environment-{scope_suffix}");
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(organization_name)
            .append(", ")
            .bind(organization_name_key)
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", ")
            .bind(project_name)
            .append(", ")
            .bind(project_name_key)
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", ")
            .bind(environment_id.as_uuid())
            .append(", ")
            .bind(environment_name)
            .append(", ")
            .bind(environment_name_key)
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    Ok(())
}

async fn enroll_node(
    nodes: &PostgresNodeRepository,
    organization_id: OrganizationId,
    proposed_node_id: NodeId,
    capabilities: &RuntimeCapabilities,
    enrolled_at: DateTime<Utc>,
) -> TestResult<(NodeId, Uuid)> {
    let token_id = EnrollmentTokenId::new();
    let secret = format!("a3sn_{}", token_id.as_uuid().simple().to_string().repeat(2));
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    let token = EnrollmentToken::new(
        token_id,
        organization_id,
        "Agent recovery worker",
        credential.clone(),
        enrolled_at,
        enrolled_at + Duration::minutes(5),
    )?;
    nodes
        .issue_enrollment_token(
            token.clone(),
            DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "fleet.enrollment-token.issued".into(),
                schema_version: 1,
                scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                    organization_id: organization_id.as_uuid(),
                },
                aggregate_id: token.id.as_uuid(),
                aggregate_version: token.aggregate_version,
                occurred_at: token.created_at,
                correlation_id: Uuid::now_v7(),
                causation_id: None,
                payload: json!({"name": token.name}),
            },
            idempotency(
                &format!(
                    "test.agent-code-recovery.organizations/{organization_id}/enrollment"
                ),
                "issue-node-token",
                b"issue-node-token",
            )?,
        )
        .await?;
    let stored_capabilities = NodeCapabilities::new(
        capabilities.provider_id.to_string(),
        capabilities.provider_build.clone(),
        serde_json::to_value(capabilities)?,
    )?;
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id,
                name: NodeName::new("agent-recovery-worker")?,
                agent_instance_id,
                agent_version: "0.1.0-test".into(),
                capabilities: stored_capabilities.clone(),
                request_digest: format!("sha256:{}", "c".repeat(64)),
                requested_at: enrolled_at,
            },
        )
        .await?;
    if reservation.node.id != proposed_node_id {
        return Err(invalid("Fleet enrollment changed the proposed node identity").into());
    }
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0-test".into(),
            capabilities: stored_capabilities,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id))
}

fn agent_runtime_template() -> SourceWorkloadTemplate {
    SourceWorkloadTemplate {
        process: ServiceProcess {
            command: Vec::new(),
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 250,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: Some(64 * 1024 * 1024),
        },
        ports: Vec::new(),
        health: None,
    }
}

#[cfg(target_os = "linux")]
#[path = "agent_code_recovery/real_box_release.rs"]
mod real_box_release;

#[cfg(target_os = "linux")]
pub use real_box_release::{LiveAgentProbe, LiveAgentWindow};

#[cfg(target_os = "linux")]
pub async fn exercise_published_agent_release_real_box(postgres_url: String) -> TestResult {
    real_box_release::exercise(postgres_url).await
}

#[cfg(target_os = "linux")]
pub async fn exercise_published_agent_release_real_box_with_live_probe(
    postgres_url: String,
    live: LiveAgentProbe,
) -> TestResult {
    real_box_release::exercise_with_live_probe(postgres_url, live).await
}
