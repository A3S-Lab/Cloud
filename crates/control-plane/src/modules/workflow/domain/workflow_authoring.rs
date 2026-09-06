//! Domain contract for the hosted workflow authoring journal.
//!
//! The journal deliberately treats Flow operation and snapshot bytes as opaque. Flow
//! owns decoding, validation, and application of the DSL; Cloud owns the durable
//! ordering, idempotency, compare-and-swap (CAS), and cursor semantics around the
//! already validated result.

use crate::modules::shared_kernel::domain::Sha256Digest;
use std::collections::BTreeMap;

/// Maximum encoded size of one authoring operation payload.
pub const WORKFLOW_AUTHORING_OPERATION_MAX_BYTES: usize = 1024 * 1024;
/// Maximum encoded size of a portable workflow snapshot.
pub const WORKFLOW_AUTHORING_SNAPSHOT_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Maximum number of entries returned by one journal page.
pub const WORKFLOW_AUTHORING_MAX_PAGE_SIZE: usize = 256;
/// Maximum size of the idempotency operation identifier.
pub const WORKFLOW_AUTHORING_MAX_OPERATION_ID_BYTES: usize = 255;

const OPERATION_DIGEST_DOMAIN: &[u8] = b"a3s-flow-authoring-operation-v1\0";

/// An opaque, canonical operation emitted by a3s-flow.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowAuthoringOperation {
    operation_id: String,
    base_snapshot_digest: Sha256Digest,
    operation_bytes: Vec<u8>,
    operation_digest: Sha256Digest,
}

impl WorkflowAuthoringOperation {
    /// Creates an operation and computes its stable idempotency digest.
    pub fn try_new(
        operation_id: impl Into<String>,
        base_snapshot_digest: Sha256Digest,
        operation_bytes: Vec<u8>,
    ) -> Result<Self, WorkflowAuthoringError> {
        let operation_id = operation_id.into();
        validate_operation_id(&operation_id)?;
        validate_digest(&base_snapshot_digest, "base snapshot digest")?;
        validate_bytes(
            &operation_bytes,
            WORKFLOW_AUTHORING_OPERATION_MAX_BYTES,
            "operation",
        )?;
        let operation_digest = digest_operation(&base_snapshot_digest, &operation_bytes);
        Ok(Self {
            operation_id,
            base_snapshot_digest,
            operation_bytes,
            operation_digest,
        })
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn base_snapshot_digest(&self) -> &Sha256Digest {
        &self.base_snapshot_digest
    }

    /// Returns the exact bytes supplied by Flow. Cloud must not parse them.
    pub fn operation_bytes(&self) -> &[u8] {
        &self.operation_bytes
    }

    pub fn operation_digest(&self) -> &Sha256Digest {
        &self.operation_digest
    }

    /// Validates a value restored from persistence or an untrusted transport.
    pub fn validate(&self) -> Result<(), WorkflowAuthoringError> {
        validate_operation_id(&self.operation_id)?;
        validate_digest(&self.base_snapshot_digest, "base snapshot digest")?;
        validate_bytes(
            &self.operation_bytes,
            WORKFLOW_AUTHORING_OPERATION_MAX_BYTES,
            "operation",
        )?;
        let expected = digest_operation(&self.base_snapshot_digest, &self.operation_bytes);
        if self.operation_digest != expected {
            return Err(WorkflowAuthoringError::Invalid(
                "operation digest does not match operation bytes".into(),
            ));
        }
        Ok(())
    }
}

/// An opaque, canonical workflow snapshot produced and validated by a3s-flow.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowAuthoringSnapshot {
    snapshot_bytes: Vec<u8>,
    snapshot_digest: Sha256Digest,
}

impl WorkflowAuthoringSnapshot {
    pub fn try_from_bytes(snapshot_bytes: Vec<u8>) -> Result<Self, WorkflowAuthoringError> {
        validate_bytes(
            &snapshot_bytes,
            WORKFLOW_AUTHORING_SNAPSHOT_MAX_BYTES,
            "snapshot",
        )?;
        let snapshot_digest = Sha256Digest::from_bytes(&snapshot_bytes);
        Ok(Self {
            snapshot_bytes,
            snapshot_digest,
        })
    }

