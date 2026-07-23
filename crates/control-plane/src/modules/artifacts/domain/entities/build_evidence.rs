use super::build_artifact::validate_sha256;
use super::oci_publication::PublishedOciArtifact;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, OperationId, SourceRevisionId,
};
use crate::modules::sources::domain::{BuildPlatform, BuildRecipe};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const BUILD_EVIDENCE_SCHEMA: &str = "a3s.cloud.build-evidence.v1";
pub const DSSE_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";
pub const IN_TOTO_STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
pub const SLSA_PROVENANCE_PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";
pub const SLSA_BUILD_TYPE: &str = "https://a3s.dev/cloud/build/v1";
pub const SPDX_VERSION: &str = "SPDX-2.3";

const MAX_CANONICAL_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_SPDX_FILES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildEvidence {
    pub schema: String,
    pub build_run_id: BuildRunId,
    pub operation_id: OperationId,
    pub source_revision_id: SourceRevisionId,
    pub attempt: u32,
    pub repository: String,
    pub commit_sha: String,
    pub source_content_digest: String,
    pub recipe: BuildRecipe,
    pub recipe_digest: String,
    pub runtime_spec_digest: String,
    pub builder: BuildEvidenceBuilder,
    pub platforms: Vec<BuildPlatform>,
    pub artifact: PublishedOciArtifact,
    pub sbom: SpdxDocument,
    pub sbom_digest: String,
    pub provenance: SlsaProvenanceStatement,
    pub provenance_digest: String,
    pub envelope: DsseEnvelope,
    pub signing_key: BuildEvidenceSigningKey,
    pub verification_state: BuildEvidenceVerificationState,
    pub attested_at: DateTime<Utc>,
}

