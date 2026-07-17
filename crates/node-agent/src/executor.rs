use crate::{
    CommandJournalError, FileCommandJournal, GatewaySnapshotInstallError,
    GatewaySnapshotInstallOutcome, GatewaySnapshotInstaller, JournalDecision,
};
use a3s_cloud_contracts::{
    GatewayAckState, NodeCommandAck, NodeCommandEnvelope, NodeCommandFailure, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeGatewayAck,
};
use a3s_runtime::contract::RuntimeInspection;
use a3s_runtime::{RuntimeClient, RuntimeError};
use chrono::Utc;
use std::sync::Arc;

pub struct CommandExecutor {
    journal: FileCommandJournal,
    runtime: Arc<dyn RuntimeClient>,
    gateway: Arc<dyn GatewaySnapshotInstaller>,
}

impl CommandExecutor {
    pub fn runtime_only(journal: FileCommandJournal, runtime: Arc<dyn RuntimeClient>) -> Self {
        Self::new(journal, runtime, Arc::new(RuntimeOnlyGatewayInstaller))
    }

    pub fn new(
        journal: FileCommandJournal,
        runtime: Arc<dyn RuntimeClient>,
        gateway: Arc<dyn GatewaySnapshotInstaller>,
    ) -> Self {
        Self {
            journal,
            runtime,
            gateway,
        }
    }

    pub async fn execute(
        &self,
        envelope: NodeCommandEnvelope,
    ) -> Result<NodeCommandAck, CommandExecutionError> {
        match self.journal.begin(envelope.clone()).await? {
            JournalDecision::Replay(acknowledgement) => return Ok(acknowledgement),
            JournalDecision::Execute => {}
        }
        let now = Utc::now();
        let outcome = if envelope.is_expired_at(now) {
            rejected("command_expired", "command expired before Runtime dispatch")
        } else {
            match self.dispatch(&envelope).await {
                Ok(_) if Utc::now() > envelope.not_after => NodeCommandOutcome::Failed {
                    failure: NodeCommandFailure {
                        code: "command_completed_after_deadline".into(),
                        message: "Runtime operation completed after the command deadline".into(),
                        retryable: true,
                    },
                },
                Ok(result) => NodeCommandOutcome::Succeeded {
                    result: Box::new(result),
                },
                Err(error) => dispatch_failure(error),
            }
        };
        self.journal
            .complete(envelope.command_id, Utc::now(), outcome)
            .await
            .map_err(Into::into)
    }

    pub fn journal(&self) -> &FileCommandJournal {
        &self.journal
    }

