use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogCompactionRange {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub first_sequence: u64,
    pub through_sequence: u64,
    pub compacted_at: DateTime<Utc>,
}

impl NodeLogCompactionRange {
    pub fn validate(&self) -> Result<(), String> {
        if self.node_id.as_uuid().is_nil()
            || self.unit_id.is_empty()
            || self.unit_id.len() > 512
            || self.unit_id.contains('\0')
            || self.generation == 0
            || self.first_sequence > self.through_sequence
        {
            return Err("log compaction range is invalid".into());
        }
        Ok(())
    }

    pub fn clipped_after(&self, after_sequence: Option<u64>) -> Option<Self> {
        let Some(after_sequence) = after_sequence else {
            return Some(self.clone());
        };
        if self.through_sequence <= after_sequence {
            return None;
        }
        let first_sequence = after_sequence
            .checked_add(1)
            .map_or(self.first_sequence, |sequence| {
                self.first_sequence.max(sequence)
            });
        Some(Self {
            first_sequence,
            ..self.clone()
        })
    }

    pub fn compacted_chunks(&self) -> u64 {
        self.through_sequence
            .saturating_sub(self.first_sequence)
            .saturating_add(1)
    }

    pub fn coalesce(mut ranges: Vec<Self>) -> Result<Vec<Self>, String> {
        for range in &ranges {
            range.validate()?;
        }
        ranges.sort_by(|left, right| {
            (
                left.node_id,
                left.unit_id.as_str(),
                left.generation,
                left.first_sequence,
            )
                .cmp(&(
                    right.node_id,
                    right.unit_id.as_str(),
                    right.generation,
                    right.first_sequence,
                ))
        });
        let mut compacted = Vec::<Self>::new();
        for range in ranges {
            if let Some(previous) = compacted.last_mut() {
                if previous.node_id == range.node_id
                    && previous.unit_id == range.unit_id
                    && previous.generation == range.generation
                    && previous.through_sequence.saturating_add(1) >= range.first_sequence
                {
                    previous.through_sequence =
                        previous.through_sequence.max(range.through_sequence);
                    previous.compacted_at = previous.compacted_at.max(range.compacted_at);
                    continue;
                }
            }
            compacted.push(range);
        }
        Ok(compacted)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeLogCompactionResult {
    pub compacted_tombstones: usize,
    pub created_ranges: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogRetentionTarget {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub object_key: String,
    pub received_at: DateTime<Utc>,
}

impl NodeLogRetentionTarget {
    pub fn validate(&self) -> Result<(), String> {
        if self.node_id.as_uuid().is_nil()
            || self.unit_id.is_empty()
            || self.unit_id.len() > 512
            || self.unit_id.contains('\0')
            || self.generation == 0
            || self.object_key.is_empty()
            || self.object_key.len() > 4096
            || self.object_key.contains('\0')
        {
            return Err("log retention target is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait ILogRetentionRepository: Send + Sync {
    async fn list_log_chunks_for_retention(
        &self,
        received_before: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<NodeLogRetentionTarget>, RepositoryError>;

    async fn mark_log_chunk_retained(
        &self,
        target: &NodeLogRetentionTarget,
        retained_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError>;

    async fn compact_log_tombstones(
        &self,
        retained_before: DateTime<Utc>,
        compacted_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<NodeLogCompactionResult, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn compaction_ranges_merge_only_adjacent_positions_of_one_generation() {
        let now = Utc::now();
        let node_id = NodeId::new();
        let other_node_id = NodeId::new();
        let ranges = NodeLogCompactionRange::coalesce(vec![
            range(node_id, "service", 1, 2, now),
            range(node_id, "service", 1, 0, now - Duration::seconds(1)),
            range(node_id, "service", 1, 1, now),
            range(node_id, "service", 1, 4, now),
            range(node_id, "other", 1, 3, now),
            range(node_id, "service", 2, 0, now),
            range(other_node_id, "service", 1, 0, now),
        ])
        .expect("valid compaction ranges");

        assert_eq!(ranges.len(), 5);
        let primary = ranges
            .iter()
            .find(|range| {
                range.node_id == node_id
                    && range.unit_id == "service"
                    && range.generation == 1
                    && range.first_sequence == 0
            })
            .expect("primary coalesced range");
        assert_eq!(primary.through_sequence, 2);
        assert_eq!(primary.compacted_at, now);
        assert!(ranges.iter().any(|range| {
            range.node_id == node_id
                && range.unit_id == "service"
                && range.generation == 1
                && range.first_sequence == 4
        }));
        assert!(ranges.iter().any(|range| {
            range.node_id == node_id && range.unit_id == "service" && range.generation == 2
        }));
        assert!(ranges.iter().any(|range| range.node_id == other_node_id));
    }

    fn range(
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
        sequence: u64,
        compacted_at: DateTime<Utc>,
    ) -> NodeLogCompactionRange {
        NodeLogCompactionRange {
            node_id,
            unit_id: unit_id.into(),
            generation,
            first_sequence: sequence,
            through_sequence: sequence,
            compacted_at,
        }
    }
}
