use super::entities::{
    AgentReleaseAdmission, AgentReleaseRuntimeContract, AgentWorkloadRevisionBinding, OciArtifact,
    SecretBinding, SecretBindingTarget, ServiceResources,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, OrganizationId, SecretId, Sha256Digest,
};
use a3s_cloud_contracts::{
    agent_release_builder_uri, agent_release_manifest_archive, agent_release_source_uri,
    artifact_uri, AgentReleaseManifest, AgentReleaseProvenance,
};
use chrono::Utc;

const OCI_DIGEST: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const SOURCE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROVENANCE_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn release_template() -> &'static str {
    concat!(
        "agent_release {\n",
        "  schema = \"a3s.code.agent-release.v1\"\n",
        "  protocol = \"a3s.code.agent.v1\"\n",
        "  artifact { digest = \"sha256:1111111111111111111111111111111111111111111111111111111111111111\" media_type = \"application/vnd.oci.image.manifest.v1+json\" }\n",
        "  entrypoint { command = \"/usr/bin/a3s\" args = [\"code\", \"harness\", \"--manifest\", \"/app/.a3s/asset.acl\"] }\n",
        "  health { transport = \"http\" port = 8080 readiness_path = \"/health/ready\" liveness_path = \"/health/live\" shutdown_grace_seconds = 30 }\n",
        "  storage { workspace = \"ephemeral\" cache = \"ephemeral\" persistent_data = \"none\" }\n",
        "  capability \"runtime.service\" { level = 1 }\n",
        "  capability \"secrets.external\" { level = 1 }\n",
        "  capability \"workspace.local\" { level = 1 }\n",
        "  secret \"provider-api-key\" { target = \"environment\" destination = \"PROVIDER_API_KEY\" }\n",
        "  secret \"signing-key\" { target = \"file\" destination = \"/run/secrets/signing-key\" }\n",
        "  provenance \"source\" { uri = \"urn:a3s:source:template\" digest = \"sha256:2222222222222222222222222222222222222222222222222222222222222222\" }\n",
        "  provenance \"builder\" { uri = \"urn:a3s:builder:template\" digest = \"sha256:4444444444444444444444444444444444444444444444444444444444444444\" }\n",
        "}\n",
    )
}