    /// Rehydrates a snapshot while checking the persisted digest against its bytes.
    pub fn try_from_verified_bytes(
        snapshot_bytes: Vec<u8>,
        snapshot_digest: Sha256Digest,
    ) -> Result<Self, WorkflowAuthoringError> {
        let value = Self {
            snapshot_bytes,
            snapshot_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn snapshot_bytes(&self) -> &[u8] {
        &self.snapshot_bytes
    }

    pub fn snapshot_digest(&self) -> &Sha256Digest {
        &self.snapshot_digest
    }

    pub fn validate(&self) -> Result<(), WorkflowAuthoringError> {
        validate_bytes(
            &self.snapshot_bytes,
            WORKFLOW_AUTHORING_SNAPSHOT_MAX_BYTES,
            "snapshot",
        )?;
        validate_digest(&self.snapshot_digest, "snapshot digest")?;
        let expected = Sha256Digest::from_bytes(&self.snapshot_bytes);
        if self.snapshot_digest != expected {
            return Err(WorkflowAuthoringError::Invalid(
                "snapshot digest does not match snapshot bytes".into(),
            ));
        }
        Ok(())
    }
}

/// One ordered, durable authoring journal entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowAuthoringEntry {
    sequence: u64,
    operation_id: String,
    operation_digest: Sha256Digest,
    base_snapshot_digest: Sha256Digest,
    result_snapshot_digest: Sha256Digest,
    operation_bytes: Vec<u8>,
}

impl WorkflowAuthoringEntry {
    /// Rehydrates an entry from durable columns and verifies its operation
    /// digest and bounds.
    pub fn try_from_parts(
        sequence: u64,
        operation_id: impl Into<String>,
        operation_digest: Sha256Digest,
        base_snapshot_digest: Sha256Digest,
        result_snapshot_digest: Sha256Digest,
        operation_bytes: Vec<u8>,
    ) -> Result<Self, WorkflowAuthoringError> {
        let operation_id = operation_id.into();
        let operation = WorkflowAuthoringOperation::try_new(
            operation_id.clone(),
            base_snapshot_digest.clone(),
            operation_bytes.clone(),
        )?;
        if operation.operation_digest() != &operation_digest {
            return Err(WorkflowAuthoringError::Invalid(
                "journal operation digest does not match operation bytes".into(),
            ));
        }
        let value = Self {
            sequence,
            operation_id,
            operation_digest,
            base_snapshot_digest,
            result_snapshot_digest,
            operation_bytes,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn operation_digest(&self) -> &Sha256Digest {
        &self.operation_digest
    }

    pub fn base_snapshot_digest(&self) -> &Sha256Digest {
        &self.base_snapshot_digest
    }

    pub fn result_snapshot_digest(&self) -> &Sha256Digest {
        &self.result_snapshot_digest
    }

    /// Returns the exact opaque operation bytes for cursor consumers.
    pub fn operation_bytes(&self) -> &[u8] {
        &self.operation_bytes
    }

    fn validate(&self) -> Result<(), WorkflowAuthoringError> {
        if self.sequence == 0 {
            return Err(WorkflowAuthoringError::Invalid(
                "journal sequence must be greater than zero".into(),
            ));
        }
        let operation = WorkflowAuthoringOperation {
            operation_id: self.operation_id.clone(),
            base_snapshot_digest: self.base_snapshot_digest.clone(),
            operation_bytes: self.operation_bytes.clone(),
            operation_digest: self.operation_digest.clone(),
        };
        operation.validate()?;
        validate_digest(&self.result_snapshot_digest, "result snapshot digest")
    }
}

/// Result of an append. Retries return the original entry with `replayed = true`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowAuthoringAppend {
    pub entry: WorkflowAuthoringEntry,
    pub replayed: bool,
}

/// A cursor page over ordered authoring entries.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowAuthoringPage {
    pub entries: Vec<WorkflowAuthoringEntry>,
    /// Exclusive cursor for the next page; `None` means the stream is complete.
    pub next_cursor: Option<u64>,
}

/// In-memory domain model used by the Cloud application/persistence adapters.
///
/// Persistence implementations may store entries and snapshots independently, but
/// must preserve the invariants enforced here. This type is intentionally not a DSL
/// parser and is not an execution history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowAuthoringJournal {
    initial_snapshot: WorkflowAuthoringSnapshot,
    current_snapshot: WorkflowAuthoringSnapshot,
    next_sequence: u64,
    entries: Vec<WorkflowAuthoringEntry>,
    operation_index: BTreeMap<String, usize>,
}

