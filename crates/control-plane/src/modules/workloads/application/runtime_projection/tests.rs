use super::*;
use crate::modules::artifacts::application::project_hosted_build_outcome;
use crate::modules::artifacts::domain::test_support::succeeded_hosted_build;
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseVersion, McpServiceProfile,
    McpServiceProfileBinding, McpServiceProfileSpec,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, BuildRunId, EnvironmentId, GitCommitSha,
    OrganizationId, ProjectId, ResourceName, SecretId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    AgentWorkloadRevisionBinding, HttpHealthCheck, OciArtifact, SecretBinding, SecretBindingTarget,
    ServicePort, ServiceProcess, ServiceResources, ServiceTemplate, SkillWorkloadRevisionBinding,
    Workload,
};
use a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;

#[test]
fn projects_digest_bound_service_without_provider_fields() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let revision_id = WorkloadRevisionId::new();
    let secret_id = SecretId::new();
    let mut revision = WorkloadRevision::create(
        revision_id,
        WorkloadId::new(),
        3,
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!("oci://registry.example/fixture@{digest}"),
                digest: digest.clone(),
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ServiceProcess {
                command: Vec::new(),
                args: vec!["serve".into()],
                working_directory: None,
                environment: BTreeMap::new(),
            },
            secrets: vec![
                SecretBinding {
                    name: "api-token".into(),
                    secret_id,
                    version: 4,
                    target: SecretBindingTarget::Environment {
                        variable: "API_TOKEN".into(),
                    },
                },
                SecretBinding {
                    name: "registry".into(),
                    secret_id,
                    version: 5,
                    target: SecretBindingTarget::RegistryCredential,
                },
            ],
            resources: ServiceResources {
                cpu_millis: 250,
                memory_bytes: 64 * 1024 * 1024,
                pids: 64,
                ephemeral_storage_bytes: None,
            },
            ports: vec![ServicePort {
                name: "http".into(),
                container_port: 8080,
            }],
            health: Some(HttpHealthCheck {
                port_name: "http".into(),
                path: "/health".into(),
                interval_ms: 1_000,
                timeout_ms: 500,
                healthy_threshold: 2,
                unhealthy_threshold: 3,
                stabilization_window_ms: 5_000,
            }),
        },
        Utc::now(),
    )
    .expect("revision");
    let spec = project_runtime_spec(&revision).expect("Runtime spec");
    assert_eq!(
        spec.unit_id,
        format!("workload:{}:revision:{}", revision.workload_id, revision.id)
    );
    assert_eq!(spec.generation, 3);
    assert_eq!(spec.artifact.digest, digest);
    assert_eq!(spec.class, RuntimeUnitClass::Service);
    assert_eq!(spec.isolation, IsolationLevel::Sandbox);
    assert!(spec.health.is_some());
    assert_eq!(spec.secrets.len(), 2);
    assert_eq!(
        CloudSecretReference::parse(&spec.secrets[0].reference).expect("Secret reference"),
        CloudSecretReference::new(revision_id.as_uuid(), secret_id.as_uuid(), 4)
            .expect("expected Secret reference")
    );
    assert_eq!(spec.secrets[1].target, SecretTarget::RegistryCredential);
    assert!(spec.mounts.is_empty());
    assert!(spec.semantics_profile_digest.is_none());

    let skill_asset_id = AssetId::new();
    let skill_release_id = AssetReleaseId::new();
    let organization_id = OrganizationId::new();
    let skill_digest =
        Sha256Digest::parse(format!("sha256:{}", "f".repeat(64))).expect("Skill digest");
    revision
        .restore_agent_binding(
            AgentWorkloadRevisionBinding::restore(
                organization_id,
                AssetId::new(),
                AssetReleaseId::new(),
                BuildRunId::new(),
            )
            .expect("Agent binding"),
        )
        .expect("restore Agent binding");
    revision
        .restore_skill_binding(
            SkillWorkloadRevisionBinding::restore(
                organization_id,
                skill_asset_id,
                skill_release_id,
                skill_digest.clone(),
                4096,
            )
            .expect("Skill binding"),
        )
        .expect("restore Skill binding");
    let bound_spec = project_runtime_spec(&revision).expect("Skill-bound Runtime spec");
    assert_eq!(bound_spec.mounts.len(), 1);
    let mount = &bound_spec.mounts[0];
    assert_eq!(mount.name, format!("skill-{skill_asset_id}"));
    assert_eq!(mount.target, format!("/a3s/skills/{skill_asset_id}"));
    assert!(mount.read_only);
    match &mount.source {
        RuntimeMountSource::Artifact { artifact } => {
            assert_eq!(artifact.digest, skill_digest.as_str());
            assert_eq!(
                artifact.media_type,
                a3s_cloud_contracts::SKILL_BUNDLE_MEDIA_TYPE
            );
            assert_eq!(
                artifact.uri,
                a3s_cloud_contracts::artifact_uri(skill_digest.as_str())
                    .expect("Skill Artifact URI")
            );
        }
        source => panic!("unexpected Skill mount source: {source:?}"),
    }
    assert!(bound_spec.outputs.is_empty());
}

