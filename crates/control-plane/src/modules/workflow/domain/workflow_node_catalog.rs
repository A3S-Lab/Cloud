use a3s_cloud_contracts::{
    AppPlatformCapabilityAvailability, AppPlatformCapabilityCategory, AppPlatformParityManifest,
    WorkflowNodeKind, WorkflowNodeProfiles, WORKFLOW_NODE_PROFILES_REVISION,
    WORKFLOW_NODE_PROFILES_SCHEMA,
};
use serde::Serialize;
use std::collections::BTreeMap;

const PARITY_MANIFEST_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/app-platform/v1/parity-manifest.acl"
));
const WORKFLOW_NODE_PROFILES_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/app-platform/v1/workflow-node-profiles.acl"
));

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeCatalogAvailability {
    Unavailable,
    Internal,
    Public,
}

impl From<AppPlatformCapabilityAvailability> for WorkflowNodeCatalogAvailability {
    fn from(value: AppPlatformCapabilityAvailability) -> Self {
        match value {
            AppPlatformCapabilityAvailability::Unavailable => Self::Unavailable,
            AppPlatformCapabilityAvailability::Internal => Self::Internal,
            AppPlatformCapabilityAvailability::Public => Self::Public,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNodeCatalogEntry {
    pub capability_id: String,
    pub label: String,
    pub owner: String,
    pub gate: String,
    pub gate_state: String,
    pub dependencies: Vec<String>,
    pub availability: WorkflowNodeCatalogAvailability,
    pub kind: Option<String>,
    pub execution_class: String,
    pub semantic_profiles: Vec<String>,
    pub evidence: Vec<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNodeCatalog {
    pub schema: String,
    pub revision: String,
    pub baseline: String,
    pub parity_manifest_digest: String,
    pub profile_set_digest: String,
    pub parity_claim: bool,
    pub nodes: Vec<WorkflowNodeCatalogEntry>,
}

impl WorkflowNodeCatalog {
    pub fn checked_in() -> Result<Self, String> {
        let manifest = AppPlatformParityManifest::parse_acl(PARITY_MANIFEST_ACL)?;
        let profiles = WorkflowNodeProfiles::parse_acl(WORKFLOW_NODE_PROFILES_ACL)?;
        profiles.validate_manifest(&manifest)?;
        Self::compose(&manifest, &profiles)
    }

    fn compose(
        manifest: &AppPlatformParityManifest,
        profiles: &WorkflowNodeProfiles,
    ) -> Result<Self, String> {
        let gates = manifest
            .gates()
            .iter()
            .map(|gate| (gate.id(), gate.state()))
            .collect::<BTreeMap<_, _>>();
        let profile_by_id = profiles
            .profiles()
            .iter()
            .map(|profile| (profile.capability_id(), profile))
            .collect::<BTreeMap<_, _>>();
        let nodes = manifest
            .capabilities()
            .iter()
            .filter(|capability| capability.category() == AppPlatformCapabilityCategory::Node)
            .map(|capability| {
                let profile = profile_by_id.get(capability.id()).ok_or_else(|| {
                    format!("Workflow node {:?} lost its profile", capability.id())
                })?;
                let gate_state = gates
                    .get(capability.gate())
                    .ok_or_else(|| format!("Workflow node {:?} lost its gate", capability.id()))?;
                let availability = WorkflowNodeCatalogAvailability::from(capability.availability());
                let unavailable_reason = match availability {
                    WorkflowNodeCatalogAvailability::Public => None,
                    WorkflowNodeCatalogAvailability::Internal => Some(format!(
                        "{} is implemented for internal Workflow use but is not publicly available",
                        capability.gate()
                    )),
                    WorkflowNodeCatalogAvailability::Unavailable => Some(format!(
                        "{} is {}; required dependencies: {}",
                        capability.gate(),
                        gate_state.as_str(),
                        if capability.dependencies().is_empty() {
                            "none".to_owned()
                        } else {
                            capability.dependencies().join(", ")
                        }
                    )),
                };
                Ok(WorkflowNodeCatalogEntry {
                    capability_id: capability.id().to_owned(),
                    label: capability.label().to_owned(),
                    owner: capability.owner().to_owned(),
                    gate: capability.gate().to_owned(),
                    gate_state: gate_state.as_str().to_owned(),
                    dependencies: capability.dependencies().to_vec(),
                    availability,
                    kind: profile
                        .kind()
                        .map(WorkflowNodeKind::as_str)
                        .map(str::to_owned),
                    execution_class: profile.execution_class().as_str().to_owned(),
                    semantic_profiles: profile.semantic_profiles().to_vec(),
                    evidence: capability.evidence().to_vec(),
                    unavailable_reason,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if nodes.len() != 23 {
            return Err("Workflow node catalog must contain exactly 23 nodes".into());
        }
        Ok(Self {
            schema: WORKFLOW_NODE_PROFILES_SCHEMA.to_owned(),
            revision: WORKFLOW_NODE_PROFILES_REVISION.to_owned(),
            baseline: manifest.baseline().to_owned(),
            parity_manifest_digest: manifest.digest().to_owned(),
            profile_set_digest: profiles.digest().to_owned(),
            parity_claim: manifest.parity_claim(),
            nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::WorkflowNodeExecutionClass;

    #[test]
    fn checked_in_catalog_is_complete_and_never_infers_public_availability() {
        let catalog = WorkflowNodeCatalog::checked_in().expect("catalog");
        assert_eq!(catalog.nodes.len(), 23);
        assert!(!catalog.parity_claim);
        assert!(catalog
            .nodes
            .windows(2)
            .all(|pair| pair[0].capability_id < pair[1].capability_id));
        assert_eq!(
            catalog
                .nodes
                .iter()
                .filter(|node| matches!(
                    node.availability,
                    WorkflowNodeCatalogAvailability::Internal
                ))
                .map(|node| node.capability_id.as_str())
                .collect::<Vec<_>>(),
            [
                "node.human-input",
                "node.if-else",
                "node.list-operator",
                "node.output",
                "node.template",
                "node.user-input",
                "node.variable-aggregator",
            ]
        );
        assert!(catalog.nodes.iter().all(|node| {
            matches!(node.availability, WorkflowNodeCatalogAvailability::Public)
                == node.unavailable_reason.is_none()
        }));
    }

    #[test]
    fn catalog_retains_execution_boundaries_without_a_descriptor_registry() {
        let catalog = WorkflowNodeCatalog::checked_in().expect("catalog");
        let by_id = catalog
            .nodes
            .iter()
            .map(|node| (node.capability_id.as_str(), node))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(by_id["node.code"].owner, "executions");
        assert_eq!(by_id["node.code"].kind.as_deref(), Some("execution"));
        assert_eq!(by_id["node.http-request"].owner, "connectors");
        assert_eq!(
            by_id["node.schedule-trigger"].execution_class,
            WorkflowNodeExecutionClass::InvocationOnly.as_str()
        );
        assert_eq!(by_id["node.schedule-trigger"].kind, None);
        assert_eq!(
            by_id["node.agent"].semantic_profiles,
            ["agent.classic", "agent.release"]
        );
    }

    #[test]
    fn catalog_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorkflowNodeCatalog>();
        assert_send_sync::<WorkflowNodeCatalogEntry>();
    }
}
