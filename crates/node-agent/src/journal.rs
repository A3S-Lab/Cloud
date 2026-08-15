mod code_run_journal;

use crate::state_file::{self, SecureStateError, StateLock};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeResourceClaimBinding, NodeResourceClaimRelease,
};
use a3s_runtime::contract::{RuntimeInspection, RuntimeUnitSpec};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLogTarget {
    pub unit_id: String,
    pub generation: u64,
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
        self.resource_claim_projection()?;
        self.code_run_bindings_projection()?;
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

    fn resource_claim_projection(
        &self,
    ) -> Result<ResourceClaimJournalProjection, CommandJournalError> {
        let mut projection = ResourceClaimJournalProjection::default();
        let mut entries = self.entries.values().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.envelope.sequence);
        for entry in entries {
            projection.apply(entry)?;
        }
        Ok(projection)
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

    pub async fn log_targets(&self) -> Result<Vec<RuntimeLogTarget>, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.log_targets_sync())
            .await
            .map_err(task_error)?
    }

    pub(crate) async fn active_resource_claim_bindings(
        &self,
    ) -> Result<Vec<NodeResourceClaimBinding>, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.active_resource_claim_bindings_sync())
            .await
            .map_err(task_error)?
    }

    pub(crate) async fn validate_runtime_resource_binding(
        &self,
        spec: RuntimeUnitSpec,
        binding: Option<NodeResourceClaimBinding>,
    ) -> Result<(), CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || {
            journal.validate_runtime_resource_binding_sync(&spec, binding.as_ref())
        })
        .await
        .map_err(task_error)?
    }

    pub(crate) async fn validate_resource_claim_release(
        &self,
        request: NodeResourceClaimRelease,
    ) -> Result<(), CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.validate_resource_claim_release_sync(&request))
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
        if let Some(state_digest) = state_mutation_digest(&envelope.payload)? {
            if let Some(current) = journal.aggregate_generations.get(&envelope.aggregate_id) {
                if envelope.generation < current.generation {
                    return Err(CommandJournalError::Conflict(format!(
                        "command generation {} regresses durable generation {}",
                        envelope.generation, current.generation
                    )));
                }
            }
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

    fn log_targets_sync(&self) -> Result<Vec<RuntimeLogTarget>, CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let journal = self.read_journal()?;
        let mut entries = journal.entries.values().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.envelope.sequence);
        let mut targets = BTreeMap::<String, RuntimeLogTarget>::new();
        for entry in entries {
            let Some(JournalCompletion {
                outcome: NodeCommandOutcome::Succeeded { result },
                ..
            }) = entry.completion.as_ref()
            else {
                continue;
            };
            match (&entry.envelope.payload, result.as_ref()) {
                (
                    NodeCommandPayload::RuntimeApply { request, .. },
                    a3s_cloud_contracts::NodeCommandResult::RuntimeApplied { .. },
                ) => {
                    let candidate = RuntimeLogTarget {
                        unit_id: request.spec.unit_id.clone(),
                        generation: request.spec.generation,
                    };
                    let replace = targets
                        .get(&candidate.unit_id)
                        .is_none_or(|current| current.generation <= candidate.generation);
                    if replace {
                        targets.insert(candidate.unit_id.clone(), candidate);
                    }
                }
                (
                    NodeCommandPayload::RuntimeRemove { request },
                    a3s_cloud_contracts::NodeCommandResult::RuntimeRemoved { .. },
                ) if targets
                    .get(&request.unit_id)
                    .is_some_and(|target| target.generation == request.generation) =>
                {
                    targets.remove(&request.unit_id);
                }
                _ => {}
            }
        }
        Ok(targets.into_values().collect())
    }

    fn active_resource_claim_bindings_sync(
        &self,
    ) -> Result<Vec<NodeResourceClaimBinding>, CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let journal = self.read_journal()?;
        Ok(journal.resource_claim_projection()?.active_bindings())
    }

    fn validate_runtime_resource_binding_sync(
        &self,
        spec: &RuntimeUnitSpec,
        binding: Option<&NodeResourceClaimBinding>,
    ) -> Result<(), CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let journal = self.read_journal()?;
        journal
            .resource_claim_projection()?
            .validate_runtime_apply(spec, binding)
    }

    fn validate_resource_claim_release_sync(
        &self,
        request: &NodeResourceClaimRelease,
    ) -> Result<(), CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        let journal = self.read_journal()?;
        journal
            .resource_claim_projection()?
            .validate_release(request)
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
        NodeCommandPayload::ResourceClaimPrepare { request } => {
            request.validate().map_err(CommandJournalError::Invalid)?;
            Ok(Some(request.claim_digest.clone()))
        }
        NodeCommandPayload::RuntimeApply {
            request,
            resource_claim,
        } => match resource_claim {
            Some(_) => payload
                .digest()
                .map(Some)
                .map_err(CommandJournalError::Invalid),
            None => request
                .spec
                .digest()
                .map(Some)
                .map_err(CommandJournalError::Invalid),
        },
        NodeCommandPayload::GatewaySnapshotInstall { snapshot } => {
            snapshot.validate().map_err(CommandJournalError::Invalid)?;
            Ok(Some(snapshot.snapshot_digest.clone()))
        }
        NodeCommandPayload::GatewaySnapshotObserve { request } => {
            request.validate().map_err(CommandJournalError::Invalid)?;
            Ok(None)
        }
        // This journal provides exact node-command replay only. A3S Code owns
        // run idempotency and permits start/cancel/recover commands for the
        // same Runtime generation, so it must not gain a second generation
        // fence here.
        NodeCommandPayload::CodeAgentCommand { .. }
        | NodeCommandPayload::DurableCellOperatorObserve { .. } => Ok(None),
        // Fleet journals remote delivery and exact command replay. The shared
        // A3S Use Manager owns assignment-generation fencing and its nested
        // package-plan/enablement-plan/apply saga. One assignment generation can therefore
        // contain several distinct Plugin Host commands without creating a
        // second lifecycle projection in the Node Agent.
        NodeCommandPayload::PluginHostCapabilitiesInspect { .. }
        | NodeCommandPayload::PluginHostPlan { .. }
        | NodeCommandPayload::PluginHostApply { .. }
        | NodeCommandPayload::PluginHostPlanEnablement { .. }
        | NodeCommandPayload::PluginHostObserve { .. } => Ok(None),
        NodeCommandPayload::BoxBuildStart { request } => request
            .binding_digest()
            .map(Some)
            .map_err(CommandJournalError::Invalid),
        NodeCommandPayload::BoxBuildInspect { .. }
        | NodeCommandPayload::BoxBuildCancel { .. }
        | NodeCommandPayload::BoxBuildRemove { .. }
        | NodeCommandPayload::RuntimeInspect { .. }
        | NodeCommandPayload::RuntimeStop { .. }
        | NodeCommandPayload::RuntimeRemove { .. } => Ok(None),
        NodeCommandPayload::ResourceClaimRelease { request } => {
            request.validate().map_err(CommandJournalError::Invalid)?;
            Ok(Some(request.claim_digest.clone()))
        }
    }
}

