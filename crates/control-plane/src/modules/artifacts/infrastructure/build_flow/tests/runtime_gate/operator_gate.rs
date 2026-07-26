use super::process_death::ProcessDeathGateEvidence;
use super::{
    require, required_environment, RegistryCredentialMaterial, PUBLICATION_CREDENTIAL_ENV,
    REGISTRY_PASSWORD_ENV, REGISTRY_USERNAME_ENV,
};
use crate::modules::artifacts::domain::{
    BuildEvidence, BuildRun, BuildRunStatus, PublishedOciArtifact,
};
use crate::modules::artifacts::VaultBuildEvidenceSigner;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

const OPERATOR_GATE_ENV: &str = "A3S_CLOUD_TEST_G0_OPERATOR";
const CLOUD_REVISION_ENV: &str = "A3S_CLOUD_TEST_CLOUD_REVISION";
const EVIDENCE_DIRECTORY_ENV: &str = "A3S_CLOUD_TEST_G0_EVIDENCE_DIR";
const VAULT_ADDRESS_ENV: &str = "A3S_CLOUD_TEST_VAULT_ADDR";
const VAULT_TOKEN_ENV: &str = "A3S_CLOUD_TEST_VAULT_TOKEN";
const VAULT_TRANSIT_MOUNT_ENV: &str = "A3S_CLOUD_TEST_VAULT_TRANSIT_MOUNT";
const VAULT_TRANSIT_KEY_ENV: &str = "A3S_CLOUD_TEST_VAULT_TRANSIT_KEY";
const OPERATOR_EVIDENCE_SCHEMA: &str = "a3s.cloud.g0-signed-build-evidence.v1";

pub(super) struct OperatorGate {
    cloud_revision: String,
    evidence_directory: PathBuf,
    vault_address: String,
    vault_token: Zeroizing<String>,
    vault_transit_mount: String,
    vault_transit_key: String,
    registry_credential_material: Zeroizing<String>,
    registry_basic_credential: Zeroizing<String>,
    registry_password: Zeroizing<String>,
}

impl OperatorGate {
    pub(super) fn from_environment() -> Result<Option<Self>, Box<dyn Error>> {
        match std::env::var(OPERATOR_GATE_ENV) {
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Ok(value) if value == "1" => {}
            Ok(_) | Err(std::env::VarError::NotUnicode(_)) => {
                return Err(std::io::Error::other(
                    "G0 operator gate selector must be exactly 1 when present",
                )
                .into())
            }
        }
        let cloud_revision = required_environment(CLOUD_REVISION_ENV)?;
        validate_cloud_revision(&cloud_revision)?;
        let evidence_directory = PathBuf::from(required_environment(EVIDENCE_DIRECTORY_ENV)?);
        require(
            evidence_directory.is_absolute(),
            "G0 operator evidence directory must be absolute",
        )?;
        let registry_username = Zeroizing::new(required_environment(REGISTRY_USERNAME_ENV)?);
        let registry_password = Zeroizing::new(required_environment(REGISTRY_PASSWORD_ENV)?);
        let registry_credential_material =
            Zeroizing::new(serde_json::to_string(&serde_json::json!({
                "schema": RegistryCredentialMaterial::SCHEMA,
                "username": registry_username.as_str(),
                "password": registry_password.as_str(),
            }))?);
        RegistryCredentialMaterial::parse(registry_credential_material.as_bytes())?;
        let registry_basic_material = Zeroizing::new(format!(
            "{}:{}",
            registry_username.as_str(),
            registry_password.as_str()
        ));
        let registry_basic_credential =
            Zeroizing::new(STANDARD.encode(registry_basic_material.as_bytes()));
        Ok(Some(Self {
            cloud_revision,
            evidence_directory,
            vault_address: required_environment(VAULT_ADDRESS_ENV)?,
            vault_token: Zeroizing::new(required_environment(VAULT_TOKEN_ENV)?),
            vault_transit_mount: required_environment(VAULT_TRANSIT_MOUNT_ENV)?,
            vault_transit_key: required_environment(VAULT_TRANSIT_KEY_ENV)?,
            registry_credential_material,
            registry_basic_credential,
            registry_password,
        }))
    }

    pub(super) fn signer(&self) -> Result<VaultBuildEvidenceSigner, Box<dyn Error>> {
        Ok(VaultBuildEvidenceSigner::new(
            &self.vault_address,
            self.vault_token.as_str(),
            self.vault_transit_mount.clone(),
            self.vault_transit_key.clone(),
            Duration::from_secs(30),
        )?)
    }

