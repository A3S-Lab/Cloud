use crate::state_file::{self, SecureStateError, StateLock};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandOutcome,
    NodeCommandPayload,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use uuid::Uuid;

const JOURNAL_FILE: &str = "command-journal.json";
const JOURNAL_LOCK_FILE: &str = "command-journal.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalDecision {
    Execute,
    Replay(NodeCommandAck),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalCompletion {
    completed_at: DateTime<Utc>,
    outcome: NodeCommandOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
    envelope: NodeCommandEnvelope,
    completion: Option<JournalCompletion>,
    acknowledged: bool,
}

impl JournalEntry {
    fn acknowledgement(&self) -> Result<Option<NodeCommandAck>, CommandJournalError> {
        self.completion
            .as_ref()
            .map(|completion| {
                let acknowledgement = NodeCommandAck {
                    schema: NodeCommandAck::SCHEMA.into(),
                    command_id: self.envelope.command_id,
                    lease_id: self.envelope.lease_id,
                    node_id: self.envelope.node_id,
                    sequence: self.envelope.sequence,
                    payload_digest: self.envelope.payload_digest.clone(),
                    completed_at: completion.completed_at,
                    outcome: completion.outcome.clone(),
                };
                acknowledgement
                    .validate_against(&self.envelope)
                    .map_err(CommandJournalError::Invalid)?;
                Ok(acknowledgement)
            })
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AggregateGeneration {
    generation: u64,
    #[serde(rename = "apply_spec_digest")]
    state_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandJournal {
    schema: String,
    node_id: Uuid,
    last_received_sequence: u64,
    last_acknowledged_sequence: u64,
    aggregate_generations: BTreeMap<Uuid, AggregateGeneration>,
    entries: BTreeMap<Uuid, JournalEntry>,
}

impl CommandJournal {
    const SCHEMA: &'static str = "a3s.cloud.node-command-journal.v1";

    fn empty(node_id: Uuid) -> Self {
        Self {
            schema: Self::SCHEMA.into(),
            node_id,
            last_received_sequence: 0,
            last_acknowledged_sequence: 0,
            aggregate_generations: BTreeMap::new(),
            entries: BTreeMap::new(),
        }
    }

    fn validate(&self) -> Result<(), CommandJournalError> {
        if self.schema != Self::SCHEMA || self.node_id.is_nil() {
            return Err(CommandJournalError::Invalid(
                "command journal schema or node identity is invalid".into(),
            ));
        }
        if self.last_acknowledged_sequence > self.last_received_sequence {
            return Err(CommandJournalError::Invalid(
                "acknowledged command sequence exceeds received sequence".into(),
            ));
        }
        let mut sequences = BTreeSet::new();
        for (command_id, entry) in &self.entries {
            entry
                .envelope
                .validate()
                .map_err(CommandJournalError::Invalid)?;
            if *command_id != entry.envelope.command_id
                || entry.envelope.node_id != self.node_id
                || !sequences.insert(entry.envelope.sequence)
                || entry.acknowledged && entry.completion.is_none()
            {
                return Err(CommandJournalError::Invalid(
                    "command journal entry identity is invalid".into(),
                ));
            }
            entry.acknowledgement()?;
        }
        if self.last_received_sequence == 0 {
            if !self.entries.is_empty() {
                return Err(CommandJournalError::Invalid(
                    "empty command sequence contains journal entries".into(),
                ));
            }
        } else if sequences.len()
            != usize::try_from(self.last_received_sequence).map_err(|_| {
                CommandJournalError::Invalid("command sequence exceeds platform bounds".into())
            })?
            || sequences.first() != Some(&1)
            || sequences.last() != Some(&self.last_received_sequence)
        {
            return Err(CommandJournalError::Invalid(
                "command journal sequence contains a gap".into(),
            ));
        }
        if self.contiguous_acknowledged_sequence() != self.last_acknowledged_sequence {
            return Err(CommandJournalError::Invalid(
                "command journal acknowledgement projection is inconsistent".into(),
            ));
        }
        for generation in self.aggregate_generations.values() {
            if generation.generation == 0 || !is_sha256(&generation.state_digest) {
                return Err(CommandJournalError::Invalid(
                    "command journal aggregate generation is invalid".into(),
                ));
            }
        }
        Ok(())
    }

    fn contiguous_acknowledged_sequence(&self) -> u64 {
        let by_sequence = self
            .entries
            .values()
            .map(|entry| (entry.envelope.sequence, entry.acknowledged))
            .collect::<BTreeMap<_, _>>();
        let mut sequence = 0_u64;
        while by_sequence.get(&(sequence + 1)) == Some(&true) {
            sequence += 1;
        }
        sequence
    }
}

#[derive(Debug, Clone)]
pub struct FileCommandJournal {
    root: PathBuf,
    node_id: Uuid,
}

impl FileCommandJournal {
    pub fn new(root: impl Into<PathBuf>, node_id: Uuid) -> Result<Self, CommandJournalError> {
        if node_id.is_nil() {
            return Err(CommandJournalError::Invalid(
                "command journal node ID must not be nil".into(),
            ));
        }
        Ok(Self {
            root: root.into(),
            node_id,
        })
    }

    pub async fn begin(
        &self,
        envelope: NodeCommandEnvelope,
    ) -> Result<JournalDecision, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.begin_sync(envelope))
            .await
            .map_err(task_error)?
    }

    pub async fn complete(
        &self,
        command_id: Uuid,
        completed_at: DateTime<Utc>,
        outcome: NodeCommandOutcome,
    ) -> Result<NodeCommandAck, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || {
            journal.complete_sync(command_id, completed_at, outcome)
        })
        .await
        .map_err(task_error)?
    }

    pub async fn pending_acknowledgements(
        &self,
    ) -> Result<Vec<NodeCommandAck>, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.pending_acknowledgements_sync())
            .await
            .map_err(task_error)?
    }

    pub async fn mark_acknowledged(
        &self,
        receipt: NodeCommandAckReceipt,
    ) -> Result<u64, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.mark_acknowledged_sync(receipt))
            .await
            .map_err(task_error)?
    }

    pub async fn after_sequence(&self) -> Result<u64, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.after_sequence_sync())
            .await
            .map_err(task_error)?
    }

    fn begin_sync(
        &self,
        envelope: NodeCommandEnvelope,
    ) -> Result<JournalDecision, CommandJournalError> {
        envelope.validate().map_err(CommandJournalError::Invalid)?;
        if envelope.node_id != self.node_id {
            return Err(CommandJournalError::Conflict(
                "command belongs to a different node".into(),
            ));
        }
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let mut journal = self.read_journal()?;
        if journal.entries.contains_key(&envelope.command_id) {
            let (lease_changed, decision) = {
                let existing = journal
                    .entries
                    .get_mut(&envelope.command_id)
                    .ok_or_else(|| {
                        CommandJournalError::Invalid("command journal entry disappeared".into())
                    })?;
                let mut rebound = existing.envelope.clone();
                rebound.lease_id = envelope.lease_id;
                if rebound != envelope {
                    return Err(CommandJournalError::Conflict(
                        "command ID was redelivered with different immutable content".into(),
                    ));
                }
                let lease_changed = existing.envelope.lease_id != envelope.lease_id;
                if lease_changed {
                    existing.envelope = envelope;
                }
                let decision = existing
                    .acknowledgement()?
                    .map_or(JournalDecision::Execute, JournalDecision::Replay);
                (lease_changed, decision)
            };
            if lease_changed {
                journal.validate()?;
                self.write_journal(&journal)?;
            }
            return Ok(decision);
        }
        let expected = journal
            .last_received_sequence
            .checked_add(1)
            .ok_or_else(|| CommandJournalError::Invalid("command sequence overflowed".into()))?;
        if envelope.sequence != expected {
            return Err(CommandJournalError::Conflict(format!(
                "command sequence {} does not follow durable sequence {}",
                envelope.sequence, journal.last_received_sequence
            )));
        }
        if let Some(current) = journal.aggregate_generations.get(&envelope.aggregate_id) {
            if envelope.generation < current.generation {
                return Err(CommandJournalError::Conflict(format!(
                    "command generation {} regresses durable generation {}",
                    envelope.generation, current.generation
                )));
            }
        }
        if let Some(state_digest) = state_mutation_digest(&envelope.payload)? {
            match journal.aggregate_generations.get(&envelope.aggregate_id) {
                Some(current)
                    if current.generation == envelope.generation
                        && current.state_digest != state_digest =>
                {
                    return Err(CommandJournalError::Conflict(
                        "state-changing command generation has conflicting content".into(),
                    ));
                }
                _ => {
                    journal.aggregate_generations.insert(
                        envelope.aggregate_id,
                        AggregateGeneration {
                            generation: envelope.generation,
                            state_digest,
                        },
                    );
                }
            }
        }
        journal.last_received_sequence = envelope.sequence;
        journal.entries.insert(
            envelope.command_id,
            JournalEntry {
                envelope,
                completion: None,
                acknowledged: false,
            },
        );
        journal.validate()?;
        self.write_journal(&journal)?;
        Ok(JournalDecision::Execute)
    }

    fn complete_sync(
        &self,
        command_id: Uuid,
        completed_at: DateTime<Utc>,
        outcome: NodeCommandOutcome,
    ) -> Result<NodeCommandAck, CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let mut journal = self.read_journal()?;
        let entry = journal
            .entries
            .get_mut(&command_id)
            .ok_or_else(|| CommandJournalError::Conflict("command was not journaled".into()))?;
        if let Some(existing) = &entry.completion {
            if existing.outcome != outcome {
                return Err(CommandJournalError::Conflict(
                    "completed command outcome changed across replay".into(),
                ));
            }
            return entry.acknowledgement()?.ok_or_else(|| {
                CommandJournalError::Invalid("completed command has no acknowledgement".into())
            });
        }
        entry.completion = Some(JournalCompletion {
            completed_at,
            outcome,
        });
        let acknowledgement = entry.acknowledgement()?.ok_or_else(|| {
            CommandJournalError::Invalid("completed command has no acknowledgement".into())
        })?;
        journal.validate()?;
        self.write_journal(&journal)?;
        Ok(acknowledgement)
    }

    fn pending_acknowledgements_sync(&self) -> Result<Vec<NodeCommandAck>, CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let journal = self.read_journal()?;
        let mut entries = journal
            .entries
            .values()
            .filter(|entry| !entry.acknowledged && entry.completion.is_some())
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.envelope.sequence);
        entries
            .into_iter()
            .map(|entry| {
                entry.acknowledgement()?.ok_or_else(|| {
                    CommandJournalError::Invalid("pending acknowledgement has no completion".into())
                })
            })
            .collect()
    }

    fn mark_acknowledged_sync(
        &self,
        receipt: NodeCommandAckReceipt,
    ) -> Result<u64, CommandJournalError> {
        receipt.validate().map_err(CommandJournalError::Invalid)?;
        if receipt.node_id != self.node_id {
            return Err(CommandJournalError::Conflict(
                "acknowledgement receipt belongs to a different node".into(),
            ));
        }
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let mut journal = self.read_journal()?;
        let entry = journal
            .entries
            .get_mut(&receipt.command_id)
            .ok_or_else(|| {
                CommandJournalError::Conflict("acknowledgement receipt command is unknown".into())
            })?;
        if entry.completion.is_none() {
            return Err(CommandJournalError::Conflict(
                "command was acknowledged before durable completion".into(),
            ));
        }
        entry.acknowledged = true;
        journal.last_acknowledged_sequence = journal.contiguous_acknowledged_sequence();
        journal.validate()?;
        self.write_journal(&journal)?;
        Ok(journal.last_acknowledged_sequence)
    }

    fn after_sequence_sync(&self) -> Result<u64, CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        Ok(self.read_journal()?.last_acknowledged_sequence)
    }

    fn read_journal(&self) -> Result<CommandJournal, CommandJournalError> {
        let path = self.root.join(JOURNAL_FILE);
        let journal: CommandJournal = state_file::read_json(&path, "node command journal")?
            .unwrap_or_else(|| CommandJournal::empty(self.node_id));
        if journal.node_id != self.node_id {
            return Err(CommandJournalError::Conflict(
                "command journal belongs to a different node".into(),
            ));
        }
        journal.validate()?;
        Ok(journal)
    }

    fn write_journal(&self, journal: &CommandJournal) -> Result<(), CommandJournalError> {
        state_file::atomic_write(&self.root.join(JOURNAL_FILE), journal).map_err(Into::into)
    }
}