#[derive(Debug, Default)]
struct ResourceClaimJournalProjection {
    claims: BTreeMap<Uuid, ProjectedResourceClaim>,
}

impl ResourceClaimJournalProjection {
    fn apply(&mut self, entry: &JournalEntry) -> Result<(), CommandJournalError> {
        match &entry.envelope.payload {
            NodeCommandPayload::ResourceClaimPrepare { request } => {
                let projected = self
                    .claims
                    .entry(request.binding.claim_id)
                    .or_insert_with(|| ProjectedResourceClaim::new(request));
                projected.register_prepare(request)?;
                if entry.completion.is_some() {
                    projected.prepare_terminal = true;
                }
            }
            NodeCommandPayload::ResourceClaimRelease { request } => {
                let projected =
                    self.claims
                        .get_mut(&request.binding.claim_id)
                        .ok_or_else(|| {
                            CommandJournalError::Invalid(
                                "resource claim release has no durable prepare command".into(),
                            )
                        })?;
                projected.register_release(request)?;
            }
            NodeCommandPayload::RuntimeApply { .. }
            | NodeCommandPayload::RuntimeInspect { .. }
            | NodeCommandPayload::RuntimeStop { .. }
            | NodeCommandPayload::RuntimeRemove { .. }
            | NodeCommandPayload::DurableCellOperatorObserve { .. }
            | NodeCommandPayload::CodeAgentCommand { .. }
            | NodeCommandPayload::BoxBuildStart { .. }
            | NodeCommandPayload::BoxBuildInspect { .. }
            | NodeCommandPayload::BoxBuildCancel { .. }
            | NodeCommandPayload::BoxBuildRemove { .. }
            | NodeCommandPayload::GatewaySnapshotInstall { .. }
            | NodeCommandPayload::GatewaySnapshotObserve { .. }
            | NodeCommandPayload::PluginHostCapabilitiesInspect { .. }
            | NodeCommandPayload::PluginHostPlan { .. }
            | NodeCommandPayload::PluginHostApply { .. }
            | NodeCommandPayload::PluginHostPlanEnablement { .. }
            | NodeCommandPayload::PluginHostObserve { .. } => {}
        }

        let Some(completion) = &entry.completion else {
            return Ok(());
        };
        let NodeCommandOutcome::Succeeded { result } = &completion.outcome else {
            return Ok(());
        };
        match (&entry.envelope.payload, result.as_ref()) {
            (
                NodeCommandPayload::ResourceClaimPrepare { request },
                NodeCommandResult::ResourceClaimPrepared { prepared },
            ) => {
                prepared
                    .validate_for(request)
                    .map_err(CommandJournalError::Invalid)?;
                self.activate_prepared(request)?;
            }
            (
                NodeCommandPayload::RuntimeApply {
                    request,
                    resource_claim: Some(binding),
                },
                NodeCommandResult::RuntimeApplied { observation },
            ) => {
                self.validate_runtime_apply(&request.spec, Some(binding))?;
                binding
                    .validate_runtime_observation(observation)
                    .map_err(CommandJournalError::Invalid)?;
                let projected = self.claims.get_mut(&binding.claim_id).ok_or_else(|| {
                    CommandJournalError::Invalid(
                        "Runtime apply references an unknown resource claim".into(),
                    )
                })?;
                projected.bound = true;
                projected.runtime_fenced = false;
            }
            (
                NodeCommandPayload::RuntimeApply {
                    request,
                    resource_claim: None,
                },
                NodeCommandResult::RuntimeApplied { .. },
            ) => {
                self.validate_runtime_apply(&request.spec, None)?;
            }
            (
                NodeCommandPayload::RuntimeStop { request },
                NodeCommandResult::RuntimeStopped { inspection },
            ) if stopped_or_absent(inspection) => {
                self.mark_runtime_fenced(&request.unit_id, request.generation);
            }
            (
                NodeCommandPayload::RuntimeRemove { request },
                NodeCommandResult::RuntimeRemoved { .. },
            ) => {
                self.mark_runtime_fenced(&request.unit_id, request.generation);
            }
            (
                NodeCommandPayload::ResourceClaimRelease { request },
                NodeCommandResult::ResourceClaimReleased { released },
            ) => {
                self.validate_release(request)?;
                released
                    .validate_for(request)
                    .map_err(CommandJournalError::Invalid)?;
                let projected =
                    self.claims
                        .get_mut(&request.binding.claim_id)
                        .ok_or_else(|| {
                            CommandJournalError::Invalid(
                                "released resource claim disappeared from the journal".into(),
                            )
                        })?;
                projected.active = false;
                projected.released = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn activate_prepared(
        &mut self,
        request: &a3s_cloud_contracts::NodeResourceClaimPrepare,
    ) -> Result<(), CommandJournalError> {
        let binding = &request.binding;
        if self.claims.values().any(|candidate| {
            candidate.active
                && candidate.binding.claim_id != binding.claim_id
                && candidate.binding.runtime_unit_id == binding.runtime_unit_id
                && candidate.binding.runtime_generation == binding.runtime_generation
        }) {
            return Err(CommandJournalError::Invalid(
                "two active resource claims bind one Runtime generation".into(),
            ));
        }
        for candidate in self.claims.values().filter(|candidate| candidate.active) {
            if candidate.binding.claim_id == binding.claim_id {
                return Err(CommandJournalError::Invalid(
                    "resource claim was prepared more than once".into(),
                ));
            }
            for requested in &binding.slots {
                if requested.kind.is_shared_capacity() {
                    continue;
                }
                if candidate.binding.slots.iter().any(|slot| {
                    slot.kind == requested.kind
                        && slot.stable_resource_id == requested.stable_resource_id
                }) {
                    return Err(CommandJournalError::Invalid(format!(
                        "exclusive resource slot {} has two active Agent claims",
                        requested.stable_resource_id
                    )));
                }
            }
        }
        let projected = self.claims.get_mut(&binding.claim_id).ok_or_else(|| {
            CommandJournalError::Invalid("prepared resource claim disappeared".into())
        })?;
        projected.active = true;
        projected.released = false;
        Ok(())
    }

    fn mark_runtime_fenced(&mut self, unit_id: &str, generation: u64) {
        for projected in self.claims.values_mut().filter(|candidate| {
            candidate.active
                && candidate.binding.runtime_unit_id == unit_id
                && candidate.binding.runtime_generation == generation
        }) {
            projected.runtime_fenced = true;
        }
    }

    fn active_bindings(&self) -> Vec<NodeResourceClaimBinding> {
        self.claims
            .values()
            .filter(|claim| claim.active)
            .map(|claim| claim.binding.clone())
            .collect()
    }

    fn validate_runtime_apply(
        &self,
        spec: &RuntimeUnitSpec,
        binding: Option<&NodeResourceClaimBinding>,
    ) -> Result<(), CommandJournalError> {
        spec.validate().map_err(CommandJournalError::Invalid)?;
        let matching = self.claims.values().find(|claim| {
            claim.active
                && claim.binding.runtime_unit_id == spec.unit_id
                && claim.binding.runtime_generation == spec.generation
        });
        match (binding, matching) {
            (Some(binding), Some(projected)) if projected.binding == *binding => binding
                .validate_runtime_spec(spec)
                .map_err(CommandJournalError::Invalid),
            (Some(_), Some(_)) => Err(CommandJournalError::Conflict(
                "Runtime apply changed its prepared resource claim binding".into(),
            )),
            (Some(_), None) => Err(CommandJournalError::Conflict(
                "Runtime apply has no active prepared resource claim".into(),
            )),
            (None, Some(_)) => Err(CommandJournalError::Conflict(
                "unbound Runtime apply targets an active prepared resource claim".into(),
            )),
            (None, None) => Ok(()),
        }
    }

    fn validate_release(
        &self,
        request: &NodeResourceClaimRelease,
    ) -> Result<(), CommandJournalError> {
        request.validate().map_err(CommandJournalError::Invalid)?;
        let projected = self.claims.get(&request.binding.claim_id).ok_or_else(|| {
            CommandJournalError::Conflict(
                "resource claim release has no durable Agent preparation".into(),
            )
        })?;
        if projected.binding != request.binding
            || !projected.prepare_terminal
            || request.claim_generation <= projected.prepare_generation
            || request.claim_generation != projected.latest_generation
            || request.claim_digest == projected.prepare_digest
        {
            return Err(CommandJournalError::Conflict(
                "resource claim release does not advance its exact prepared binding".into(),
            ));
        }
        if projected.released {
            return Err(CommandJournalError::Conflict(
                "resource claim binding is already released".into(),
            ));
        }
        if projected.active && projected.bound && !projected.runtime_fenced {
            return Err(CommandJournalError::Conflict(
                "bound resource claim cannot release before Runtime stop or removal evidence"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ProjectedResourceClaim {
    binding: NodeResourceClaimBinding,
    prepare_generation: u64,
    prepare_digest: String,
    latest_generation: u64,
    prepare_terminal: bool,
    active: bool,
    bound: bool,
    runtime_fenced: bool,
    released: bool,
}

impl ProjectedResourceClaim {
    fn new(request: &a3s_cloud_contracts::NodeResourceClaimPrepare) -> Self {
        Self {
            binding: request.binding.clone(),
            prepare_generation: request.claim_generation,
            prepare_digest: request.claim_digest.clone(),
            latest_generation: request.claim_generation,
            prepare_terminal: false,
            active: false,
            bound: false,
            runtime_fenced: false,
            released: false,
        }
    }

    fn register_prepare(
        &mut self,
        request: &a3s_cloud_contracts::NodeResourceClaimPrepare,
    ) -> Result<(), CommandJournalError> {
        request.validate().map_err(CommandJournalError::Invalid)?;
        if self.binding != request.binding
            || self.prepare_generation != request.claim_generation
            || self.prepare_digest != request.claim_digest
        {
            return Err(CommandJournalError::Conflict(
                "resource claim prepare binding changed across journal replay".into(),
            ));
        }
        Ok(())
    }

    fn register_release(
        &mut self,
        request: &NodeResourceClaimRelease,
    ) -> Result<(), CommandJournalError> {
        request.validate().map_err(CommandJournalError::Invalid)?;
        if self.binding != request.binding
            || request.claim_generation <= self.prepare_generation
            || request.claim_generation < self.latest_generation
        {
            return Err(CommandJournalError::Conflict(
                "resource claim release generation or binding regressed".into(),
            ));
        }
        self.latest_generation = request.claim_generation;
        Ok(())
    }
}

fn stopped_or_absent(inspection: &RuntimeInspection) -> bool {
    match inspection {
        RuntimeInspection::NotFound { .. } => true,
        RuntimeInspection::Found { observation, .. } => observation.state.is_terminal(),
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
#[path = "journal_tests.rs"]
mod tests;
