use super::fixture::{connect_driver, found, require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_cloud_contracts::NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE;
use a3s_runtime::contract::{RuntimeOutputSpec, RuntimeUnitState};
use a3s_runtime::{RuntimeClient, RuntimeResult};
use sha2::{Digest, Sha256};
use std::sync::Arc;

impl DockerConformanceFixture {
    pub(crate) async fn run_outputs(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let mut spec = specs::task_spec(
            specs::unit_id(&self.namespace, "output-exact"),
            "mkdir -p /outputs/result && printf 'deterministic-output' >/outputs/result/value",
        );
        spec.outputs = vec![RuntimeOutputSpec {
            name: "result".into(),
            path: "/outputs/result".into(),
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            max_bytes: 1024 * 1024,
        }];
        let apply = specs::apply("output-exact-apply", spec.clone());
        let first = client.apply(&apply).await?;
        require(
            first.state == RuntimeUnitState::Succeeded && first.outputs.len() == 1,
            "Docker Task did not collect its declared output",
        )?;
        let resource = resource_id(&first)?.to_owned();
        let output = first.outputs[0].clone();
        require(
            output.name == "result"
                && output.artifact.media_type == NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
                && output
                    .artifact
                    .uri
                    .starts_with("a3s-node-artifact://sha256/")
                && output.size_bytes > 0
                && output.size_bytes <= spec.outputs[0].max_bytes,
            "Docker Task output changed its typed name, media type, URI, or bound",
        )?;
        let bytes = self.artifacts.read_blob(&output.artifact).await?;
        require(
            bytes.len() as u64 == output.size_bytes
                && format!("sha256:{:x}", Sha256::digest(&bytes)) == output.artifact.digest,
            "Docker Task output bytes do not match their reported size and digest",
        )?;

        let replay = client.apply(&apply).await?;
        require(
            replay == first,
            "Docker Task output changed during exact apply replay",
        )?;
        let restarted_driver = Arc::new(
            connect_driver(&self.namespace, self.node_id, self.artifacts.manager()).await?,
        );
        let reconstructed = found(
            self.inspect_driver(restarted_driver.as_ref(), &spec.unit_id)
                .await?,
        )?;
        require(
            reconstructed.outputs == first.outputs
                && resource_id(&reconstructed)? == resource
                && reconstructed.state == RuntimeUnitState::Succeeded,
            "Docker Task output identity changed after client and driver reconstruction",
        )?;

        let mut oversized = specs::task_spec(
            specs::unit_id(&self.namespace, "output-bounded"),
            "mkdir -p /outputs/result && printf 'too-large' >/outputs/result/value",
        );
        oversized.outputs = vec![RuntimeOutputSpec {
            name: "result".into(),
            path: "/outputs/result".into(),
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            max_bytes: 1,
        }];
        require(
            client
                .apply(&specs::apply("output-bounded-apply", oversized.clone()))
                .await
                .is_err(),
            "Docker Task accepted output bytes beyond the declared bound",
        )?;
        client
            .remove(&specs::action("output-bounded-remove", &oversized))
            .await?;
        require(
            self.artifacts.spec_views_absent(&oversized).await?,
            "Docker oversized-output cleanup retained a spec view",
        )?;

        self.artifacts
            .tamper_blob_same_length(&output.artifact)
            .await?;
        require(
            self.inspect_driver(restarted_driver.as_ref(), &spec.unit_id)
                .await
                .is_err(),
            "Docker Task output digest tampering survived inspection",
        )?;
        let restarted = self.restarted_client(restarted_driver);
        restarted
            .remove(&specs::action("output-exact-remove", &spec))
            .await?;
        require(
            self.artifacts.spec_views_absent(&spec).await?
                && self.artifacts.blob_absent(&output.artifact).await?,
            "Docker Task output removal retained its view or unreferenced blob",
        )?;
        eprintln!(
            "A3S_RUNTIME_OUTPUTS_CASE_PASS case=OUTPUT-EXACT-BOUNDED digest_binding=true replay=true restart=true cleanup=true"
        );
        Ok(())
    }
}
