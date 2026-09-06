//! Adapter from the Cloud authoring port to the Flow-owned DSL boundary.
//!
//! Cloud deliberately passes opaque bytes through its journal. This adapter is
//! the only Cloud infrastructure component that interprets those bytes, and it
//! delegates all parsing, structural validation, canonicalization, and
//! operation application to the public `a3s-flow` API.

use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::IWorkflowAuthoringFlowPort;
use crate::modules::workflow::domain::{WorkflowAuthoringOperation, WorkflowAuthoringSnapshot};
use a3s_flow::{
    apply_workflow_authoring_operation, canonical_workflow_authoring_snapshot, WorkflowDslError,
};
use async_trait::async_trait;

/// Production Cloud adapter for the Flow-owned stateless authoring API.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlowWorkflowAuthoringAdapter;

impl FlowWorkflowAuthoringAdapter {
    fn snapshot(bytes: Vec<u8>) -> ApplicationResult<WorkflowAuthoringSnapshot> {
        WorkflowAuthoringSnapshot::try_from_bytes(bytes).map_err(|error| {
            ApplicationError::Internal(format!(
                "Flow returned a snapshot outside the Cloud authoring contract: {error}"
            ))
        })
    }
}

#[async_trait]
impl IWorkflowAuthoringFlowPort for FlowWorkflowAuthoringAdapter {
    async fn validate_snapshot(
        &self,
        snapshot: &WorkflowAuthoringSnapshot,
    ) -> ApplicationResult<WorkflowAuthoringSnapshot> {
        let canonical = canonical_workflow_authoring_snapshot(snapshot.snapshot_bytes())
            .map_err(map_flow_error)?;
        Self::snapshot(canonical)
    }

    async fn apply_operation(
        &self,
        base_snapshot: &WorkflowAuthoringSnapshot,
        operation: &WorkflowAuthoringOperation,
    ) -> ApplicationResult<WorkflowAuthoringSnapshot> {
        if operation.base_snapshot_digest() != base_snapshot.snapshot_digest() {
            return Err(ApplicationError::Conflict(format!(
                "workflow authoring operation base digest does not match the supplied snapshot: expected {}, received {}",
                base_snapshot.snapshot_digest(),
                operation.base_snapshot_digest()
            )));
        }
        let result = apply_workflow_authoring_operation(
            base_snapshot.snapshot_bytes(),
            operation.operation_bytes(),
        )
        .map_err(map_flow_error)?;
        Self::snapshot(result)
    }
}

fn map_flow_error(error: WorkflowDslError) -> ApplicationError {
    ApplicationError::Invalid(format!("workflow authoring DSL is invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use serde_json::json;

    const SNAPSHOT: &[u8] = br#"{"version":"0.7.0","kind":"app","app":{"name":"Draft","mode":"workflow"},"workflow":{"graph":{"nodes":[],"edges":[]}}}"#;

    fn snapshot() -> WorkflowAuthoringSnapshot {
        WorkflowAuthoringSnapshot::try_from_bytes(SNAPSHOT.to_vec()).expect("snapshot")
    }

    fn operation(
        base: &WorkflowAuthoringSnapshot,
        value: serde_json::Value,
    ) -> WorkflowAuthoringOperation {
        let bytes = serde_json::to_vec(&value).expect("operation JSON");
        WorkflowAuthoringOperation::try_new("op-1", base.snapshot_digest().clone(), bytes)
            .expect("operation")
    }

    #[tokio::test]
    async fn canonicalizes_drafts_and_applies_flow_operations() {
        let adapter = FlowWorkflowAuthoringAdapter;
        let base = snapshot();
        let canonical = adapter.validate_snapshot(&base).await.expect("canonical");
        assert_eq!(canonical.snapshot_bytes(), br#"{"version":"0.7.0","kind":"app","app":{"name":"Draft","mode":"workflow"},"dependencies":[],"workflow":{"graph":{"nodes":[],"edges":[]}}}"#);

        let operation = operation(
            &canonical,
            json!({
                "kind": "add-node",
                "id": "start",
                "type": "start",
                "configuration": {}
            }),
        );
        let result = adapter
            .apply_operation(&canonical, &operation)
            .await
            .expect("result");
        let value: serde_json::Value =
            serde_json::from_slice(result.snapshot_bytes()).expect("JSON");
        assert_eq!(value["workflow"]["graph"]["nodes"][0]["id"], "start");
    }

    #[tokio::test]
    async fn rejects_digest_mismatch_and_malformed_operations_before_mutation() {
        let adapter = FlowWorkflowAuthoringAdapter;
        let base = snapshot();
        let operation = WorkflowAuthoringOperation::try_new(
            "op-1",
            Sha256Digest::from_bytes(b"other"),
            serde_json::to_vec(&json!({
                "kind": "set-app-name",
                "name": "changed"
            }))
            .expect("operation JSON"),
        )
        .expect("operation");
        assert!(matches!(
            adapter.apply_operation(&base, &operation).await,
            Err(ApplicationError::Conflict(_))
        ));

        let malformed = WorkflowAuthoringOperation::try_new(
            "op-2",
            base.snapshot_digest().clone(),
            b"{\"kind\":\"unknown\"}".to_vec(),
        )
        .expect("operation");
        assert!(matches!(
            adapter.apply_operation(&base, &malformed).await,
            Err(ApplicationError::Invalid(_))
        ));
    }
}