    async fn dispatch(
        &self,
        envelope: &NodeCommandEnvelope,
    ) -> Result<NodeCommandResult, DispatchError> {
        match &envelope.payload {
            NodeCommandPayload::RuntimeApply { request } => {
                let observation = self.runtime.apply(request).await?;
                Ok(NodeCommandResult::RuntimeApplied {
                    observation: Box::new(observation),
                })
            }
            NodeCommandPayload::RuntimeInspect {
                unit_id,
                generation,
            } => {
                let inspection = self.runtime.inspect(unit_id).await?;
                if let RuntimeInspection::Found { observation, .. } = &inspection {
                    if observation.generation != *generation {
                        return Err((if observation.generation > *generation {
                            RuntimeError::StaleGeneration {
                                unit_id: unit_id.clone(),
                                requested: *generation,
                                current: observation.generation,
                            }
                        } else {
                            RuntimeError::GenerationConflict {
                                unit_id: unit_id.clone(),
                                generation: *generation,
                            }
                        })
                        .into());
                    }
                }
                Ok(NodeCommandResult::RuntimeInspected { inspection })
            }
            NodeCommandPayload::RuntimeStop { request } => {
                let inspection = self.runtime.stop(request).await?;
                Ok(NodeCommandResult::RuntimeStopped { inspection })
            }
            NodeCommandPayload::RuntimeRemove { request } => {
                let removal = self.runtime.remove(request).await?;
                Ok(NodeCommandResult::RuntimeRemoved { removal })
            }
            NodeCommandPayload::GatewaySnapshotInstall { snapshot } => {
                let installed = self.gateway.install(snapshot).await?;
                let (state, message) = match installed {
                    GatewaySnapshotInstallOutcome::Applied => (GatewayAckState::Applied, None),
                    GatewaySnapshotInstallOutcome::Rejected { message } => {
                        (GatewayAckState::Rejected, Some(message))
                    }
                };
                let acknowledgement = NodeGatewayAck {
                    schema: NodeGatewayAck::SCHEMA.into(),
                    acknowledgement_id: uuid::Uuid::now_v7(),
                    command_id: envelope.command_id,
                    node_id: envelope.node_id,
                    revision: snapshot.revision,
                    snapshot_digest: snapshot.snapshot_digest.clone(),
                    state,
                    message,
                    acknowledged_at: Utc::now(),
                };
                acknowledgement
                    .validate_for(envelope.command_id, envelope.node_id, snapshot)
                    .map_err(|error| {
                        DispatchError::Gateway(GatewaySnapshotInstallError::Protocol(error))
                    })?;
                Ok(NodeCommandResult::GatewaySnapshotInstalled { acknowledgement })
            }
        }
    }
}

struct RuntimeOnlyGatewayInstaller;

