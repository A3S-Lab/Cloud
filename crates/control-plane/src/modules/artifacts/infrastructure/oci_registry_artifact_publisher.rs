use super::buildkit_build_service::OciLayoutBlob;
use super::runtime_build_output_validator::RuntimeBuildOutputValidator;
use crate::infrastructure::{required_registry_header, OciRegistryClient, OciRegistryClientError};
use crate::modules::artifacts::domain::entities::{validate_registry, validate_repository_prefix};
use crate::modules::artifacts::domain::{
    BuildArtifactPublicationError, BuildOutputValidationError, BuildRun, IBuildArtifactPublisher,
    OciPublicationRequest, OciPublicationTarget, PublishedOciArtifact,
};
use a3s_cloud_contracts::RegistryCredentialMaterial;
use async_trait::async_trait;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use reqwest::{Response, StatusCode};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

const DOCKER_CONTENT_DIGEST: &str = "docker-content-digest";

#[derive(Debug, Clone)]
pub struct OciRegistryArtifactPublisherOptions {
    pub registry: String,
    pub repository_prefix: String,
    pub credential_env: String,
    pub allow_anonymous: bool,
}

pub struct OciRegistryArtifactPublisher {
    client: OciRegistryClient,
    outputs: Arc<RuntimeBuildOutputValidator>,
    registry: String,
    repository_prefix: String,
    credential_env: String,
    allow_anonymous: bool,
}

impl OciRegistryArtifactPublisher {
    pub fn new(
        outputs: Arc<RuntimeBuildOutputValidator>,
        request_timeout: Duration,
        insecure_hosts: impl IntoIterator<Item = String>,
        options: OciRegistryArtifactPublisherOptions,
    ) -> Result<Self, String> {
        validate_registry(&options.registry)?;
        validate_repository_prefix(&options.repository_prefix)?;
        if options.credential_env.is_empty() {
            if !options.allow_anonymous {
                return Err(
                    "OCI publication requires a credential environment reference or explicit anonymous access"
                        .into(),
                );
            }
        } else if !valid_env_name(&options.credential_env) {
            return Err("OCI publication credential environment reference is invalid".into());
        }
        let insecure_hosts = insecure_hosts
            .into_iter()
            .filter(|host| host == &options.registry)
            .collect::<Vec<_>>();
        Ok(Self {
            client: OciRegistryClient::new(request_timeout, insecure_hosts)?,
            outputs,
            registry: options.registry,
            repository_prefix: options.repository_prefix,
            credential_env: options.credential_env,
            allow_anonymous: options.allow_anonymous,
        })
    }

    fn materialize_credential(
        &self,
    ) -> Result<Option<RegistryCredentialMaterial>, BuildArtifactPublicationError> {
        if self.credential_env.is_empty() {
            return if self.allow_anonymous {
                Ok(None)
            } else {
                Err(BuildArtifactPublicationError::Credential(
                    "registry credential reference is not configured".into(),
                ))
            };
        }
        let value = std::env::var(&self.credential_env).map_err(|_| {
            BuildArtifactPublicationError::Credential(
                "referenced registry credential Secret is unavailable".into(),
            )
        })?;
        let value = Zeroizing::new(value);
        RegistryCredentialMaterial::parse(value.as_bytes())
            .map(Some)
            .map_err(|_| {
                BuildArtifactPublicationError::Credential(
                    "referenced registry credential Secret material is invalid".into(),
                )
            })
    }