fn admission_with_builder(
    build_run_id: BuildRunId,
    manifest_builder_id: BuildRunId,
) -> Result<AgentReleaseAdmission, String> {
    let template = AgentReleaseManifest::parse(release_template())
        .map_err(|error| format!("test release template is invalid: {error}"))?;
    let manifest = template
        .bind_publication(
            OCI_DIGEST,
            [
                AgentReleaseProvenance::new(
                    "source",
                    agent_release_source_uri(SOURCE_DIGEST)?,
                    SOURCE_DIGEST,
                )
                .map_err(|error| error.to_string())?,
                AgentReleaseProvenance::new(
                    "builder",
                    agent_release_builder_uri(manifest_builder_id.as_uuid())?,
                    PROVENANCE_DIGEST,
                )
                .map_err(|error| error.to_string())?,
            ],
        )
        .map_err(|error| error.to_string())?;
    let archive = agent_release_manifest_archive(manifest.canonical_acl().as_bytes())?;
    let archive_digest = Sha256Digest::from_bytes(&archive);
    AgentReleaseAdmission::new(
        OrganizationId::new(),
        AssetId::new(),
        AssetReleaseId::new(),
        build_run_id,
        Utc::now(),
        OciArtifact {
            uri: format!("oci://registry.example/a3s/agent@{OCI_DIGEST}"),
            digest: OCI_DIGEST.into(),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        manifest.identity(),
        manifest.canonical_acl(),
        artifact_uri(archive_digest.as_str())?,
        archive_digest.as_str(),
        archive.len() as u64,
    )
}

fn admission() -> AgentReleaseAdmission {
    let build_run_id = BuildRunId::new();
    admission_with_builder(build_run_id, build_run_id).expect("Agent release admission")
}

fn resources(ephemeral_storage_bytes: Option<u64>) -> ServiceResources {
    ServiceResources {
        cpu_millis: 250,
        memory_bytes: 64 * 1024 * 1024,
        pids: 64,
        ephemeral_storage_bytes,
    }
}

fn environment_secret() -> SecretBinding {
    SecretBinding {
        name: "provider-api-key".into(),
        secret_id: SecretId::new(),
        version: 1,
        target: SecretBindingTarget::Environment {
            variable: "PROVIDER_API_KEY".into(),
        },
    }
}

fn file_secret() -> SecretBinding {
    SecretBinding {
        name: "signing-key".into(),
        secret_id: SecretId::new(),
        version: 1,
        target: SecretBindingTarget::File {
            path: "/run/secrets/signing-key".into(),
            mode: 0o400,
        },
    }
}

#[test]
fn agent_template_requires_bounded_ephemeral_storage_and_exact_declared_secrets() {
    let admission = admission();
    let exact = vec![environment_secret(), file_secret()];
    let resolved = admission
        .resolve_template(exact.clone(), resources(Some(64 * 1024 * 1024)))
        .expect("exact Agent template");
    assert_eq!(resolved.secrets, exact);
    assert_eq!(resolved.process.command, ["/usr/bin/a3s"]);
    assert_eq!(resolved.ports[0].container_port, 8080);
    assert_eq!(
        resolved.health.as_ref().expect("readiness probe").path,
        "/health/ready"
    );

    for unbounded in [None, Some(0)] {
        assert!(admission
            .resolve_template(
                vec![environment_secret(), file_secret()],
                resources(unbounded),
            )
            .expect_err("unbounded storage must fail closed")
            .contains("ephemeral storage"));
    }

    let mut mismatches = Vec::new();
    mismatches.push(vec![environment_secret()]);

    let mut wrong_environment = environment_secret();
    wrong_environment.target = SecretBindingTarget::Environment {
        variable: "DIFFERENT_API_KEY".into(),
    };
    mismatches.push(vec![wrong_environment, file_secret()]);

    let mut wrong_path = file_secret();
    wrong_path.target = SecretBindingTarget::File {
        path: "/run/secrets/different".into(),
        mode: 0o400,
    };
    mismatches.push(vec![environment_secret(), wrong_path]);

    let mut wrong_mode = file_secret();
    wrong_mode.target = SecretBindingTarget::File {
        path: "/run/secrets/signing-key".into(),
        mode: 0o600,
    };
    mismatches.push(vec![environment_secret(), wrong_mode]);

    for mismatch in mismatches {
        assert!(admission
            .resolve_template(mismatch, resources(Some(64 * 1024 * 1024)))
            .expect_err("Secret mismatch must fail closed")
            .contains("do not match"));
    }

    let registry = SecretBinding {
        name: "registry".into(),
        secret_id: SecretId::new(),
        version: 1,
        target: SecretBindingTarget::RegistryCredential,
    };
    admission
        .resolve_template(
            vec![environment_secret(), file_secret(), registry.clone()],
            resources(Some(64 * 1024 * 1024)),
        )
        .expect("one registry credential may accompany declared Agent Secrets");
    assert!(admission
        .resolve_template(
            vec![
                environment_secret(),
                file_secret(),
                registry.clone(),
                registry
            ],
            resources(Some(64 * 1024 * 1024)),
        )
        .is_err());
}

#[test]
fn admission_and_restored_binding_reject_provenance_or_archive_tampering() {
    let build_run_id = BuildRunId::new();
    let wrong_builder = BuildRunId::new();
    assert_ne!(build_run_id, wrong_builder);
    assert!(admission_with_builder(build_run_id, wrong_builder)
        .expect_err("wrong builder must fail at admission")
        .contains("provenance binding"));

    let admission = admission();
    let binding_ids = (
        OrganizationId::new(),
        AssetId::new(),
        AssetReleaseId::new(),
        BuildRunId::new(),
    );
    for field in ["archiveUri", "archiveDigest", "archiveSizeBytes"] {
        let mut encoded =
            serde_json::to_value(admission.runtime_contract()).expect("serialize runtime contract");
        match field {
            "archiveUri" => {
                encoded[field] =
                    serde_json::json!(artifact_uri(&format!("sha256:{}", "c".repeat(64)))
                        .expect("changed Artifact URI"));
            }
            "archiveDigest" => {
                encoded[field] = serde_json::json!(format!("sha256:{}", "c".repeat(64)));
            }
            "archiveSizeBytes" => encoded[field] = serde_json::json!(1),
            _ => unreachable!(),
        }
        let contract = serde_json::from_value::<AgentReleaseRuntimeContract>(encoded)
            .expect("deserialize structurally valid tampering");
        assert!(AgentWorkloadRevisionBinding::restore_with_contract(
            binding_ids.0,
            binding_ids.1,
            binding_ids.2,
            binding_ids.3,
            Some(contract),
        )
        .expect_err("tampered archive must fail closed")
        .contains("archive changed its exact bytes"));
    }
}