#[async_trait::async_trait]
impl GatewaySnapshotInstaller for RuntimeOnlyGatewayInstaller {
    async fn install(
        &self,
        _snapshot: &a3s_cloud_contracts::GatewaySnapshot,
    ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError> {
        Err(GatewaySnapshotInstallError::Protocol(
            "Gateway installer is not configured for this Runtime-only executor".into(),
        ))
    }
}

enum DispatchError {
    Runtime(RuntimeError),
    Gateway(GatewaySnapshotInstallError),
}

impl From<RuntimeError> for DispatchError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<GatewaySnapshotInstallError> for DispatchError {
    fn from(error: GatewaySnapshotInstallError) -> Self {
        Self::Gateway(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandExecutionError {
    #[error(transparent)]
    Journal(#[from] CommandJournalError),
}

fn rejected(code: &str, message: &str) -> NodeCommandOutcome {
    NodeCommandOutcome::Rejected {
        failure: NodeCommandFailure {
            code: code.into(),
            message: message.into(),
            retryable: false,
        },
    }
}

fn runtime_failure(error: RuntimeError) -> NodeCommandOutcome {
    let (status, code, retryable) = match error {
        RuntimeError::InvalidRequest(_) => (FailureStatus::Rejected, "invalid_request", false),
        RuntimeError::NotFound { .. } | RuntimeError::RequestNotFound { .. } => {
            (FailureStatus::Rejected, "not_found", false)
        }
        RuntimeError::RequestConflict { .. } => {
            (FailureStatus::Rejected, "request_conflict", false)
        }
        RuntimeError::StaleGeneration { .. } => {
            (FailureStatus::Rejected, "stale_generation", false)
        }
        RuntimeError::GenerationConflict { .. } => {
            (FailureStatus::Rejected, "generation_conflict", false)
        }
        RuntimeError::DeadlineExceeded(_) => (FailureStatus::Rejected, "deadline_exceeded", false),
        RuntimeError::UnsupportedCapabilities(_) => {
            (FailureStatus::Rejected, "unsupported_capabilities", false)
        }
        RuntimeError::ProviderUnavailable(_) => {
            (FailureStatus::Failed, "provider_unavailable", true)
        }
        RuntimeError::Transport(_) => (FailureStatus::Failed, "runtime_transport", true),
        RuntimeError::Protocol(_) => (FailureStatus::Failed, "runtime_protocol", false),
    };
    let failure = NodeCommandFailure {
        code: code.into(),
        message: sanitize_error(&error.to_string()),
        retryable,
    };
    match status {
        FailureStatus::Rejected => NodeCommandOutcome::Rejected { failure },
        FailureStatus::Failed => NodeCommandOutcome::Failed { failure },
    }
}

fn dispatch_failure(error: DispatchError) -> NodeCommandOutcome {
    match error {
        DispatchError::Runtime(error) => runtime_failure(error),
        DispatchError::Gateway(error) => {
            let failure = NodeCommandFailure {
                code: error.code().into(),
                message: sanitize_error(&error.to_string()),
                retryable: error.retryable(),
            };
            if error.retryable() {
                NodeCommandOutcome::Failed { failure }
            } else {
                NodeCommandOutcome::Rejected { failure }
            }
        }
    }
}

enum FailureStatus {
    Rejected,
    Failed,
}

fn sanitize_error(message: &str) -> String {
    let message = message.replace(['\0', '\r', '\n'], " ");
    let message = message.trim();
    if message.is_empty() {
        "Runtime operation failed".into()
    } else {
        message.chars().take(16 * 1024).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{GatewaySnapshot, NodeCommandMetadata, NodeCommandPayload};
    use a3s_runtime::contract::{
        RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
        RuntimeExecResult, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation, RuntimeRemoval,
    };
    use a3s_runtime::RuntimeResult;
    use async_trait::async_trait;
    use chrono::Duration;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    struct InspectRuntime {
        calls: AtomicUsize,
        error: bool,
    }

    struct InspectGateway {
        calls: AtomicUsize,
        outcome: GatewaySnapshotInstallOutcome,
    }

    #[async_trait]
    impl GatewaySnapshotInstaller for InspectGateway {
        async fn install(
            &self,
            _snapshot: &GatewaySnapshot,
        ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.outcome.clone())
        }
    }

    fn gateway() -> Arc<InspectGateway> {
        Arc::new(InspectGateway {
            calls: AtomicUsize::new(0),
            outcome: GatewaySnapshotInstallOutcome::Applied,
        })
    }

    #[async_trait]
    impl RuntimeClient for InspectRuntime {
        async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
            Err(RuntimeError::Protocol("unused capabilities call".into()))
        }

        async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
            Err(RuntimeError::Protocol("unused apply call".into()))
        }

        async fn inspect(&self, unit_id: &str) -> RuntimeResult<RuntimeInspection> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.error {
                Err(RuntimeError::ProviderUnavailable(
                    "Docker is offline".into(),
                ))
            } else {
                Ok(RuntimeInspection::NotFound {
                    schema: RuntimeInspection::SCHEMA.into(),
                    unit_id: unit_id.into(),
                    last_generation: Some(1),
                })
            }
        }

        async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
            Err(RuntimeError::Protocol("unused stop call".into()))
        }

        async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
            Err(RuntimeError::Protocol("unused remove call".into()))
        }

        async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
            Err(RuntimeError::Protocol("unused logs call".into()))
        }

