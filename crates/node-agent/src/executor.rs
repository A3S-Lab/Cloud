use crate::{CommandJournalError, FileCommandJournal, JournalDecision};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandFailure, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult,
};
use a3s_runtime::contract::RuntimeInspection;
use a3s_runtime::{RuntimeClient, RuntimeError};
use chrono::Utc;
use std::sync::Arc;

pub struct CommandExecutor {
    journal: FileCommandJournal,
    runtime: Arc<dyn RuntimeClient>,
}

impl CommandExecutor {
    pub fn new(journal: FileCommandJournal, runtime: Arc<dyn RuntimeClient>) -> Self {
        Self { journal, runtime }
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
            match self.dispatch(&envelope.payload).await {
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
                Err(error) => runtime_failure(error),
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
        payload: &NodeCommandPayload,
    ) -> Result<NodeCommandResult, RuntimeError> {
        match payload {
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
                if let RuntimeInspection::Found { observation } = &inspection {
                    if observation.generation != *generation {
                        return Err(if observation.generation > *generation {
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
                        });
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
        }
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
        RuntimeError::NotFound { .. } => (FailureStatus::Rejected, "not_found", false),
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
    use a3s_cloud_contracts::{NodeCommandMetadata, NodeCommandPayload};
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
}
