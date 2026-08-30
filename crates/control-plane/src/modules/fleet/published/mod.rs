//! Immutable facts published by Fleet.
//!
//! Consumers receive a bounded Node-session and Runtime observation snapshot,
//! never Node/NodePool aggregates, enrollment credentials, commands, leases,
//! or persistence interfaces.

mod runtime_node_evidence;

pub(in crate::modules::fleet) use runtime_node_evidence::ValidatedRuntimeNodeEvidenceProjection;
pub use runtime_node_evidence::{RuntimeNodeEvidence, RUNTIME_NODE_EVIDENCE_SCHEMA};