        async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
            Err(RuntimeError::Protocol("unused exec call".into()))
        }
    }

    fn command(
        node_id: Uuid,
        command_id: Uuid,
        lease_id: Uuid,
        not_after: chrono::DateTime<Utc>,
    ) -> NodeCommandEnvelope {
        let issued_at = Utc::now() - Duration::seconds(1);
        NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id,
                lease_id,
                node_id,
                sequence: 1,
                aggregate_id: Uuid::now_v7(),
                issued_at,
                not_after,
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::RuntimeInspect {
                unit_id: "service-1".into(),
                generation: 1,
            },
        )
        .expect("command")
    }

    #[tokio::test]
    async fn completed_command_replay_does_not_call_runtime_twice() {
        let directory = tempfile::tempdir().expect("journal directory");
        let node_id = Uuid::now_v7();
        let command_id = Uuid::now_v7();
        let runtime = Arc::new(InspectRuntime {
            calls: AtomicUsize::new(0),
            error: false,
        });
        let executor = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal"),
            runtime.clone(),
            gateway(),
        );
        let first = command(
            node_id,
            command_id,
            Uuid::now_v7(),
            Utc::now() + Duration::minutes(1),
        );
        let first_ack = executor.execute(first.clone()).await.expect("execute");
        let mut redelivered = first;
        redelivered.lease_id = Uuid::now_v7();
        let replayed = executor.execute(redelivered).await.expect("replay");
        assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
        assert_eq!(first_ack.outcome, replayed.outcome);
        assert_ne!(first_ack.lease_id, replayed.lease_id);
    }

    #[tokio::test]
    async fn expired_commands_do_not_reach_runtime_and_provider_errors_are_retryable() {
        let expired_directory = tempfile::tempdir().expect("expired journal directory");
        let expired_node = Uuid::now_v7();
        let runtime = Arc::new(InspectRuntime {
            calls: AtomicUsize::new(0),
            error: false,
        });
        let expired_executor = CommandExecutor::new(
            FileCommandJournal::new(expired_directory.path(), expired_node).expect("journal"),
            runtime.clone(),
            gateway(),
        );
        let expired = expired_executor
            .execute(command(
                expired_node,
                Uuid::now_v7(),
                Uuid::now_v7(),
                Utc::now() - Duration::milliseconds(1),
            ))
            .await
            .expect("expired acknowledgement");
        assert!(matches!(
            expired.outcome,
            NodeCommandOutcome::Rejected { .. }
        ));
        assert_eq!(runtime.calls.load(Ordering::SeqCst), 0);

        let failure_directory = tempfile::tempdir().expect("failure journal directory");
        let failure_node = Uuid::now_v7();
        let failing_runtime = Arc::new(InspectRuntime {
            calls: AtomicUsize::new(0),
            error: true,
        });
        let failure_executor = CommandExecutor::new(
            FileCommandJournal::new(failure_directory.path(), failure_node).expect("journal"),
            failing_runtime,
            gateway(),
        );
        let failed = failure_executor
            .execute(command(
                failure_node,
                Uuid::now_v7(),
                Uuid::now_v7(),
                Utc::now() + Duration::minutes(1),
            ))
            .await
            .expect("failure acknowledgement");
        assert!(matches!(
            failed.outcome,
            NodeCommandOutcome::Failed {
                failure: NodeCommandFailure {
                    retryable: true,
                    ..
                }
            }
        ));
    }

    #[tokio::test]
    async fn gateway_install_returns_an_exact_revision_acknowledgement() {
        let directory = tempfile::tempdir().expect("journal directory");
        let node_id = Uuid::now_v7();
        let issued_at = Utc::now() - Duration::seconds(1);
        let snapshot = GatewaySnapshot::new(3, Some(2), "management { enabled = true }\n")
            .expect("Gateway snapshot");
        let envelope = NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: Uuid::now_v7(),
                lease_id: Uuid::now_v7(),
                node_id,
                sequence: 1,
                aggregate_id: Uuid::now_v7(),
                issued_at,
                not_after: issued_at + Duration::minutes(1),
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::GatewaySnapshotInstall {
                snapshot: Box::new(snapshot.clone()),
            },
        )
        .expect("Gateway command");
        let gateway = gateway();
        let executor = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal"),
            Arc::new(InspectRuntime {
                calls: AtomicUsize::new(0),
                error: false,
            }),
            gateway.clone(),
        );
        let acknowledgement = executor
            .execute(envelope.clone())
            .await
            .expect("execute Gateway command");
        let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
            panic!("Gateway install must produce a result");
        };
        let NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } = result.as_ref()
        else {
            panic!("Gateway install returned the wrong result kind");
        };
        acknowledgement
            .validate_for(envelope.command_id, node_id, &snapshot)
            .expect("exact Gateway acknowledgement");
        assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
    }
}