impl WorkflowAuthoringJournal {
    pub fn try_new(
        initial_snapshot: WorkflowAuthoringSnapshot,
    ) -> Result<Self, WorkflowAuthoringError> {
        initial_snapshot.validate()?;
        Ok(Self {
            current_snapshot: initial_snapshot.clone(),
            initial_snapshot,
            next_sequence: 1,
            entries: Vec::new(),
            operation_index: BTreeMap::new(),
        })
    }

    pub fn initial_snapshot(&self) -> &WorkflowAuthoringSnapshot {
        &self.initial_snapshot
    }

    pub fn current_snapshot(&self) -> &WorkflowAuthoringSnapshot {
        &self.current_snapshot
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[WorkflowAuthoringEntry] {
        &self.entries
    }

    /// Returns the durable result for an operation that was already appended.
    ///
    /// This is intentionally separate from [`Self::append`]. Application
    /// services can answer an authorized retry from the materialized journal
    /// without invoking Flow again; a racing writer is still reconciled by the
    /// CAS/idempotency checks in `append`.
    pub fn replay(
        &self,
        operation: &WorkflowAuthoringOperation,
    ) -> Result<Option<WorkflowAuthoringAppend>, WorkflowAuthoringError> {
        operation.validate()?;
        let Some(index) = self.operation_index.get(operation.operation_id()) else {
            return Ok(None);
        };
        let existing = &self.entries[*index];
        if existing.operation_digest() != operation.operation_digest() {
            return Err(WorkflowAuthoringError::IdempotencyConflict {
                operation_id: operation.operation_id().to_owned(),
            });
        }
        Ok(Some(WorkflowAuthoringAppend {
            entry: existing.clone(),
            replayed: true,
        }))
    }

    /// Rehydrates a journal from durable snapshots and entries.
    ///
    /// The persistence adapter must load the initial and current materialized
    /// snapshots and the contiguous entry set in sequence order. All cross-row
    /// invariants are checked before the value is returned.
    pub fn try_from_parts(
        initial_snapshot: WorkflowAuthoringSnapshot,
        current_snapshot: WorkflowAuthoringSnapshot,
        next_sequence: u64,
        entries: Vec<WorkflowAuthoringEntry>,
    ) -> Result<Self, WorkflowAuthoringError> {
        let operation_index = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.operation_id.clone(), index))
            .collect();
        let value = Self {
            initial_snapshot,
            current_snapshot,
            next_sequence,
            entries,
            operation_index,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn last_sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    pub fn entry(&self, sequence: u64) -> Option<&WorkflowAuthoringEntry> {
        if sequence == 0 {
            return None;
        }
        usize::try_from(sequence - 1)
            .ok()
            .and_then(|index| self.entries.get(index))
    }

    /// Appends a Flow-validated result using CAS and an operation-id idempotency key.
    pub fn append(
        &mut self,
        operation: WorkflowAuthoringOperation,
        result_snapshot: WorkflowAuthoringSnapshot,
    ) -> Result<WorkflowAuthoringAppend, WorkflowAuthoringError> {
        if let Some(replay) = self.replay(&operation)? {
            return Ok(replay);
        }

        result_snapshot.validate()?;
        if operation.base_snapshot_digest() != self.current_snapshot.snapshot_digest() {
            return Err(WorkflowAuthoringError::CasConflict {
                expected: self.current_snapshot.snapshot_digest().clone(),
                actual: operation.base_snapshot_digest().clone(),
            });
        }
        if self.next_sequence == u64::MAX {
            return Err(WorkflowAuthoringError::SequenceExhausted);
        }

        let sequence = self.next_sequence;
        let entry = WorkflowAuthoringEntry {
            sequence,
            operation_id: operation.operation_id,
            operation_digest: operation.operation_digest,
            base_snapshot_digest: operation.base_snapshot_digest,
            result_snapshot_digest: result_snapshot.snapshot_digest().clone(),
            operation_bytes: operation.operation_bytes,
        };
        self.operation_index
            .insert(entry.operation_id.clone(), self.entries.len());
        self.entries.push(entry.clone());
        self.current_snapshot = result_snapshot;
        self.next_sequence += 1;
        Ok(WorkflowAuthoringAppend {
            entry,
            replayed: false,
        })
    }

    /// Returns entries strictly after `after_sequence`.
    pub fn page(
        &self,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<WorkflowAuthoringPage, WorkflowAuthoringError> {
        if !(1..=WORKFLOW_AUTHORING_MAX_PAGE_SIZE).contains(&limit) {
            return Err(WorkflowAuthoringError::InvalidPageLimit {
                max: WORKFLOW_AUTHORING_MAX_PAGE_SIZE,
            });
        }
        let after_sequence = after_sequence.unwrap_or(0);
        if after_sequence > self.last_sequence() {
            return Err(WorkflowAuthoringError::InvalidCursor(after_sequence));
        }
        let start = usize::try_from(after_sequence)
            .map_err(|_| WorkflowAuthoringError::InvalidCursor(after_sequence))?;
        let end = start.saturating_add(limit).min(self.entries.len());
        let entries = self.entries[start..end].to_vec();
        let next_cursor = (end < self.entries.len()).then(|| self.entries[end - 1].sequence);
        Ok(WorkflowAuthoringPage {
            entries,
            next_cursor,
        })
    }

    /// Checks all invariants after rehydrating entries from persistence.
    pub fn validate(&self) -> Result<(), WorkflowAuthoringError> {
        self.initial_snapshot.validate()?;
        self.current_snapshot.validate()?;
        let mut previous_digest = self.initial_snapshot.snapshot_digest().clone();
        let mut operation_index = BTreeMap::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let expected_sequence =
                u64::try_from(index + 1).map_err(|_| WorkflowAuthoringError::SequenceExhausted)?;
            entry.validate()?;
            if entry.sequence != expected_sequence {
                return Err(WorkflowAuthoringError::Invalid(
                    "journal sequences must be contiguous".into(),
                ));
            }
            if entry.base_snapshot_digest() != &previous_digest {
                return Err(WorkflowAuthoringError::Invalid(
                    "journal entry base digest does not match the preceding snapshot".into(),
                ));
            }
            if operation_index
                .insert(entry.operation_id.clone(), index)
                .is_some()
            {
                return Err(WorkflowAuthoringError::Invalid(
                    "journal operation ids must be unique".into(),
                ));
            }
            previous_digest = entry.result_snapshot_digest().clone();
        }
        let expected_next = u64::try_from(self.entries.len())
            .ok()
            .and_then(|length| length.checked_add(1))
            .ok_or(WorkflowAuthoringError::SequenceExhausted)?;
        if self.next_sequence != expected_next {
            return Err(WorkflowAuthoringError::Invalid(
                "journal next sequence is inconsistent".into(),
            ));
        }
        if self.current_snapshot.snapshot_digest() != &previous_digest {
            return Err(WorkflowAuthoringError::Invalid(
                "current snapshot does not match the journal tail".into(),
            ));
        }
        if self.operation_index != operation_index {
            return Err(WorkflowAuthoringError::Invalid(
                "journal operation index is inconsistent".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowAuthoringError {
    #[error("invalid workflow authoring value: {0}")]
    Invalid(String),
    #[error("workflow authoring CAS conflict: expected base {expected}, received {actual}")]
    CasConflict {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    #[error("workflow authoring operation id was reused with different input: {operation_id}")]
    IdempotencyConflict { operation_id: String },
    #[error("workflow authoring sequence is exhausted")]
    SequenceExhausted,
    #[error("workflow authoring page limit must be between 1 and {max}")]
    InvalidPageLimit { max: usize },
    #[error("workflow authoring cursor is invalid: {0}")]
    InvalidCursor(u64),
}

fn validate_operation_id(operation_id: &str) -> Result<(), WorkflowAuthoringError> {
    if operation_id.is_empty()
        || operation_id.len() > WORKFLOW_AUTHORING_MAX_OPERATION_ID_BYTES
        || operation_id.contains(['\0', '\r', '\n'])
    {
        return Err(WorkflowAuthoringError::Invalid(
            "operation id must be 1..255 bytes and contain no NUL, CR, or LF".into(),
        ));
    }
    Ok(())
}

fn validate_bytes(bytes: &[u8], max: usize, name: &str) -> Result<(), WorkflowAuthoringError> {
    if bytes.is_empty() || bytes.len() > max {
        return Err(WorkflowAuthoringError::Invalid(format!(
            "{name} bytes must be between 1 and {max} bytes"
        )));
    }
    Ok(())
}

fn validate_digest(digest: &Sha256Digest, name: &str) -> Result<(), WorkflowAuthoringError> {
    Sha256Digest::parse(digest.as_str()).map_err(|_| {
        WorkflowAuthoringError::Invalid(format!("{name} is not a canonical sha256 digest"))
    })?;
    Ok(())
}

fn digest_operation(base_digest: &Sha256Digest, operation_bytes: &[u8]) -> Sha256Digest {
    let mut preimage = Vec::with_capacity(
        OPERATION_DIGEST_DOMAIN.len() + 8 + base_digest.as_str().len() + 8 + operation_bytes.len(),
    );
    preimage.extend_from_slice(OPERATION_DIGEST_DOMAIN);
    update_length_delimited(&mut preimage, base_digest.as_str().as_bytes());
    update_length_delimited(&mut preimage, operation_bytes);
    Sha256Digest::from_bytes(&preimage)
}

fn update_length_delimited(preimage: &mut Vec<u8>, bytes: &[u8]) {
    preimage.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    preimage.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(value: &[u8]) -> WorkflowAuthoringSnapshot {
        WorkflowAuthoringSnapshot::try_from_bytes(value.to_vec()).expect("snapshot")
    }

    fn operation(
        id: &str,
        base: &WorkflowAuthoringSnapshot,
        bytes: &[u8],
    ) -> WorkflowAuthoringOperation {
        WorkflowAuthoringOperation::try_new(id, base.snapshot_digest().clone(), bytes.to_vec())
            .expect("operation")
    }

    #[test]
    fn computes_and_verifies_snapshot_digest() {
        let value = snapshot(br#"{"name":"demo"}"#);
        assert_eq!(
            value.snapshot_digest(),
            &Sha256Digest::from_bytes(value.snapshot_bytes())
        );
        assert!(WorkflowAuthoringSnapshot::try_from_verified_bytes(
            value.snapshot_bytes().to_vec(),
            Sha256Digest::from_bytes(b"other"),
        )
        .is_err());
    }

    #[test]
    fn appends_with_cas_and_replays_same_operation() {
        let initial = snapshot(b"initial");
        let next = snapshot(b"next");
        let mut journal = WorkflowAuthoringJournal::try_new(initial.clone()).expect("journal");
        let op = operation("op-1", &initial, br#"{"kind":"set-app-name"}"#);
        let first = journal.append(op.clone(), next.clone()).expect("append");
        assert!(!first.replayed);
        assert_eq!(first.entry.sequence(), 1);
        assert_eq!(journal.current_snapshot(), &next);

        let replay = journal
            .append(op, snapshot(b"ignored-result"))
            .expect("replay");
        assert!(replay.replayed);
        assert_eq!(replay.entry, first.entry);
        assert_eq!(journal.last_sequence(), 1);
    }

    #[test]
    fn rejects_stale_base_and_conflicting_idempotency() {
        let initial = snapshot(b"initial");
        let mut journal = WorkflowAuthoringJournal::try_new(initial.clone()).expect("journal");
        let op = operation("op-1", &initial, b"one");
        journal
            .append(op.clone(), snapshot(b"next"))
            .expect("append");

        let stale = operation("op-2", &initial, b"two");
        assert!(matches!(
            journal.append(stale, snapshot(b"other")),
            Err(WorkflowAuthoringError::CasConflict { .. })
        ));

        let conflict = operation("op-1", &initial, b"different");
        assert!(matches!(
            journal.append(conflict, snapshot(b"ignored")),
            Err(WorkflowAuthoringError::IdempotencyConflict { .. })
        ));
        assert_eq!(journal.last_sequence(), 1);
    }

    #[test]
    fn pages_are_cursor_exclusive_and_bounded() {
        let initial = snapshot(b"0");
        let mut journal = WorkflowAuthoringJournal::try_new(initial.clone()).expect("journal");
        let mut current = initial;
        for index in 1..=3 {
            let next = snapshot(format!("{index}").as_bytes());
            let op = operation(
                &format!("op-{index}"),
                &current,
                format!("{index}").as_bytes(),
            );
            journal.append(op, next.clone()).expect("append");
            current = next;
        }

        let first = journal.page(None, 2).expect("page");
        assert_eq!(
            first
                .entries
                .iter()
                .map(WorkflowAuthoringEntry::sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(first.next_cursor, Some(2));
        let second = journal.page(first.next_cursor, 2).expect("page");
        assert_eq!(
            second
                .entries
                .iter()
                .map(WorkflowAuthoringEntry::sequence)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(second.next_cursor, None);
        assert!(matches!(
            journal.page(Some(4), 2),
            Err(WorkflowAuthoringError::InvalidCursor(4))
        ));
        assert!(matches!(
            journal.page(None, WORKFLOW_AUTHORING_MAX_PAGE_SIZE + 1),
            Err(WorkflowAuthoringError::InvalidPageLimit { .. })
        ));
    }

    #[test]
    fn accepts_opaque_bytes_but_enforces_bounds() {
        let binary = vec![0, 159, 255];
        let snapshot = WorkflowAuthoringSnapshot::try_from_bytes(binary.clone()).expect("snapshot");
        assert_eq!(snapshot.snapshot_bytes(), binary.as_slice());
        assert!(WorkflowAuthoringSnapshot::try_from_bytes(Vec::new()).is_err());
        assert!(WorkflowAuthoringOperation::try_new(
            "",
            snapshot.snapshot_digest().clone(),
            vec![1],
        )
        .is_err());
        assert!(WorkflowAuthoringOperation::try_new(
            "op",
            snapshot.snapshot_digest().clone(),
            vec![0; WORKFLOW_AUTHORING_OPERATION_MAX_BYTES + 1],
        )
        .is_err());
    }

    #[test]
    fn validates_the_full_chain() {
        let initial = snapshot(b"initial");
        let mut journal = WorkflowAuthoringJournal::try_new(initial.clone()).expect("journal");
        let op = operation("op", &initial, b"payload");
        journal.append(op, snapshot(b"next")).expect("append");
        assert!(journal.validate().is_ok());

        let restored = WorkflowAuthoringJournal::try_from_parts(
            journal.initial_snapshot().clone(),
            journal.current_snapshot().clone(),
            journal.last_sequence() + 1,
            journal.entries().to_vec(),
        )
        .expect("restore");
        assert_eq!(restored, journal);
    }

    #[test]
    fn serialized_transport_values_round_trip_without_parsing_opaque_bytes() {
        let initial = snapshot(b"initial");
        let mut journal = WorkflowAuthoringJournal::try_new(initial.clone()).expect("journal");
        let append = journal
            .append(operation("op", &initial, &[0, 159, 255]), snapshot(b"next"))
            .expect("append");
        let encoded = serde_json::to_vec(&append).expect("encode append");
        let decoded: WorkflowAuthoringAppend =
            serde_json::from_slice(&encoded).expect("decode append");
        assert_eq!(decoded, append);
    }
}