    pub(super) async fn write_evidence(
        &self,
        result: &OperatorGateResult,
        build: &BuildRun,
        process_death: &ProcessDeathGateEvidence,
    ) -> Result<(), Box<dyn Error>> {
        result.evidence.validate().map_err(std::io::Error::other)?;
        process_death.validate()?;
        require(
            result.evidence.artifact == result.published,
            "G0 operator evidence changed its published OCI artifact",
        )?;
        let key_version = result
            .evidence
            .signing_key
            .key_version
            .ok_or_else(|| std::io::Error::other("Vault evidence omitted its key version"))?;
        let durable_build = serde_json::to_vec(build)?;
        let durable_evidence = serde_json::to_vec(&result.evidence)?;
        self.reject_protected_material(&durable_build)?;
        self.reject_protected_material(&durable_evidence)?;
        require(
            std::env::var_os(PUBLICATION_CREDENTIAL_ENV).is_none(),
            "G0 operator publication credential remained materialized after publication",
        )?;
        let document = SignedBuildGateEvidence {
            schema: OPERATOR_EVIDENCE_SCHEMA,
            cloud_revision: self.cloud_revision.clone(),
            registry_authority_digest: sha256_bytes(result.registry_authority.as_bytes()),
            build_run_id_digest: sha256_bytes(build.id.to_string().as_bytes()),
            artifact_digest: result.published.digest.clone(),
            artifact_media_type: result.published.media_type.clone(),
            artifact_size_bytes: result.published.size_bytes,
            sbom_digest: result.evidence.sbom_digest.clone(),
            provenance_digest: result.evidence.provenance_digest.clone(),
            key_id: result.evidence.signing_key.key_id.clone(),
            key_version,
            attested_at: result.evidence.attested_at,
            completed_at: Utc::now(),
            process_death: process_death.clone(),
            checks: SignedBuildGateChecks {
                registry_https: true,
                publication_replay_verified: true,
                vault_signature_verified: true,
                durable_credentials_absent: true,
                runtime_removed: true,
                cleanup_completed: build.status == BuildRunStatus::Succeeded,
                publication_process_death_recovered: true,
                evidence_process_death_recovered: true,
            },
        };
        let mut encoded = serde_json::to_vec_pretty(&document)?;
        self.reject_protected_material(&encoded)?;
        encoded.push(b'\n');
        tokio::fs::create_dir_all(&self.evidence_directory).await?;
        let temporary = self
            .evidence_directory
            .join(format!(".signed-build-evidence-{}.tmp", Uuid::now_v7()));
        tokio::fs::write(&temporary, encoded).await?;
        tokio::fs::rename(
            temporary,
            self.evidence_directory.join("signed-build-evidence.json"),
        )
        .await?;
        Ok(())
    }

    pub(super) fn reject_protected_material(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        require(
            !contains_secret(bytes, self.vault_token.as_bytes())
                && !contains_secret(bytes, self.registry_credential_material.as_bytes())
                && !contains_secret(bytes, self.registry_basic_credential.as_bytes())
                && !contains_secret(bytes, self.registry_password.as_bytes()),
            "G0 operator durable evidence contains protected provider material",
        )
    }
}

pub(super) struct OperatorGateResult {
    pub(super) registry_authority: String,
    pub(super) published: PublishedOciArtifact,
    pub(super) evidence: BuildEvidence,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedBuildGateEvidence {
    schema: &'static str,
    cloud_revision: String,
    registry_authority_digest: String,
    build_run_id_digest: String,
    artifact_digest: String,
    artifact_media_type: String,
    artifact_size_bytes: u64,
    sbom_digest: String,
    provenance_digest: String,
    key_id: String,
    key_version: u32,
    attested_at: chrono::DateTime<Utc>,
    completed_at: chrono::DateTime<Utc>,
    process_death: ProcessDeathGateEvidence,
    checks: SignedBuildGateChecks,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedBuildGateChecks {
    registry_https: bool,
    publication_replay_verified: bool,
    vault_signature_verified: bool,
    durable_credentials_absent: bool,
    runtime_removed: bool,
    cleanup_completed: bool,
    publication_process_death_recovered: bool,
    evidence_process_death_recovered: bool,
}

fn validate_cloud_revision(revision: &str) -> Result<(), Box<dyn Error>> {
    require(
        revision.len() == 40
            && revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')),
        "G0 operator Cloud revision must be a full lowercase Git SHA",
    )
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn contains_secret(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}