fn state_mutation_digest(
    payload: &NodeCommandPayload,
) -> Result<Option<String>, CommandJournalError> {
    match payload {
        NodeCommandPayload::RuntimeApply { request } => request
            .spec
            .digest()
            .map(Some)
            .map_err(CommandJournalError::Invalid),
        NodeCommandPayload::GatewaySnapshotInstall { snapshot } => {
            snapshot.validate().map_err(CommandJournalError::Invalid)?;
            Ok(Some(snapshot.snapshot_digest.clone()))
        }
        NodeCommandPayload::RuntimeInspect { .. }
        | NodeCommandPayload::RuntimeStop { .. }
        | NodeCommandPayload::RuntimeRemove { .. } => Ok(None),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandJournalError {
    #[error("invalid command journal: {0}")]
    Invalid(String),
    #[error("command journal conflict: {0}")]
    Conflict(String),
    #[error("command journal storage failed: {0}")]
    Storage(String),
}

impl From<SecureStateError> for CommandJournalError {
    fn from(error: SecureStateError) -> Self {
        match error {
            SecureStateError::Invalid(message) => Self::Invalid(message),
            SecureStateError::Storage(message) => Self::Storage(message),
        }
    }
}

fn task_error(error: tokio::task::JoinError) -> CommandJournalError {
    CommandJournalError::Storage(format!("command journal task failed: {error}"))
}

fn is_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{NodeCommandMetadata, NodeCommandResult};
    use a3s_runtime::contract::{
        ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
        RuntimeApplyRequest, RuntimeInspection, RuntimeNetworkSpec, RuntimeProcessSpec,
        RuntimeUnitClass, RuntimeUnitSpec,
    };
    use chrono::Duration;
    use std::collections::BTreeMap;

    fn envelope(
        node_id: Uuid,
        command_id: Uuid,
        lease_id: Uuid,
        sequence: u64,
        aggregate_id: Uuid,
        generation: u64,
    ) -> NodeCommandEnvelope {
        let issued_at = Utc::now();
        NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id,
                lease_id,
                node_id,
                sequence,
                aggregate_id,
                issued_at,
                not_after: issued_at + Duration::minutes(1),
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::RuntimeInspect {
                unit_id: "service-1".into(),
                generation,
            },
        )
        .expect("command envelope")
    }

    fn outcome() -> NodeCommandOutcome {
        NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeInspected {
                inspection: RuntimeInspection::NotFound {
                    schema: RuntimeInspection::SCHEMA.into(),
                    unit_id: "service-1".into(),
                    last_generation: Some(1),
                },
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_envelope(
        node_id: Uuid,
        command_id: Uuid,
        sequence: u64,
        aggregate_id: Uuid,
        request_id: &str,
        artifact: char,
    ) -> NodeCommandEnvelope {
        let issued_at = Utc::now();
        let digest = format!("sha256:{}", artifact.to_string().repeat(64));
        let spec = RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.into(),
            unit_id: "service-1".into(),
            generation: 1,
            class: RuntimeUnitClass::Service,
            artifact: ArtifactRef {
                uri: format!("oci://registry.example/app@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: RuntimeProcessSpec {
                command: Vec::new(),
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            mounts: Vec::new(),
            secrets: Vec::new(),
            network: RuntimeNetworkSpec {
                mode: NetworkMode::None,
                ports: Vec::new(),
            },
            resources: ResourceLimits {
                cpu_millis: 100,
                memory_bytes: 32 * 1024 * 1024,
                pids: 32,
                ephemeral_storage_bytes: None,
                execution_timeout_ms: None,
            },
            isolation: IsolationLevel::Container,
            health: None,
            restart: RestartPolicy::Always,
            outputs: Vec::new(),
            semantics_profile_digest: None,
        };
        NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id,
                lease_id: Uuid::now_v7(),
                node_id,
                sequence,
                aggregate_id,
                issued_at,
                not_after: issued_at + Duration::minutes(1),
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: request_id.into(),
                    deadline_at_ms: None,
                    spec,
                }),
            },
        )
        .expect("Runtime apply envelope")
    }

    #[tokio::test]
    async fn completed_command_rebinds_a_new_lease_without_reexecuting() {
        let directory = tempfile::tempdir().expect("journal directory");
        let node_id = Uuid::now_v7();
        let command_id = Uuid::now_v7();
        let aggregate_id = Uuid::now_v7();
        let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
        let first = envelope(node_id, command_id, Uuid::now_v7(), 1, aggregate_id, 1);
        assert_eq!(
            journal.begin(first.clone()).await.expect("begin command"),
            JournalDecision::Execute
        );
        let completed_at = Utc::now();
        let first_ack = journal
            .complete(command_id, completed_at, outcome())
            .await
            .expect("complete command");
        let mut redelivered = first;
        redelivered.lease_id = Uuid::now_v7();
        let replay = journal
            .begin(redelivered.clone())
            .await
            .expect("redeliver command");
        let replay_ack = match replay {
            JournalDecision::Replay(value) => value,
            JournalDecision::Execute => panic!("completed command must not execute again"),
        };
        assert_ne!(first_ack.lease_id, replay_ack.lease_id);
        assert_eq!(replay_ack.lease_id, redelivered.lease_id);
        assert_eq!(replay_ack.outcome, first_ack.outcome);
        assert_eq!(replay_ack.completed_at, first_ack.completed_at);
        assert_eq!(journal.after_sequence().await.expect("after sequence"), 0);
        let receipt = NodeCommandAckReceipt {
            schema: NodeCommandAckReceipt::SCHEMA.into(),
            command_id,
            node_id,
            replayed: false,
        };
        assert_eq!(
            journal
                .mark_acknowledged(receipt)
                .await
                .expect("mark acknowledged"),
            1
        );
        assert!(journal
            .pending_acknowledgements()
            .await
            .expect("pending acknowledgements")
            .is_empty());
    }

    #[tokio::test]
    async fn journal_rejects_sequence_gaps_and_command_content_conflicts() {
        let directory = tempfile::tempdir().expect("journal directory");
        let node_id = Uuid::now_v7();
        let command_id = Uuid::now_v7();
        let aggregate_id = Uuid::now_v7();
        let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");
        assert!(journal
            .begin(envelope(
                node_id,
                command_id,
                Uuid::now_v7(),
                2,
                aggregate_id,
                1,
            ))
            .await
            .is_err());
        let first = envelope(node_id, command_id, Uuid::now_v7(), 1, aggregate_id, 1);
        journal.begin(first.clone()).await.expect("first command");
        let mut conflict = first;
        conflict.payload = NodeCommandPayload::RuntimeInspect {
            unit_id: "different-service".into(),
            generation: 1,
        };
        conflict.payload_digest = conflict.payload.digest().expect("payload digest");
        assert!(journal.begin(conflict).await.is_err());
    }

    #[tokio::test]
    async fn same_generation_recovery_apply_allows_a_new_request_for_the_same_spec() {
        let directory = tempfile::tempdir().expect("journal directory");
        let node_id = Uuid::now_v7();
        let aggregate_id = Uuid::now_v7();
        let journal = FileCommandJournal::new(directory.path(), node_id).expect("journal");

        journal
            .begin(apply_envelope(
                node_id,
                Uuid::now_v7(),
                1,
                aggregate_id,
                "deployment-apply",
                'a',
            ))
            .await
            .expect("initial apply");
        assert_eq!(
            journal
                .begin(apply_envelope(
                    node_id,
                    Uuid::now_v7(),
                    2,
                    aggregate_id,
                    "recovery-apply",
                    'a',
                ))
                .await
                .expect("same-spec recovery apply"),
            JournalDecision::Execute
        );
        assert!(journal
            .begin(apply_envelope(
                node_id,
                Uuid::now_v7(),
                3,
                aggregate_id,
                "conflicting-apply",
                'b',
            ))
            .await
            .is_err());
    }
}
