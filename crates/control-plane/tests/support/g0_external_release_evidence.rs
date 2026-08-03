use super::fixture::{sha256, GateConfig};
use serde_json::json;
use std::path::Path;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct EvidenceFacts<'a> {
    pub source_repository: &'a str,
    pub source_commit: &'a str,
    pub source_content_digest: &'a str,
    pub build_input_digest: &'a str,
    pub box_output_digest: &'a str,
    pub published_artifact_digest: &'a str,
    pub published_resource_identity: &'a str,
    pub sbom_digest: &'a str,
    pub provenance_digest: &'a str,
    pub signing_key_id: &'a str,
    pub signing_key_version: u32,
    pub build_run_id: Uuid,
    pub workload_id: Uuid,
    pub deployment_id: Uuid,
    pub source_evidence_digest: &'a str,
    pub box_evidence_digest: &'a str,
    pub registry_authority: &'a str,
}

pub(super) async fn write(config: &GateConfig, facts: EvidenceFacts<'_>) -> TestResult {
    let vault_identity = format!(
        "{}/{}/{}",
        config.vault_address, config.vault_transit_mount, config.vault_signing_key
    );
    let evidence = json!({
        "schema": "a3s.cloud.g0-external-release-evidence.v1",
        "cloudRevision": config.cloud_revision,
        "boxRevision": config.box_revision,
        "privateSourceEvidenceDigest": facts.source_evidence_digest,
        "boxProviderEvidenceDigest": facts.box_evidence_digest,
        "sourceContentDigest": facts.source_content_digest,
        "buildInputDigest": facts.build_input_digest,
        "boxOutputDigest": facts.box_output_digest,
        "publishedArtifactDigest": facts.published_artifact_digest,
        "publishedResourceIdentityDigest": sha256(facts.published_resource_identity.as_bytes()),
        "sbomDigest": facts.sbom_digest,
        "provenanceDigest": facts.provenance_digest,
        "signingKeyId": facts.signing_key_id,
        "signingKeyVersion": facts.signing_key_version,
        "buildRunId": facts.build_run_id,
        "workloadId": facts.workload_id,
        "deploymentId": facts.deployment_id,
        "registryIdentityDigest": sha256(facts.registry_authority.as_bytes()),
        "vaultIdentityDigest": sha256(vault_identity.as_bytes()),
        "checks": {
            "privateSourceResolved": true,
            "productionInputPrepared": true,
            "productionRecipeProjected": true,
            "boxOutputSourceBound": true,
            "boxProcessDeathReplayed": true,
            "parentCacheHydrated": true,
            "boxRemovalAuthoritative": true,
            "postgresFleetCommandsExact": true,
            "ociGraphValidated": true,
            "externalRegistryPublished": true,
            "remoteRegistryReplayExact": true,
            "vaultTransitSigned": true,
            "signatureLocallyVerified": true,
            "buildEvidencePersisted": true,
            "buildRunRestartRestored": true,
            "workloadHandoffPersisted": true,
            "deploymentWorkflowVersionThree": true,
            "idempotencyReplayExact": true,
            "publishedResourceTracked": true
        }
    });
    let mut encoded = serde_json::to_vec_pretty(&evidence)?;
    encoded.push(b'\n');
    for protected in [
        facts.source_repository,
        facts.source_commit,
        config.registry_url.as_str(),
        config.registry_password.as_str(),
        config.vault_address.as_str(),
        config.vault_token.as_str(),
    ] {
        if contains(&encoded, protected.as_bytes()) {
            return Err(test_error(
                "G0 external release evidence contains protected provider input",
            ));
        }
    }
    tokio::fs::create_dir_all(&config.evidence_directory).await?;
    write_durable(
        &config.evidence_directory.join("external-release.json"),
        &encoded,
    )
    .await
}

async fn write_durable(path: &Path, body: &[u8]) -> TestResult {
    let parent = path
        .parent()
        .ok_or_else(|| test_error("G0 evidence path has no parent"))?;
    let temporary = parent.join(format!(".external-release-{}.tmp", Uuid::now_v7()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary).await?;
    use tokio::io::AsyncWriteExt;
    file.write_all(body).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(temporary, path).await?;
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn test_error(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}
