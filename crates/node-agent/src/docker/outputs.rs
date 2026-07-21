use super::{artifact_error, container::container_id, DockerRuntimeDriver};
use a3s_runtime::contract::{RuntimeOutputArtifact, RuntimeUnitSpec};
use a3s_runtime::RuntimeResult;
use bollard::container::DownloadFromContainerOptions;
use bollard::models::ContainerInspectResponse;
use futures_util::TryStreamExt;
use tokio_util::io::StreamReader;

impl DockerRuntimeDriver {
    pub(super) async fn collect_outputs(
        &self,
        spec: &RuntimeUnitSpec,
        container: &ContainerInspectResponse,
    ) -> RuntimeResult<Vec<RuntimeOutputArtifact>> {
        if spec.outputs.is_empty() {
            return Ok(Vec::new());
        }
        let artifacts = self.artifacts().await?;
        let container_id = container_id(container)?;
        let mut captured = Vec::with_capacity(spec.outputs.len());
        for output in &spec.outputs {
            let stream = self
                .docker
                .download_from_container(
                    &container_id,
                    Some(DownloadFromContainerOptions {
                        path: output.path.clone(),
                    }),
                )
                .map_err(|error| std::io::Error::other(error.to_string()));
            let reader = StreamReader::new(stream);
            captured.push(
                artifacts
                    .capture_output(spec, output, Box::pin(reader))
                    .await
                    .map_err(artifact_error)?,
            );
        }
        Ok(captured)
    }
}