impl BuildEvidence {
    pub fn restore(mut self) -> Result<Self, String> {
        self.attested_at = canonical_timestamp(self.attested_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUILD_EVIDENCE_SCHEMA
            || self.attempt == 0
            || self.operation_id.as_uuid() != self.build_run_id.as_uuid()
            || self.repository.trim().is_empty()
            || self.repository.len() > 4096
            || self.repository.contains(['\0', '\r', '\n'])
            || !matches!(self.commit_sha.len(), 40 | 64)
            || !self.commit_sha.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.commit_sha != self.commit_sha.to_ascii_lowercase()
            || canonical_timestamp(self.attested_at) != self.attested_at
            || self.verification_state != BuildEvidenceVerificationState::Verified
        {
            return Err("build evidence identity or verification state is invalid".into());
        }
        validate_sha256(&self.source_content_digest, "source content digest")?;
        validate_sha256(&self.recipe_digest, "build recipe digest")?;
        validate_sha256(&self.runtime_spec_digest, "Runtime specification digest")?;
        if self.recipe.digest()? != self.recipe_digest {
            return Err("build evidence recipe digest does not match its canonical recipe".into());
        }
        self.builder.validate()?;
        self.artifact.validate()?;
        validate_platforms(&self.platforms)?;
        self.sbom.validate()?;
        self.provenance.validate()?;
        self.signing_key.validate()?;

        let sbom = canonical_json(&self.sbom)?;
        if sha256_digest(&sbom) != self.sbom_digest {
            return Err("build evidence SBOM digest does not match its canonical document".into());
        }
        let provenance = canonical_json(&self.provenance)?;
        if sha256_digest(&provenance) != self.provenance_digest {
            return Err(
                "build evidence provenance digest does not match its canonical statement".into(),
            );
        }
        self.envelope.validate(&provenance, &self.signing_key)?;
        self.validate_spdx_binding()?;
        self.validate_provenance_binding()?;
        Ok(())
    }

    fn validate_spdx_binding(&self) -> Result<(), String> {
        let artifact_checksum = digest_hex(&self.artifact.digest)?;
        let root = self
            .sbom
            .packages
            .iter()
            .find(|package| package.spdx_id == "SPDXRef-Package-OCI")
            .ok_or_else(|| "build evidence SBOM omitted its OCI root package".to_owned())?;
        if root.download_location != self.artifact.uri
            || !root
                .checksums
                .iter()
                .any(|checksum| checksum.is_sha256(artifact_checksum))
        {
            return Err("build evidence SBOM changed its published OCI root".into());
        }
        Ok(())
    }

    fn validate_provenance_binding(&self) -> Result<(), String> {
        let artifact_digest = digest_hex(&self.artifact.digest)?;
        let sbom_digest = digest_hex(&self.sbom_digest)?;
        if !self
            .provenance
            .subject
            .iter()
            .any(|subject| subject.matches(&self.artifact.uri, "sha256", artifact_digest))
            || !self.provenance.subject.iter().any(|subject| {
                subject.matches(&self.sbom.document_namespace, "sha256", sbom_digest)
            })
        {
            return Err(
                "build evidence provenance subjects are not artifact and SBOM bound".into(),
            );
        }
        let definition = &self.provenance.predicate.build_definition;
        let external = &definition.external_parameters;
        let internal = &definition.internal_parameters;
        if external.repository != self.repository
            || external.commit_sha != self.commit_sha
            || external.source_content_digest != self.source_content_digest
            || external.recipe != self.recipe
            || external.recipe_digest != self.recipe_digest
            || external.platforms != self.platforms
            || internal.build_run_id != self.build_run_id
            || internal.operation_id != self.operation_id
            || internal.source_revision_id != self.source_revision_id
            || internal.attempt != self.attempt
            || internal.runtime_spec_digest != self.runtime_spec_digest
            || self.provenance.predicate.run_details.builder.id != self.builder.uri
        {
            return Err("build evidence provenance changed its immutable build inputs".into());
        }
        let builder_digest = digest_hex(&self.builder.digest)?;
        if !self
            .provenance
            .predicate
            .run_details
            .builder
            .builder_dependencies
            .iter()
            .any(|dependency| dependency.matches(&self.builder.uri, "sha256", builder_digest))
        {
            return Err("build evidence provenance omitted its digest-pinned builder".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildEvidenceBuilder {
    pub uri: String,
    pub digest: String,
}

impl BuildEvidenceBuilder {
    pub fn validate(&self) -> Result<(), String> {
        validate_sha256(&self.digest, "build evidence builder digest")?;
        if self.uri.trim().is_empty()
            || self.uri.len() > 4096
            || self.uri.contains(['\0', '\r', '\n'])
            || !self.uri.ends_with(&format!("@{}", self.digest))
        {
            return Err("build evidence builder must be a digest-pinned URI".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildEvidenceVerificationState {
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildEvidenceSigningKey {
    pub algorithm: String,
    pub key_id: String,
    pub public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_version: Option<u32>,
}

impl BuildEvidenceSigningKey {
    pub fn validate(&self) -> Result<(), String> {
        if self.algorithm != "ed25519" || self.key_version == Some(0) {
            return Err("build evidence signing key algorithm or version is invalid".into());
        }
        validate_sha256(&self.key_id, "build evidence signing key ID")?;
        if sha256_digest(&self.public_key_bytes()?) != self.key_id {
            return Err("build evidence signing key ID does not match its public key".into());
        }
        Ok(())
    }

    fn public_key_bytes(&self) -> Result<[u8; 32], String> {
        let public_key = STANDARD
            .decode(&self.public_key)
            .map_err(|_| "build evidence Ed25519 public key is not canonical base64".to_owned())?;
        if public_key.len() != 32 || STANDARD.encode(&public_key) != self.public_key {
            return Err("build evidence Ed25519 public key is invalid".into());
        }
        public_key
            .try_into()
            .map_err(|_| "build evidence Ed25519 public key is invalid".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DsseEnvelope {
    pub payload_type: String,
    pub payload: String,
    pub signatures: Vec<DsseSignature>,
}

impl DsseEnvelope {
    pub fn validate(
        &self,
        expected_payload: &[u8],
        signing_key: &BuildEvidenceSigningKey,
    ) -> Result<(), String> {
        if self.payload_type != DSSE_PAYLOAD_TYPE || self.signatures.len() != 1 {
            return Err("build evidence DSSE envelope shape is invalid".into());
        }
        let payload = STANDARD
            .decode(&self.payload)
            .map_err(|_| "build evidence DSSE payload is not canonical base64".to_owned())?;
        if payload != expected_payload || STANDARD.encode(&payload) != self.payload {
            return Err("build evidence DSSE payload changed its provenance statement".into());
        }
        let signature = &self.signatures[0];
        if signature.key_id != signing_key.key_id {
            return Err("build evidence DSSE signature changed its key ID".into());
        }
        let signature_bytes = STANDARD
            .decode(&signature.signature)
            .map_err(|_| "build evidence DSSE signature is not canonical base64".to_owned())?;
        if signature_bytes.len() != 64 || STANDARD.encode(&signature_bytes) != signature.signature {
            return Err("build evidence DSSE Ed25519 signature is invalid".into());
        }
        let pae = dsse_pae(&self.payload_type, &payload)?;
        UnparsedPublicKey::new(&ED25519, signing_key.public_key_bytes()?)
            .verify(&pae, &signature_bytes)
            .map_err(|_| "build evidence DSSE Ed25519 signature failed verification".to_owned())?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DsseSignature {
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpdxDocument {
    pub spdx_version: String,
    pub data_license: String,
    #[serde(rename = "SPDXID")]
    pub spdx_id: String,
    pub name: String,
    pub document_namespace: String,
    pub creation_info: SpdxCreationInfo,
    pub packages: Vec<SpdxPackage>,
    pub files: Vec<SpdxFile>,
    pub relationships: Vec<SpdxRelationship>,
}

impl SpdxDocument {
    pub fn validate(&self) -> Result<(), String> {
        if self.spdx_version != SPDX_VERSION
            || self.data_license != "CC0-1.0"
            || self.spdx_id != "SPDXRef-DOCUMENT"
            || !valid_text(&self.name, 255)
            || !valid_uri(&self.document_namespace)
            || self.packages.is_empty()
            || self.packages.len() > 64
            || self.files.is_empty()
            || self.files.len() > MAX_SPDX_FILES
            || self.relationships.is_empty()
            || self.relationships.len() > MAX_SPDX_FILES.saturating_add(64)
        {
            return Err("SPDX 2.3 document identity or bounds are invalid".into());
        }
        self.creation_info.validate()?;
        let mut identifiers = BTreeSet::from([self.spdx_id.as_str()]);
        for package in &self.packages {
            package.validate()?;
            if !identifiers.insert(&package.spdx_id) {
                return Err("SPDX document contains duplicate element IDs".into());
            }
        }
        for file in &self.files {
            file.validate()?;
            if !identifiers.insert(&file.spdx_id) {
                return Err("SPDX document contains duplicate element IDs".into());
            }
        }
        for relationship in &self.relationships {
            relationship.validate()?;
            if !identifiers.contains(relationship.spdx_element_id.as_str())
                || !identifiers.contains(relationship.related_spdx_element.as_str())
            {
                return Err("SPDX relationship references an unknown element".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpdxCreationInfo {
    pub created: DateTime<Utc>,
    pub creators: Vec<String>,
}

impl SpdxCreationInfo {
    fn validate(&self) -> Result<(), String> {
        if canonical_timestamp(self.created) != self.created
            || self.creators.is_empty()
            || self.creators.len() > 16
            || self
                .creators
                .iter()
                .any(|creator| !valid_text(creator, 255))
        {
            return Err("SPDX creation information is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpdxPackage {
    pub name: String,
    #[serde(rename = "SPDXID")]
    pub spdx_id: String,
    pub version_info: String,
    pub supplier: String,
    pub download_location: String,
    pub files_analyzed: bool,
    pub checksums: Vec<SpdxChecksum>,
    pub primary_package_purpose: String,
}

impl SpdxPackage {
    fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.name, 255)
            || !valid_spdx_id(&self.spdx_id)
            || !valid_text(&self.version_info, 255)
            || !valid_text(&self.supplier, 255)
            || !valid_uri(&self.download_location)
            || self.files_analyzed
            || self.primary_package_purpose != "CONTAINER"
        {
            return Err("SPDX OCI package is invalid".into());
        }
        validate_checksums(&self.checksums)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpdxFile {
    pub file_name: String,
    #[serde(rename = "SPDXID")]
    pub spdx_id: String,
    pub checksums: Vec<SpdxChecksum>,
    pub file_types: Vec<String>,
    pub comment: String,
}

impl SpdxFile {
    fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.file_name, 4096)
            || !valid_spdx_id(&self.spdx_id)
            || self.file_types != ["BINARY"]
            || !valid_text(&self.comment, 4096)
        {
            return Err("SPDX OCI descriptor file is invalid".into());
        }
        validate_checksums(&self.checksums)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpdxChecksum {
    pub algorithm: String,
    pub checksum_value: String,
}

impl SpdxChecksum {
    fn is_sha256(&self, expected: &str) -> bool {
        self.algorithm == "SHA256" && self.checksum_value == expected
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpdxRelationship {
    pub spdx_element_id: String,
    pub relationship_type: String,
    pub related_spdx_element: String,
}

impl SpdxRelationship {
    fn validate(&self) -> Result<(), String> {
        if !valid_spdx_id(&self.spdx_element_id)
            || !matches!(self.relationship_type.as_str(), "DESCRIBES" | "CONTAINS")
            || !valid_spdx_id(&self.related_spdx_element)
        {
            return Err("SPDX relationship is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlsaProvenanceStatement {
    #[serde(rename = "_type")]
    pub statement_type: String,
    pub subject: Vec<InTotoSubject>,
    #[serde(rename = "predicateType")]
    pub predicate_type: String,
    pub predicate: SlsaProvenancePredicate,
}

impl SlsaProvenanceStatement {
    pub fn validate(&self) -> Result<(), String> {
        if self.statement_type != IN_TOTO_STATEMENT_TYPE
            || self.predicate_type != SLSA_PROVENANCE_PREDICATE_TYPE
            || self.subject.len() != 2
            || self
                .subject
                .iter()
                .any(|subject| subject.validate().is_err())
        {
            return Err("SLSA provenance statement identity is invalid".into());
        }
        self.predicate.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InTotoSubject {
    pub name: String,
    pub digest: BTreeMap<String, String>,
}

impl InTotoSubject {
    fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.name, 4096) || self.digest.len() != 1 {
            return Err("in-toto subject is invalid".into());
        }
        for (algorithm, digest) in &self.digest {
            if algorithm != "sha256" || !valid_hex_sha256(digest) {
                return Err("in-toto subject digest is invalid".into());
            }
        }
        Ok(())
    }

    fn matches(&self, name: &str, algorithm: &str, digest: &str) -> bool {
        self.name == name
            && self
                .digest
                .get(algorithm)
                .is_some_and(|value| value == digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlsaProvenancePredicate {
    pub build_definition: SlsaBuildDefinition,
    pub run_details: SlsaRunDetails,
}

impl SlsaProvenancePredicate {
    fn validate(&self) -> Result<(), String> {
        self.build_definition.validate()?;
        self.run_details.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlsaBuildDefinition {
    pub build_type: String,
    pub external_parameters: SlsaExternalParameters,
    pub internal_parameters: SlsaInternalParameters,
    pub resolved_dependencies: Vec<SlsaResourceDescriptor>,
}

impl SlsaBuildDefinition {
    fn validate(&self) -> Result<(), String> {
        if self.build_type != SLSA_BUILD_TYPE
            || self.resolved_dependencies.is_empty()
            || self.resolved_dependencies.len() > 32
        {
            return Err("SLSA build definition is invalid".into());
        }
        validate_platforms(&self.external_parameters.platforms)?;
        validate_sha256(
            &self.external_parameters.source_content_digest,
            "SLSA source content digest",
        )?;
        validate_sha256(
            &self.external_parameters.recipe_digest,
            "SLSA build recipe digest",
        )?;
        validate_sha256(
            &self.internal_parameters.runtime_spec_digest,
            "SLSA Runtime specification digest",
        )?;
        if self.external_parameters.recipe.digest()? != self.external_parameters.recipe_digest
            || self.internal_parameters.attempt == 0
            || self.internal_parameters.operation_id.as_uuid()
                != self.internal_parameters.build_run_id.as_uuid()
        {
            return Err("SLSA build parameters are invalid".into());
        }
        for dependency in &self.resolved_dependencies {
            dependency.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlsaExternalParameters {
    pub repository: String,
    pub commit_sha: String,
    pub source_content_digest: String,
    pub recipe: BuildRecipe,
    pub recipe_digest: String,
    pub platforms: Vec<BuildPlatform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlsaInternalParameters {
    pub build_run_id: BuildRunId,
    pub operation_id: OperationId,
    pub source_revision_id: SourceRevisionId,
    pub attempt: u32,
    pub runtime_spec_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlsaResourceDescriptor {
    pub uri: String,
    pub digest: BTreeMap<String, String>,
}

impl SlsaResourceDescriptor {
    fn validate(&self) -> Result<(), String> {
        if !valid_uri(&self.uri) || self.digest.len() != 1 {
            return Err("SLSA resource descriptor is invalid".into());
        }
        for (algorithm, digest) in &self.digest {
            if !matches!(algorithm.as_str(), "sha256" | "gitCommit")
                || match algorithm.as_str() {
                    "sha256" => !valid_hex_sha256(digest),
                    "gitCommit" => {
                        !matches!(digest.len(), 40 | 64)
                            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                            || digest != &digest.to_ascii_lowercase()
                    }
                    _ => true,
                }
            {
                return Err("SLSA resource descriptor digest is invalid".into());
            }
        }
        Ok(())
    }

    fn matches(&self, uri: &str, algorithm: &str, digest: &str) -> bool {
        self.uri == uri
            && self
                .digest
                .get(algorithm)
                .is_some_and(|value| value == digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlsaRunDetails {
    pub builder: SlsaBuilder,
    pub metadata: SlsaRunMetadata,
}

impl SlsaRunDetails {
    fn validate(&self) -> Result<(), String> {
        self.builder.validate()?;
        if !valid_text(&self.metadata.invocation_id, 255)
            || canonical_timestamp(self.metadata.started_on) != self.metadata.started_on
            || canonical_timestamp(self.metadata.finished_on) != self.metadata.finished_on
            || self.metadata.finished_on < self.metadata.started_on
        {
            return Err("SLSA run metadata is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlsaBuilder {
    pub id: String,
    pub builder_dependencies: Vec<SlsaResourceDescriptor>,
}

impl SlsaBuilder {
    fn validate(&self) -> Result<(), String> {
        if !valid_uri(&self.id)
            || self.builder_dependencies.is_empty()
            || self.builder_dependencies.len() > 16
        {
            return Err("SLSA builder identity is invalid".into());
        }
        for dependency in &self.builder_dependencies {
            dependency.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlsaRunMetadata {
    pub invocation_id: String,
    pub started_on: DateTime<Utc>,
    pub finished_on: DateTime<Utc>,
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("could not project canonical JSON value: {error}"))?;
    let value = sort_json(value);
    let encoded = serde_json::to_vec(&value)
        .map_err(|error| format!("could not encode canonical JSON document: {error}"))?;
    if encoded.len() > MAX_CANONICAL_DOCUMENT_BYTES {
        return Err("canonical JSON document exceeds its byte bound".into());
    }
    Ok(encoded)
}

pub fn sha256_digest(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

pub fn dsse_pae(payload_type: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload_type.is_empty()
        || payload_type.len() > 255
        || !payload_type
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b' ')
        || payload.len() > MAX_CANONICAL_DOCUMENT_BYTES
    {
        return Err("DSSE payload type or body exceeds its protocol bounds".into());
    }
    let prefix = format!(
        "DSSEv1 {} {} {} ",
        payload_type.len(),
        payload_type,
        payload.len()
    );
    let mut pae = Vec::with_capacity(prefix.len().saturating_add(payload.len()));
    pae.extend_from_slice(prefix.as_bytes());
    pae.extend_from_slice(payload);
    Ok(pae)
}

fn validate_platforms(platforms: &[BuildPlatform]) -> Result<(), String> {
    if platforms.is_empty() || platforms.len() > 8 {
        return Err("build evidence platforms are invalid".into());
    }
    let mut unique = BTreeSet::new();
    for platform in platforms {
        let platform = BuildPlatform::parse(platform.as_str())?;
        if !unique.insert(platform) {
            return Err("build evidence platforms must be unique".into());
        }
    }
    Ok(())
}

fn validate_checksums(checksums: &[SpdxChecksum]) -> Result<(), String> {
    if checksums.len() != 1
        || checksums[0].algorithm != "SHA256"
        || !valid_hex_sha256(&checksums[0].checksum_value)
    {
        return Err("SPDX checksum must contain one canonical SHA-256 digest".into());
    }
    Ok(())
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value.contains(['\0', '\r', '\n'])
        && !value.chars().any(char::is_control)
}

fn valid_uri(value: &str) -> bool {
    value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && url::Url::parse(value).is_ok_and(|url| {
            !url.scheme().is_empty()
                && !url.cannot_be_a_base()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

fn valid_spdx_id(value: &str) -> bool {
    value.strip_prefix("SPDXRef-").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 255
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    })
}

fn valid_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest_hex(value: &str) -> Result<&str, String> {
    value
        .strip_prefix("sha256:")
        .filter(|digest| valid_hex_sha256(digest))
        .ok_or_else(|| "build evidence digest is not canonical SHA-256".into())
}

fn sort_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, sort_json(value)))
                .collect::<BTreeMap<_, _>>();
            let mut object = Map::new();
            for (key, value) in sorted {
                object.insert(key, value);
            }
            Value::Object(object)
        }
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_nested_objects_without_changing_array_order() {
        let document = json!({
            "z": {"last": 2, "first": 1},
            "a": [{"y": 2, "x": 1}, 3]
        });

        assert_eq!(
            canonical_json(&document).expect("canonical JSON"),
            br#"{"a":[{"x":1,"y":2},3],"z":{"first":1,"last":2}}"#
        );
    }

    #[test]
    fn dsse_pae_uses_the_standard_length_delimited_encoding() {
        assert_eq!(
            dsse_pae("application/vnd.in-toto+json", br#"{"subject":[]}"#).expect("DSSE PAE"),
            b"DSSEv1 28 application/vnd.in-toto+json 14 {\"subject\":[]}".to_vec()
        );
    }
}
