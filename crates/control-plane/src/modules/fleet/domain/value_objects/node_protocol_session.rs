use crate::modules::shared_kernel::domain::canonical_timestamp;
use a3s_cloud_contracts::{
    NodeProtocolContractSet, NodeSessionHello, NodeSessionSelection, NodeSessionSelectionReference,
};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProtocolPolicy {
    supported: NodeProtocolContractSet,
    required: NodeProtocolContractSet,
}

impl NodeProtocolPolicy {
    pub fn new(
        supported: NodeProtocolContractSet,
        required: NodeProtocolContractSet,
    ) -> Result<Self, NodeProtocolSessionError> {
        supported
            .validate()
            .map_err(NodeProtocolSessionError::InvalidContract)?;
        required
            .validate()
            .map_err(NodeProtocolSessionError::InvalidContract)?;
        if !required.is_subset_of(&supported) {
            return Err(NodeProtocolSessionError::InvalidPolicy);
        }
        Ok(Self {
            supported,
            required,
        })
    }

    fn select(
        &self,
        offered: &NodeProtocolContractSet,
    ) -> Result<NodeProtocolContractSet, NodeProtocolSessionError> {
        let selected = NodeProtocolContractSet {
            agent_readable: offered
                .agent_readable
                .iter()
                .filter(|schema| self.supported.agent_readable.contains(schema))
                .cloned()
                .collect(),
            agent_writable: offered
                .agent_writable
                .iter()
                .filter(|schema| self.supported.agent_writable.contains(schema))
                .cloned()
                .collect(),
        };
        selected
            .validate()
            .map_err(|_| NodeProtocolSessionError::RequiredContractUnavailable)?;
        if !self.required.is_subset_of(&selected) {
            return Err(NodeProtocolSessionError::RequiredContractUnavailable);
        }
        Ok(selected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProtocolSessionRecord {
    hello: NodeSessionHello,
    selection: NodeSessionSelection,
}

impl NodeProtocolSessionRecord {
    pub fn restore(
        hello: NodeSessionHello,
        selection: NodeSessionSelection,
    ) -> Result<Self, NodeProtocolSessionError> {
        hello
            .validate()
            .map_err(NodeProtocolSessionError::InvalidContract)?;
        selection
            .validate_for(&hello, selection.selected_at)
            .map_err(NodeProtocolSessionError::InvalidContract)?;
        Ok(Self { hello, selection })
    }

    pub const fn hello(&self) -> &NodeSessionHello {
        &self.hello
    }

    pub const fn selection(&self) -> &NodeSessionSelection {
        &self.selection
    }

    pub fn reference(&self) -> Result<NodeSessionSelectionReference, NodeProtocolSessionError> {
        self.selection
            .reference()
            .map_err(NodeProtocolSessionError::InvalidContract)
    }
}

#[derive(Debug, Clone)]
pub struct NodeProtocolNegotiation {
    hello: NodeSessionHello,
    policy: NodeProtocolPolicy,
    received_at: DateTime<Utc>,
    selection_lifetime: Duration,
    proposed_session_id: Uuid,
}

impl NodeProtocolNegotiation {
    pub fn new(
        hello: NodeSessionHello,
        policy: NodeProtocolPolicy,
        received_at: DateTime<Utc>,
        selection_lifetime: Duration,
        proposed_session_id: Uuid,
    ) -> Result<Self, NodeProtocolSessionError> {
        let received_at = canonical_timestamp(received_at);
        hello
            .validate_at(received_at)
            .map_err(NodeProtocolSessionError::InvalidContract)?;
        if selection_lifetime <= Duration::zero()
            || selection_lifetime > Duration::hours(NodeSessionSelection::MAX_LIFETIME_HOURS)
            || proposed_session_id.is_nil()
        {
            return Err(NodeProtocolSessionError::InvalidSelectionDraft);
        }
        Ok(Self {
            hello,
            policy,
            received_at,
            selection_lifetime,
            proposed_session_id,
        })
    }

    pub const fn hello(&self) -> &NodeSessionHello {
        &self.hello
    }

    pub fn apply(
        &self,
        current: Option<&NodeProtocolSessionRecord>,
    ) -> Result<NodeProtocolNegotiationOutcome, NodeProtocolSessionError> {
        if let Some(current) = current {
            if current.hello == self.hello {
                return Ok(NodeProtocolNegotiationOutcome {
                    record: current.clone(),
                    replayed: true,
                });
            }
            self.validate_successor(current)?;
        } else if self.hello.previous_selection.is_some() || self.hello.hello_sequence != 1 {
            return Err(NodeProtocolSessionError::MissingSelectionPredecessor);
        }

        let contracts = self.policy.select(&self.hello.contracts)?;
        if current.is_some_and(|record| !record.selection.contracts.is_subset_of(&contracts)) {
            return Err(NodeProtocolSessionError::ProtocolDowngrade);
        }
        let generation = current
            .map(|record| record.selection.generation.checked_add(1))
            .unwrap_or(Some(1))
            .ok_or(NodeProtocolSessionError::SequenceExhausted)?;
        let expires_at = self
            .received_at
            .checked_add_signed(self.selection_lifetime)
            .ok_or(NodeProtocolSessionError::InvalidSelectionDraft)?;
        let selection = NodeSessionSelection {
            schema: NodeSessionSelection::SCHEMA.into(),
            node_id: self.hello.node_id,
            agent_instance_id: self.hello.agent_instance_id,
            session_epoch: self.hello.session_epoch,
            hello_sequence: self.hello.hello_sequence,
            session_id: self.proposed_session_id,
            generation,
            selected_at: self.received_at,
            expires_at,
            contracts,
            previous_selection: self.hello.previous_selection.clone(),
        };
        selection
            .validate_for(&self.hello, self.received_at)
            .map_err(NodeProtocolSessionError::InvalidContract)?;
        Ok(NodeProtocolNegotiationOutcome {
            record: NodeProtocolSessionRecord {
                hello: self.hello.clone(),
                selection,
            },
            replayed: false,
        })
    }

    fn validate_successor(
        &self,
        current: &NodeProtocolSessionRecord,
    ) -> Result<(), NodeProtocolSessionError> {
        if self.hello.node_id != current.hello.node_id
            || self.hello.agent_instance_id != current.hello.agent_instance_id
        {
            return Err(NodeProtocolSessionError::IdentityConflict);
        }
        if self.hello.previous_selection.as_ref() != Some(&current.reference()?) {
            return Err(NodeProtocolSessionError::SelectionChainConflict);
        }
        if self.hello.session_epoch == current.hello.session_epoch {
            let expected_sequence = current
                .hello
                .hello_sequence
                .checked_add(1)
                .ok_or(NodeProtocolSessionError::SequenceExhausted)?;
            if self.hello.hello_sequence != expected_sequence {
                return Err(NodeProtocolSessionError::HelloSequenceConflict);
            }
        } else if self.hello.hello_sequence != 1 {
            return Err(NodeProtocolSessionError::HelloSequenceConflict);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProtocolNegotiationOutcome {
    record: NodeProtocolSessionRecord,
    replayed: bool,
}

impl NodeProtocolNegotiationOutcome {
    pub const fn record(&self) -> &NodeProtocolSessionRecord {
        &self.record
    }

    pub const fn selection(&self) -> &NodeSessionSelection {
        self.record.selection()
    }

    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NodeProtocolSessionError {
    #[error("node protocol contract is invalid: {0}")]
    InvalidContract(String),
    #[error("node protocol policy is invalid")]
    InvalidPolicy,
    #[error("node protocol selection draft is invalid")]
    InvalidSelectionDraft,
    #[error("required node protocol contract is unavailable")]
    RequiredContractUnavailable,
    #[error("node protocol selection predecessor is missing")]
    MissingSelectionPredecessor,
    #[error("node protocol session identity conflicts with its head")]
    IdentityConflict,
    #[error("node protocol selection chain conflicts with its head")]
    SelectionChainConflict,
    #[error("node protocol hello sequence conflicts with its head")]
    HelloSequenceConflict,
    #[error("node protocol hello sequence is exhausted")]
    SequenceExhausted,
    #[error("node protocol selection would downgrade the active contract set")]
    ProtocolDowngrade,
}

#[cfg(test)]
#[path = "node_protocol_session_tests.rs"]
mod tests;