    async fn find_validated(
        &self,
        request: &OciPublicationRequest,
        validated: &super::buildkit_build_service::ValidatedBuildkitOutput,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError> {
        let root = validated
            .blobs
            .iter()
            .find(|blob| blob.digest == request.target.descriptor.digest())
            .ok_or_else(|| {
                BuildArtifactPublicationError::Integrity(
                    "validated OCI graph omitted its publication root".into(),
                )
            })?;
        if root.media_type != request.target.descriptor.media_type()
            || root.size != request.target.descriptor.size()
            || !root.is_manifest()
        {
            return Err(BuildArtifactPublicationError::Integrity(
                "validated OCI graph changed its publication root".into(),
            ));
        }
        let response = self
            .client
            .head_manifest(
                self.client
                    .manifest_url(
                        &request.target.registry,
                        &request.target.repository,
                        &root.digest,
                    )
                    .map_err(map_client_error)?,
                &request.target.repository,
                "pull,push",
                credential,
                &root.media_type,
            )
            .await
            .map_err(map_client_error)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        verify_manifest_response(response, root, "published OCI root")?;
        for blob in &validated.blobs {
            if blob.digest == root.digest {
                continue;
            }
            self.verify_blob_present(&request.target, blob, credential)
                .await?;
        }
        Ok(Some(PublishedOciArtifact::from_target(&request.target)))
    }

    async fn verify_blob_present(
        &self,
        target: &OciPublicationTarget,
        blob: &OciLayoutBlob,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<(), BuildArtifactPublicationError> {
        let response = if blob.is_manifest() {
            self.client
                .head_manifest(
                    self.client
                        .manifest_url(&target.registry, &target.repository, &blob.digest)
                        .map_err(map_client_error)?,
                    &target.repository,
                    "pull,push",
                    credential,
                    &blob.media_type,
                )
                .await
                .map_err(map_client_error)?
        } else {
            self.client
                .head_blob(
                    self.client
                        .blob_url(&target.registry, &target.repository, &blob.digest)
                        .map_err(map_client_error)?,
                    &target.repository,
                    "pull,push",
                    credential,
                )
                .await
                .map_err(map_client_error)?
        };
        if response.status() == StatusCode::NOT_FOUND {
            return Err(BuildArtifactPublicationError::Integrity(format!(
                "published OCI graph is missing {}",
                blob.digest
            )));
        }
        if blob.is_manifest() {
            verify_manifest_response(response, blob, "published OCI graph descriptor")
        } else {
            verify_blob_response(response, blob, "published OCI graph blob")
        }
    }

    async fn ensure_blob(
        &self,
        target: &OciPublicationTarget,
        layout: &Path,
        blob: &OciLayoutBlob,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<(), BuildArtifactPublicationError> {
        let url = self
            .client
            .blob_url(&target.registry, &target.repository, &blob.digest)
            .map_err(map_client_error)?;
        let response = self
            .client
            .head_blob(url, &target.repository, "pull,push", credential)
            .await
            .map_err(map_client_error)?;
        if response.status().is_success() {
            return verify_blob_response(response, blob, "existing OCI registry blob");
        }
        if response.status() != StatusCode::NOT_FOUND {
            return Err(status_error(response.status(), "probing OCI registry blob"));
        }
        let start = self
            .client
            .start_blob_upload(
                self.client
                    .upload_start_url(&target.registry, &target.repository)
                    .map_err(map_client_error)?,
                &target.repository,
                credential,
            )
            .await
            .map_err(map_client_error)?;
        if start.status() != StatusCode::ACCEPTED {
            return Err(status_error(start.status(), "starting OCI blob upload"));
        }
        let location = start.headers().get(LOCATION).cloned().ok_or_else(|| {
            BuildArtifactPublicationError::Protocol(
                "registry blob upload omitted its Location".into(),
            )
        })?;
        let upload_url = self
            .client
            .upload_completion_url(
                &target.registry,
                &target.repository,
                start.url(),
                &location,
                &blob.digest,
            )
            .map_err(map_client_error)?;
        let path = blob_path(layout, &blob.digest)?;
        let completed = self
            .client
            .complete_blob_upload(upload_url, &target.repository, credential, &path, blob.size)
            .await
            .map_err(map_client_error)?;
        if completed.status() != StatusCode::CREATED {
            return Err(status_error(
                completed.status(),
                "completing OCI blob upload",
            ));
        }
        verify_digest_header(completed.headers(), &blob.digest, "uploaded OCI blob")?;
        self.verify_blob_present(target, blob, credential).await
    }

    async fn ensure_manifest(
        &self,
        target: &OciPublicationTarget,
        layout: &Path,
        blob: &OciLayoutBlob,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<(), BuildArtifactPublicationError> {
        let url = self
            .client
            .manifest_url(&target.registry, &target.repository, &blob.digest)
            .map_err(map_client_error)?;
        let response = self
            .client
            .head_manifest(
                url.clone(),
                &target.repository,
                "pull,push",
                credential,
                &blob.media_type,
            )
            .await
            .map_err(map_client_error)?;
        if response.status().is_success() {
            return verify_manifest_response(response, blob, "existing OCI registry manifest");
        }
        if response.status() != StatusCode::NOT_FOUND {
            return Err(status_error(
                response.status(),
                "probing OCI registry manifest",
            ));
        }
        let body = read_manifest(layout, blob).await?;
        let completed = self
            .client
            .put_manifest(url, &target.repository, credential, &blob.media_type, &body)
            .await
            .map_err(map_client_error)?;
        if completed.status() != StatusCode::CREATED {
            return Err(status_error(completed.status(), "publishing OCI manifest"));
        }
        verify_digest_header(completed.headers(), &blob.digest, "published OCI manifest")?;
        self.verify_blob_present(target, blob, credential).await
    }

    async fn publish_validated(
        &self,
        request: &OciPublicationRequest,
        validated: &super::buildkit_build_service::ValidatedBuildkitOutput,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError> {
        if let Some(published) = self.find_validated(request, validated, credential).await? {
            return Ok(published);
        }
        for blob in validated.blobs.iter().filter(|blob| !blob.is_manifest()) {
            self.ensure_blob(
                &request.target,
                &validated.layout_directory,
                blob,
                credential,
            )
            .await?;
        }
        let mut manifests = validated
            .blobs
            .iter()
            .filter(|blob| blob.is_manifest())
            .collect::<Vec<_>>();
        manifests.sort_by(|left, right| {
            right
                .depth
                .cmp(&left.depth)
                .then_with(|| left.digest.cmp(&right.digest))
        });
        for manifest in manifests {
            self.ensure_manifest(
                &request.target,
                &validated.layout_directory,
                manifest,
                credential,
            )
            .await?;
        }
        self.find_validated(request, validated, credential)
            .await?
            .ok_or_else(|| {
                BuildArtifactPublicationError::Protocol(
                    "registry did not expose the completed OCI publication".into(),
                )
            })
    }
}

#[async_trait]
impl IBuildArtifactPublisher for OciRegistryArtifactPublisher {
    fn target_for(
        &self,
        build: &BuildRun,
    ) -> Result<OciPublicationTarget, BuildArtifactPublicationError> {
        let output = build.output.as_ref().ok_or_else(|| {
            BuildArtifactPublicationError::Invalid(
                "build has no validated OCI output for publication".into(),
            )
        })?;
        let repository = format!(
            "{}/{}/{}/{}/{}",
            self.repository_prefix,
            build.organization_id,
            build.project_id,
            build.environment_id,
            build.id
        );
        OciPublicationTarget::new(self.registry.clone(), repository, output.descriptor.clone())
            .map_err(BuildArtifactPublicationError::Invalid)
    }

    async fn find(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError> {
        let request = OciPublicationRequest::new(request.target.clone(), request.output.clone())
            .map_err(BuildArtifactPublicationError::Invalid)?;
        let materialized = self
            .outputs
            .materialize_validated_output(&request.output)
            .await
            .map_err(map_validation_error)?;
        let result = match self.materialize_credential() {
            Ok(credential) => {
                self.find_validated(&request, &materialized.validated, credential.as_ref())
                    .await
            }
            Err(error) => Err(error),
        };
        materialized.cleanup().await;
        result
    }

    async fn publish(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError> {
        let request = OciPublicationRequest::new(request.target.clone(), request.output.clone())
            .map_err(BuildArtifactPublicationError::Invalid)?;
        let materialized = self
            .outputs
            .materialize_validated_output(&request.output)
            .await
            .map_err(map_validation_error)?;
        let result = match self.materialize_credential() {
            Ok(credential) => {
                self.publish_validated(&request, &materialized.validated, credential.as_ref())
                    .await
            }
            Err(error) => Err(error),
        };
        materialized.cleanup().await;
        result
    }
}

fn blob_path(layout: &Path, digest: &str) -> Result<PathBuf, BuildArtifactPublicationError> {
    let digest = digest.strip_prefix("sha256:").ok_or_else(|| {
        BuildArtifactPublicationError::Integrity("OCI blob digest is invalid".into())
    })?;
    Ok(layout.join("blobs/sha256").join(digest))
}

async fn read_manifest(
    layout: &Path,
    blob: &OciLayoutBlob,
) -> Result<Vec<u8>, BuildArtifactPublicationError> {
    let path = blob_path(layout, &blob.digest)?;
    let metadata = tokio::fs::symlink_metadata(&path).await.map_err(|error| {
        BuildArtifactPublicationError::Storage(format!(
            "could not inspect OCI manifest before publication: {error}"
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != blob.size {
        return Err(BuildArtifactPublicationError::Integrity(
            "OCI manifest changed before publication".into(),
        ));
    }
    let body = tokio::fs::read(path).await.map_err(|error| {
        BuildArtifactPublicationError::Storage(format!(
            "could not read OCI manifest for publication: {error}"
        ))
    })?;
    if format!("sha256:{:x}", Sha256::digest(&body)) != blob.digest {
        return Err(BuildArtifactPublicationError::Integrity(
            "OCI manifest digest changed before publication".into(),
        ));
    }
    Ok(body)
}

fn verify_manifest_response(
    response: Response,
    blob: &OciLayoutBlob,
    context: &str,
) -> Result<(), BuildArtifactPublicationError> {
    if !response.status().is_success() {
        return Err(status_error(response.status(), context));
    }
    verify_digest_header(response.headers(), &blob.digest, context)?;
    verify_size_header(response.headers(), blob.size, context)?;
    let media_type = required_registry_header(response.headers(), CONTENT_TYPE.as_str())
        .map_err(map_client_error)?
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if media_type != blob.media_type {
        return Err(BuildArtifactPublicationError::Protocol(format!(
            "{context} changed its media type"
        )));
    }
    Ok(())
}

fn verify_blob_response(
    response: Response,
    blob: &OciLayoutBlob,
    context: &str,
) -> Result<(), BuildArtifactPublicationError> {
    if !response.status().is_success() {
        return Err(status_error(response.status(), context));
    }
    verify_digest_header(response.headers(), &blob.digest, context)?;
    verify_size_header(response.headers(), blob.size, context)
}

fn verify_digest_header(
    headers: &reqwest::header::HeaderMap,
    expected: &str,
    context: &str,
) -> Result<(), BuildArtifactPublicationError> {
    let digest =
        required_registry_header(headers, DOCKER_CONTENT_DIGEST).map_err(map_client_error)?;
    if digest != expected {
        return Err(BuildArtifactPublicationError::Protocol(format!(
            "{context} changed its content digest"
        )));
    }
    Ok(())
}

fn verify_size_header(
    headers: &reqwest::header::HeaderMap,
    expected: u64,
    context: &str,
) -> Result<(), BuildArtifactPublicationError> {
    let size = required_registry_header(headers, CONTENT_LENGTH.as_str())
        .map_err(map_client_error)?
        .parse::<u64>()
        .map_err(|_| {
            BuildArtifactPublicationError::Protocol(format!(
                "{context} returned an invalid content length"
            ))
        })?;
    if size != expected {
        return Err(BuildArtifactPublicationError::Protocol(format!(
            "{context} changed its content length"
        )));
    }
    Ok(())
}

fn status_error(status: StatusCode, context: &str) -> BuildArtifactPublicationError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            BuildArtifactPublicationError::Unauthorized
        }
        StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => {
            BuildArtifactPublicationError::Unavailable(format!("{context} returned HTTP {status}"))
        }
        status if status.is_server_error() => {
            BuildArtifactPublicationError::Unavailable(format!("{context} returned HTTP {status}"))
        }
        status => {
            BuildArtifactPublicationError::Registry(format!("{context} returned HTTP {status}"))
        }
    }
}

fn map_client_error(error: OciRegistryClientError) -> BuildArtifactPublicationError {
    match error {
        OciRegistryClientError::Invalid(message) => BuildArtifactPublicationError::Invalid(message),
        OciRegistryClientError::Unauthorized => BuildArtifactPublicationError::Unauthorized,
        OciRegistryClientError::Protocol(message) => {
            BuildArtifactPublicationError::Protocol(message)
        }
        OciRegistryClientError::Transport(message) => {
            BuildArtifactPublicationError::Unavailable(message)
        }
        OciRegistryClientError::Storage(message) => BuildArtifactPublicationError::Storage(message),
    }
}

fn map_validation_error(error: BuildOutputValidationError) -> BuildArtifactPublicationError {
    match error {
        BuildOutputValidationError::Invalid(message) => {
            BuildArtifactPublicationError::Invalid(message)
        }
        BuildOutputValidationError::Unavailable(message) => {
            BuildArtifactPublicationError::Unavailable(message)
        }
        BuildOutputValidationError::Integrity(message) => {
            BuildArtifactPublicationError::Integrity(message)
        }
        BuildOutputValidationError::Storage(message) => {
            BuildArtifactPublicationError::Storage(message)
        }
    }
}

fn valid_env_name(value: &str) -> bool {
    value.len() <= 255
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'A'..=b'Z' | b'_' => true,
            b'0'..=b'9' => index > 0,
            _ => false,
        })
}

#[cfg(test)]
#[path = "oci_registry_artifact_publisher_tests.rs"]
mod tests;