#[test]
fn projects_an_opaque_semantics_profile_without_product_fields() {
    let artifact_digest = format!("sha256:{}", "e".repeat(64));
    let created_at = canonical_timestamp(Utc::now());
    let organization_id = OrganizationId::new();
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("runtime-mcp").expect("asset name"),
        AssetKind::Mcp,
        created_at,
    )
    .expect("asset");
    let mut release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0").expect("release version"),
        GitCommitSha::parse("a".repeat(40)).expect("commit"),
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("manifest digest"),
        created_at,
    )
    .expect("release");
    let build = succeeded_hosted_build(organization_id, asset.id, release.id, created_at);
    let outcome = project_hosted_build_outcome(&build)
        .expect("project hosted outcome")
        .expect("successful hosted outcome");
    release
        .publish_from_hosted_build(&asset, &outcome)
        .expect("publish from hosted BuildRun");
    let profile = McpServiceProfile::from_spec(McpServiceProfileSpec {
        protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
        endpoint_path: "/mcp".into(),
        runtime_port: "mcp".into(),
        health_path: "/health".into(),
        request_sse: true,
        subscriptions: true,
        server_discover: true,
        expected_capabilities: vec!["subscriptions".into(), "tools".into()],
        max_request_bytes: 1_048_576,
        max_response_bytes: 8_388_608,
        max_stream_seconds: 3_600,
    })
    .expect("profile");
    let profile_binding = McpServiceProfileBinding {
        organization_id,
        asset_id: asset.id,
        asset_release_id: release.id,
        profile: profile.clone(),
        created_at: created_at + Duration::seconds(2),
    };
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("runtime-mcp-workload").expect("workload name"),
        created_at + Duration::seconds(3),
    );
    let mut revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        3,
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!("oci://registry.example/mcp-fixture@{artifact_digest}"),
                digest: artifact_digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ServiceProcess {
                command: vec!["/app/service".into()],
                args: vec!["serve".into()],
                working_directory: Some("/app".into()),
                environment: BTreeMap::new(),
            },
            secrets: Vec::new(),
            resources: ServiceResources {
                cpu_millis: 500,
                memory_bytes: 256 * 1024 * 1024,
                pids: 128,
                ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
            },
            ports: vec![ServicePort {
                name: "mcp".into(),
                container_port: 8080,
            }],
            health: Some(HttpHealthCheck {
                port_name: "mcp".into(),
                path: "/health".into(),
                interval_ms: 10_000,
                timeout_ms: 2_000,
                healthy_threshold: 1,
                unhealthy_threshold: 3,
                stabilization_window_ms: 30_000,
            }),
        },
        created_at + Duration::seconds(3),
    )
    .expect("revision");
    revision
        .bind_mcp_release(&workload, &asset, &release, &profile_binding)
        .expect("bind MCP release");

    let spec = project_runtime_spec(&revision).expect("profile-bound Runtime spec");
    assert_eq!(
        spec.semantics_profile_digest.as_deref(),
        Some(profile.digest().as_str())
    );
    assert_eq!(spec.class, RuntimeUnitClass::Service);
    assert_eq!(spec.network.mode, NetworkMode::Service);
    assert_eq!(spec.network.ports[0].name, "mcp");

    let invalid = project_runtime_spec_with_digest(&revision, Some("sha256:not-a-digest"))
        .expect_err("invalid digest must fail Runtime validation");
    assert!(invalid.contains("digest"));
}

#[test]
fn projects_headless_service_to_network_none_without_health() {
    let digest = format!("sha256:{}", "b".repeat(64));
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        WorkloadId::new(),
        1,
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!("oci://registry.example/fixture@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ServiceProcess {
                command: vec!["/bin/sh".into(), "-c".into()],
                args: vec!["exec sleep 3600".into()],
                working_directory: Some("/".into()),
                environment: BTreeMap::new(),
            },
            secrets: Vec::new(),
            resources: ServiceResources {
                cpu_millis: 100,
                memory_bytes: 32 * 1024 * 1024,
                pids: 32,
                ephemeral_storage_bytes: None,
            },
            ports: Vec::new(),
            health: None,
        },
        Utc::now(),
    )
    .expect("headless revision");

    let spec = project_runtime_spec(&revision).expect("headless Runtime spec");
    assert_eq!(spec.class, RuntimeUnitClass::Service);
    assert_eq!(spec.network.mode, NetworkMode::None);
    assert!(spec.network.ports.is_empty());
    assert!(spec.health.is_none());
}
