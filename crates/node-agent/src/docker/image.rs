use super::{docker_error, is_status, DockerRuntimeDriver};
use a3s_runtime::contract::{ArtifactRef, RuntimeUnitSpec};
use a3s_runtime::{RuntimeError, RuntimeResult};
use bollard::image::CreateImageOptions;
use futures_util::StreamExt;

impl DockerRuntimeDriver {
    pub(super) async fn ensure_image(&self, spec: &RuntimeUnitSpec) -> RuntimeResult<String> {
        let artifact = &spec.artifact;
        let image = image_reference(artifact)?;
        match self.docker.inspect_image(&image).await {
            Ok(inspect) => verify_repo_digest(&image, &artifact.digest, inspect.repo_digests)?,
            Err(error) if is_status(&error, 404) => {
                let options = CreateImageOptions {
                    from_image: image.as_str(),
                    ..Default::default()
                };
                let credentials = self
                    .resolve_registry_credentials(spec, &registry_address(artifact)?)
                    .await?;
                let mut pull = self.docker.create_image(Some(options), None, credentials);
                while let Some(progress) = pull.next().await {
                    progress.map_err(docker_error)?;
                }
                let inspect = self
                    .docker
                    .inspect_image(&image)
                    .await
                    .map_err(docker_error)?;
                verify_repo_digest(&image, &artifact.digest, inspect.repo_digests)?;
            }
            Err(error) => return Err(docker_error(error)),
        }
        Ok(image)
    }
}

fn image_reference(artifact: &ArtifactRef) -> RuntimeResult<String> {
    artifact.validate().map_err(RuntimeError::InvalidRequest)?;
    let url = url::Url::parse(&artifact.uri)
        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
    if url.scheme() != "oci"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(RuntimeError::InvalidRequest(
            "Docker artifacts require a credential-free oci:// URI".into(),
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        RuntimeError::InvalidRequest("Docker artifact URI has no registry host".into())
    })?;
    let authority = url
        .port()
        .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"));
    let image = format!("{authority}{}", url.path());
    let expected_suffix = format!("@{}", artifact.digest);
    if !image.ends_with(&expected_suffix) || image.matches('@').count() != 1 {
        return Err(RuntimeError::InvalidRequest(
            "Docker artifact URI must end with its authoritative digest".into(),
        ));
    }
    Ok(image)
}

fn registry_address(artifact: &ArtifactRef) -> RuntimeResult<String> {
    let url = url::Url::parse(&artifact.uri)
        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
    let host = url.host().ok_or_else(|| {
        RuntimeError::InvalidRequest("Docker artifact URI has no registry host".into())
    })?;
    let host = match host {
        url::Host::Ipv6(address) => format!("[{address}]"),
        other => other.to_string(),
    };
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

fn verify_repo_digest(
    image: &str,
    digest: &str,
    repo_digests: Option<Vec<String>>,
) -> RuntimeResult<()> {
    let expected = format!("@{digest}");
    if !repo_digests
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|repo_digest| repo_digest.ends_with(&expected))
    {
        return Err(RuntimeError::Protocol(format!(
            "Docker image {image:?} does not expose the requested repository digest"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(uri: String) -> ArtifactRef {
        ArtifactRef {
            uri,
            digest: format!("sha256:{}", "a".repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        }
    }

    #[test]
    fn image_reference_requires_the_uri_to_bind_the_digest() {
        let digest = format!("sha256:{}", "a".repeat(64));
        assert_eq!(
            image_reference(&artifact(format!(
                "oci://registry.example:5443/team/app@{digest}"
            )))
            .expect("image reference"),
            format!("registry.example:5443/team/app@{digest}")
        );
        assert_eq!(
            registry_address(&artifact(format!(
                "oci://registry.example:5443/team/app@{digest}"
            )))
            .expect("registry address"),
            "registry.example:5443"
        );
        assert_eq!(
            registry_address(&artifact(format!("oci://[::1]:5443/team/app@{digest}")))
                .expect("IPv6 registry address"),
            "[::1]:5443"
        );
        assert!(
            image_reference(&artifact("oci://registry.example/team/app:latest".into())).is_err()
        );
        assert!(image_reference(&artifact(format!(
            "oci://user:secret@registry.example/team/app@{digest}"
        )))
        .is_err());
    }
}
