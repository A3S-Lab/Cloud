use super::notification::{encode_descending_cursor, parse_descending_cursor, NotificationScope};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, NodeId, NotificationAlertPolicyId, OrganizationId,
    PrincipalId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{boolean, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

pub const NOTIFICATION_ALERT_POLICY_SCHEMA: &str = "cloud.notification.alert-policy.v1";
pub const NOTIFICATION_ALERT_POLICY_SCHEMA_V2: &str = "cloud.notification.alert-policy.v2";
pub const NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES: usize = 16 * 1024;
const NOTIFICATION_ALERT_POLICY_BLOCK: &str = "notification_alert_policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAlertSource {
    EdgeDomainClaimStatusV1,
    EdgeGatewayCertificateRenewalStatusV1,
    WorkloadDeploymentHealthV1,
    EdgeGatewayCertificateExpiryStatusV1,
    FleetNodeAvailabilityStatusV1,
}

impl NotificationAlertSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EdgeDomainClaimStatusV1 => "edge.domain-claim-status.v1",
            Self::EdgeGatewayCertificateRenewalStatusV1 => {
                "edge.gateway-certificate-renewal-status.v1"
            }
            Self::WorkloadDeploymentHealthV1 => "workload.deployment-health.v1",
            Self::EdgeGatewayCertificateExpiryStatusV1 => {
                "edge.gateway-certificate-expiry-status.v1"
            }
            Self::FleetNodeAvailabilityStatusV1 => "fleet.node-availability-status.v1",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "edge.domain-claim-status.v1" => Ok(Self::EdgeDomainClaimStatusV1),
            "edge.gateway-certificate-renewal-status.v1" => {
                Ok(Self::EdgeGatewayCertificateRenewalStatusV1)
            }
            "workload.deployment-health.v1" => Ok(Self::WorkloadDeploymentHealthV1),
            "edge.gateway-certificate-expiry-status.v1" => {
                Ok(Self::EdgeGatewayCertificateExpiryStatusV1)
            }
            "fleet.node-availability-status.v1" => Ok(Self::FleetNodeAvailabilityStatusV1),
            _ => Err("notification alert source is unsupported".into()),
        }
    }

    pub const fn event_keys(self) -> &'static [&'static str] {
        match self {
            Self::EdgeDomainClaimStatusV1 => {
                &["edge.domain-claim.rejected", "edge.domain-claim.verified"]
            }
            Self::EdgeGatewayCertificateRenewalStatusV1 => &[
                "edge.gateway-certificate.renewal-failed",
                "edge.gateway-certificate.renewed",
            ],
            Self::WorkloadDeploymentHealthV1 => {
                &["workload.deployment.failed", "workload.deployment.healthy"]
            }
            Self::EdgeGatewayCertificateExpiryStatusV1 => &[
                "edge.gateway-certificate.expiring",
                "edge.gateway-certificate.expiry-resolved",
            ],
            Self::FleetNodeAvailabilityStatusV1 => {
                &["fleet.node.unavailable", "fleet.node.availability-resolved"]
            }
        }
    }

    pub const fn definition_schema(self) -> &'static str {
        match self {
            Self::FleetNodeAvailabilityStatusV1 => NOTIFICATION_ALERT_POLICY_SCHEMA_V2,
            Self::EdgeDomainClaimStatusV1
            | Self::EdgeGatewayCertificateRenewalStatusV1
            | Self::WorkloadDeploymentHealthV1
            | Self::EdgeGatewayCertificateExpiryStatusV1 => NOTIFICATION_ALERT_POLICY_SCHEMA,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationAlertPolicyTarget {
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
    Node {
        node_id: NodeId,
    },
}

impl NotificationAlertPolicyTarget {
    pub const fn scope(self) -> NotificationScope {
        match self {
            Self::Environment {
                project_id,
                environment_id,
            } => NotificationScope::Environment {
                project_id,
                environment_id,
            },
            Self::Node { node_id } => NotificationScope::Node { node_id },
        }
    }

    pub const fn project_id(self) -> Option<ProjectId> {
        match self {
            Self::Environment { project_id, .. } => Some(project_id),
            Self::Node { .. } => None,
        }
    }

    pub const fn environment_id(self) -> Option<EnvironmentId> {
        match self {
            Self::Environment { environment_id, .. } => Some(environment_id),
            Self::Node { .. } => None,
        }
    }

    pub const fn node_id(self) -> Option<NodeId> {
        match self {
            Self::Node { node_id } => Some(node_id),
            Self::Environment { .. } => None,
        }
    }

    fn validate(self) -> Result<(), String> {
        if self.project_id().is_some_and(|id| id.as_uuid().is_nil())
            || self
                .environment_id()
                .is_some_and(|id| id.as_uuid().is_nil())
            || self.node_id().is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("notification alert policy target identifiers must not be nil".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationAlertPolicySpec {
    pub source: NotificationAlertSource,
    pub target: NotificationAlertPolicyTarget,
    pub notify_on_recovery: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentPolicySpecRecord {
    source: NotificationAlertSource,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    notify_on_recovery: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodePolicySpecRecord {
    source: NotificationAlertSource,
    node_id: NodeId,
    notify_on_recovery: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PolicySpecRecord {
    Environment(EnvironmentPolicySpecRecord),
    Node(NodePolicySpecRecord),
}

impl Serialize for NotificationAlertPolicySpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.target {
            NotificationAlertPolicyTarget::Environment {
                project_id,
                environment_id,
            } => EnvironmentPolicySpecRecord {
                source: self.source,
                project_id,
                environment_id,
                notify_on_recovery: self.notify_on_recovery,
            }
            .serialize(serializer),
            NotificationAlertPolicyTarget::Node { node_id } => NodePolicySpecRecord {
                source: self.source,
                node_id,
                notify_on_recovery: self.notify_on_recovery,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for NotificationAlertPolicySpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let spec = match PolicySpecRecord::deserialize(deserializer)? {
            PolicySpecRecord::Environment(record) => Self {
                source: record.source,
                target: NotificationAlertPolicyTarget::Environment {
                    project_id: record.project_id,
                    environment_id: record.environment_id,
                },
                notify_on_recovery: record.notify_on_recovery,
            },
            PolicySpecRecord::Node(record) => Self {
                source: record.source,
                target: NotificationAlertPolicyTarget::Node {
                    node_id: record.node_id,
                },
                notify_on_recovery: record.notify_on_recovery,
            },
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

impl NotificationAlertPolicySpec {
    pub fn validate(self) -> Result<(), String> {
        self.target.validate()?;
        match (self.source, self.target) {
            (
                NotificationAlertSource::FleetNodeAvailabilityStatusV1,
                NotificationAlertPolicyTarget::Node { .. },
            )
            | (
                NotificationAlertSource::EdgeDomainClaimStatusV1
                | NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1
                | NotificationAlertSource::WorkloadDeploymentHealthV1
                | NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1,
                NotificationAlertPolicyTarget::Environment { .. },
            ) => {}
            _ => {
                return Err(
                    "notification alert policy source and target kind are incompatible".into(),
                )
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAlertPolicyDefinition {
    spec: NotificationAlertPolicySpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl NotificationAlertPolicyDefinition {
    pub fn from_spec(spec: NotificationAlertPolicySpec) -> Result<Self, String> {
        spec.validate()?;
        let document = definition_document(spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES {
            return Err("notification alert policy ACL exceeds its byte limit".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated notification alert policy ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("notification alert policy ACL is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES {
            return Err("notification alert policy ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("notification alert policy ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("notification alert policy ACL is invalid: {error}"))?;
        let definition = Self::from_spec(parse_definition(&document)?)?;
        if definition.canonical_acl != normalized {
            return Err("notification alert policy ACL is not canonical".into());
        }
        Ok(definition)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let definition = Self::parse_acl(source)?;
        if definition.digest.as_str() != stored_digest {
            return Err("stored notification alert policy ACL and digest do not match".into());
        }
        Ok(definition)
    }

    pub const fn spec(&self) -> NotificationAlertPolicySpec {
        self.spec
    }

    pub const fn schema(&self) -> &'static str {
        self.spec.source.definition_schema()
    }

    pub const fn event_schema_version(&self) -> u32 {
        match self.spec.target {
            NotificationAlertPolicyTarget::Environment { .. } => 1,
            NotificationAlertPolicyTarget::Node { .. } => 2,
        }
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn validate(&self) -> Result<(), String> {
        let reparsed = Self::parse_acl(&self.canonical_acl)?;
        if &reparsed != self {
            return Err("notification alert policy definition is inconsistent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAlertPolicy {
    pub organization_id: OrganizationId,
    pub id: NotificationAlertPolicyId,
    pub recipient_principal_id: PrincipalId,
    pub definition: NotificationAlertPolicyDefinition,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl NotificationAlertPolicy {
    pub fn create(
        organization_id: OrganizationId,
        id: NotificationAlertPolicyId,
        recipient_principal_id: PrincipalId,
        definition: NotificationAlertPolicyDefinition,
        actor_principal_id: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if recipient_principal_id != actor_principal_id {
            return Err("notification alert policy must be owned by its exact recipient".into());
        }
        Self::restore(
            organization_id,
            id,
            recipient_principal_id,
            definition,
            1,
            actor_principal_id,
            created_at,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        id: NotificationAlertPolicyId,
        recipient_principal_id: PrincipalId,
        definition: NotificationAlertPolicyDefinition,
        aggregate_version: u64,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let policy = Self {
            organization_id,
            id,
            recipient_principal_id,
            definition,
            aggregate_version,
            created_by,
            created_at: canonical_timestamp(created_at),
            revoked_at: revoked_at.map(canonical_timestamp),
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn revoke(
        &self,
        expected_version: u64,
        actor_principal_id: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if !self.is_active()
            || expected_version != self.aggregate_version
            || actor_principal_id != self.recipient_principal_id
        {
            return Err("notification alert policy revoke is stale or unauthorized".into());
        }
        Self::restore(
            self.organization_id,
            self.id,
            self.recipient_principal_id,
            self.definition.clone(),
            self.aggregate_version + 1,
            self.created_by,
            self.created_at,
            Some(canonical_timestamp(revoked_at).max(self.created_at)),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        self.definition.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.recipient_principal_id.as_uuid().is_nil()
            || self.created_by != self.recipient_principal_id
            || self.created_at != canonical_timestamp(self.created_at)
            || self.revoked_at != self.revoked_at.map(canonical_timestamp)
        {
            return Err("notification alert policy identity or time is invalid".into());
        }
        match (self.aggregate_version, self.revoked_at) {
            (1, None) | (2, Some(_)) => {}
            _ => return Err("notification alert policy lifecycle is invalid".into()),
        }
        if self
            .revoked_at
            .is_some_and(|revoked_at| revoked_at < self.created_at)
        {
            return Err("notification alert policy lifecycle is invalid".into());
        }
        Ok(())
    }

    pub const fn is_active(&self) -> bool {
        self.aggregate_version == 1 && self.revoked_at.is_none()
    }

    pub fn matches(
        &self,
        source: NotificationAlertSource,
        target: NotificationAlertPolicyTarget,
        occurred_at: DateTime<Utc>,
    ) -> bool {
        let spec = self.definition.spec();
        self.is_active()
            && spec.source == source
            && spec.target == target
            && occurred_at >= self.created_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationAlertPolicyCursor {
    pub created_at: DateTime<Utc>,
    pub policy_id: NotificationAlertPolicyId,
}

impl NotificationAlertPolicyCursor {
    pub fn after(policy: &NotificationAlertPolicy) -> Self {
        Self {
            created_at: policy.created_at,
            policy_id: policy.id,
        }
    }

    pub fn encode(self) -> String {
        encode_descending_cursor(self.created_at, self.policy_id.as_uuid())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let (created_at, policy_id) = parse_descending_cursor(value, "notification alert policy")?;
        Ok(Self {
            created_at,
            policy_id: NotificationAlertPolicyId::from_uuid(policy_id),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAlertPolicyPage {
    pub policies: Vec<NotificationAlertPolicy>,
    pub next_cursor: Option<String>,
}

fn definition_document(spec: NotificationAlertPolicySpec) -> Document {
    let block = BlockBuilder::new(NOTIFICATION_ALERT_POLICY_BLOCK)
        .attr("schema", string(spec.source.definition_schema()))
        .attr("source", string(spec.source.as_str()));
    let block = match spec.target {
        NotificationAlertPolicyTarget::Environment {
            project_id,
            environment_id,
        } => block
            .attr("project_id", string(&project_id.to_string()))
            .attr("environment_id", string(&environment_id.to_string())),
        NotificationAlertPolicyTarget::Node { node_id } => {
            block.attr("node_id", string(&node_id.to_string()))
        }
    };
    Document {
        blocks: vec![block
            .attr("notify_on_recovery", boolean(spec.notify_on_recovery))
            .build()],
    }
}

fn parse_definition(document: &Document) -> Result<NotificationAlertPolicySpec, String> {
    if document.blocks.len() != 1 {
        return Err("notification alert policy must contain one top-level block".into());
    }
    let root = &document.blocks[0];
    let schema = required_string(root, "schema")?;
    let attributes: &[&str] = match schema.as_str() {
        NOTIFICATION_ALERT_POLICY_SCHEMA => &[
            "schema",
            "source",
            "project_id",
            "environment_id",
            "notify_on_recovery",
        ],
        NOTIFICATION_ALERT_POLICY_SCHEMA_V2 => {
            &["schema", "source", "node_id", "notify_on_recovery"]
        }
        _ => return Err("notification alert policy schema is unsupported".into()),
    };
    if root.name != NOTIFICATION_ALERT_POLICY_BLOCK
        || !root.labels.is_empty()
        || !root.blocks.is_empty()
        || root.attributes.len() != attributes.len()
        || root
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
    {
        return Err("notification alert policy root shape is invalid".into());
    }
    let source = NotificationAlertSource::parse(&required_string(root, "source")?)?;
    let target = match schema.as_str() {
        NOTIFICATION_ALERT_POLICY_SCHEMA => NotificationAlertPolicyTarget::Environment {
            project_id: ProjectId::from_uuid(parse_id(root, "project_id")?),
            environment_id: EnvironmentId::from_uuid(parse_id(root, "environment_id")?),
        },
        NOTIFICATION_ALERT_POLICY_SCHEMA_V2 => NotificationAlertPolicyTarget::Node {
            node_id: NodeId::from_uuid(parse_id(root, "node_id")?),
        },
        _ => unreachable!("schema was closed above"),
    };
    let spec = NotificationAlertPolicySpec {
        source,
        target,
        notify_on_recovery: required_bool(root, "notify_on_recovery")?,
    };
    spec.validate()?;
    Ok(spec)
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match block.attributes.get(name) {
        Some(Value::String(value)) => Ok(value.clone()),
        _ => Err(format!("notification alert policy {name} must be a string")),
    }
}

fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    block
        .attributes
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("notification alert policy {name} must be a boolean"))
}

fn parse_id(block: &Block, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(&required_string(block, name)?)
        .ok()
        .filter(|value| !value.is_nil())
        .ok_or_else(|| format!("notification alert policy {name} is invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> NotificationAlertPolicySpec {
        NotificationAlertPolicySpec {
            source: NotificationAlertSource::EdgeDomainClaimStatusV1,
            target: NotificationAlertPolicyTarget::Environment {
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
            },
            notify_on_recovery: true,
        }
    }

    fn node_spec() -> NotificationAlertPolicySpec {
        NotificationAlertPolicySpec {
            source: NotificationAlertSource::FleetNodeAvailabilityStatusV1,
            target: NotificationAlertPolicyTarget::Node {
                node_id: NodeId::new(),
            },
            notify_on_recovery: true,
        }
    }

    #[test]
    fn alert_source_registry_is_closed_and_exact() {
        for (source, name, event_keys) in [
            (
                NotificationAlertSource::EdgeDomainClaimStatusV1,
                "edge.domain-claim-status.v1",
                &["edge.domain-claim.rejected", "edge.domain-claim.verified"][..],
            ),
            (
                NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
                "edge.gateway-certificate-renewal-status.v1",
                &[
                    "edge.gateway-certificate.renewal-failed",
                    "edge.gateway-certificate.renewed",
                ][..],
            ),
            (
                NotificationAlertSource::WorkloadDeploymentHealthV1,
                "workload.deployment-health.v1",
                &["workload.deployment.failed", "workload.deployment.healthy"][..],
            ),
            (
                NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1,
                "edge.gateway-certificate-expiry-status.v1",
                &[
                    "edge.gateway-certificate.expiring",
                    "edge.gateway-certificate.expiry-resolved",
                ][..],
            ),
            (
                NotificationAlertSource::FleetNodeAvailabilityStatusV1,
                "fleet.node-availability-status.v1",
                &["fleet.node.unavailable", "fleet.node.availability-resolved"][..],
            ),
        ] {
            assert_eq!(source.as_str(), name);
            assert_eq!(NotificationAlertSource::parse(name), Ok(source));
            assert_eq!(source.event_keys(), event_keys);

            let definition = NotificationAlertPolicyDefinition::from_spec(
                if source == NotificationAlertSource::FleetNodeAvailabilityStatusV1 {
                    node_spec()
                } else {
                    NotificationAlertPolicySpec { source, ..spec() }
                },
            )
            .expect("registered source definition");
            assert!(definition.canonical_acl().contains(name));
            assert_eq!(
                NotificationAlertPolicyDefinition::parse_acl(definition.canonical_acl()),
                Ok(definition)
            );
        }
        assert!(NotificationAlertSource::parse("edge.gateway-certificate.anything.v1").is_err());
    }

    #[test]
    fn acl_is_canonical_closed_and_round_trips() {
        let definition = NotificationAlertPolicyDefinition::from_spec(spec()).expect("definition");
        assert!(definition
            .canonical_acl()
            .starts_with("notification_alert_policy {\n"));
        assert_eq!(
            NotificationAlertPolicyDefinition::parse_acl(definition.canonical_acl()),
            Ok(definition.clone())
        );
        assert!(NotificationAlertPolicyDefinition::parse_acl(
            &definition
                .canonical_acl()
                .replace("edge.domain-claim-status.v1", "custom.any-event.v1")
        )
        .is_err());
        assert!(NotificationAlertPolicyDefinition::parse_acl(
            &definition
                .canonical_acl()
                .replace("notify_on_recovery = true", "extra = true")
        )
        .is_err());
    }

    #[test]
    fn v2_is_node_only_and_v1_bytes_remain_exact() {
        let environment_fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/c0.3/notification-alert-policy-v1.acl"
        ));
        let environment = NotificationAlertPolicyDefinition::parse_acl(environment_fixture)
            .expect("canonical v1 fixture");
        assert_eq!(environment.schema(), NOTIFICATION_ALERT_POLICY_SCHEMA);
        assert_eq!(environment.event_schema_version(), 1);
        assert!(!environment.canonical_acl().contains("node_id"));

        let node_fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/c0.3/notification-alert-policy-v2.acl"
        ));
        let node = NotificationAlertPolicyDefinition::parse_acl(node_fixture)
            .expect("canonical v2 fixture");
        assert_eq!(node.schema(), NOTIFICATION_ALERT_POLICY_SCHEMA_V2);
        assert_eq!(node.event_schema_version(), 2);
        assert!(node
            .canonical_acl()
            .contains("schema = \"cloud.notification.alert-policy.v2\""));
        assert!(node.canonical_acl().contains("node_id"));
        assert!(!node.canonical_acl().contains("project_id"));
        assert!(!node.canonical_acl().contains("environment_id"));
        assert_eq!(
            NotificationAlertPolicyDefinition::parse_acl(node.canonical_acl()),
            Ok(node.clone())
        );

        assert!(
            NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
                source: NotificationAlertSource::FleetNodeAvailabilityStatusV1,
                target: spec().target,
                notify_on_recovery: true,
            })
            .is_err()
        );
        assert!(
            NotificationAlertPolicyDefinition::parse_acl(&node.canonical_acl().replace(
                NOTIFICATION_ALERT_POLICY_SCHEMA_V2,
                NOTIFICATION_ALERT_POLICY_SCHEMA
            ))
            .is_err()
        );
    }

    #[test]
    fn persisted_specs_keep_v1_shape_and_admit_only_the_closed_v2_shape() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let legacy = serde_json::json!({
            "source": "edge_domain_claim_status_v1",
            "project_id": project_id,
            "environment_id": environment_id,
            "notify_on_recovery": true,
        });
        let decoded: NotificationAlertPolicySpec =
            serde_json::from_value(legacy.clone()).expect("legacy persisted v1 spec");
        assert_eq!(
            decoded.target,
            NotificationAlertPolicyTarget::Environment {
                project_id,
                environment_id,
            }
        );
        assert_eq!(
            serde_json::to_value(decoded).expect("persisted v1 spec"),
            legacy
        );

        let node_id = NodeId::new();
        let node = serde_json::json!({
            "source": "fleet_node_availability_status_v1",
            "node_id": node_id,
            "notify_on_recovery": false,
        });
        let decoded: NotificationAlertPolicySpec =
            serde_json::from_value(node.clone()).expect("persisted v2 spec");
        assert_eq!(
            decoded.target,
            NotificationAlertPolicyTarget::Node { node_id }
        );
        assert_eq!(
            serde_json::to_value(decoded).expect("persisted v2 spec"),
            node
        );

        assert!(
            serde_json::from_value::<NotificationAlertPolicySpec>(serde_json::json!({
                "source": "fleet_node_availability_status_v1",
                "project_id": project_id,
                "environment_id": environment_id,
                "notify_on_recovery": true,
            }))
            .is_err()
        );
    }

    #[test]
    fn lifecycle_is_personal_immutable_and_time_bounded() {
        let organization_id = OrganizationId::new();
        let recipient = PrincipalId::new();
        let created_at = Utc::now();
        let policy = NotificationAlertPolicy::create(
            organization_id,
            NotificationAlertPolicyId::new(),
            recipient,
            NotificationAlertPolicyDefinition::from_spec(spec()).expect("definition"),
            recipient,
            created_at,
        )
        .expect("policy");
        assert!(policy.is_active());
        assert!(NotificationAlertPolicy::create(
            organization_id,
            NotificationAlertPolicyId::new(),
            recipient,
            policy.definition.clone(),
            PrincipalId::new(),
            created_at,
        )
        .is_err());
        let revoked = policy
            .revoke(1, recipient, created_at - chrono::Duration::seconds(1))
            .expect("revoke");
        assert_eq!(revoked.aggregate_version, 2);
        assert_eq!(revoked.revoked_at, Some(policy.created_at));
        assert!(!revoked.is_active());
        assert!(revoked.revoke(2, recipient, created_at).is_err());
    }

    #[test]
    fn source_scope_and_creation_time_are_exact() {
        let definition = NotificationAlertPolicyDefinition::from_spec(spec()).expect("definition");
        let spec = definition.spec();
        let created_at = Utc::now();
        let recipient = PrincipalId::new();
        let policy = NotificationAlertPolicy::create(
            OrganizationId::new(),
            NotificationAlertPolicyId::new(),
            recipient,
            definition,
            recipient,
            created_at,
        )
        .expect("policy");
        assert!(policy.matches(spec.source, spec.target, created_at));
        assert!(!policy.matches(
            spec.source,
            spec.target,
            created_at - chrono::Duration::microseconds(1)
        ));
        assert!(!policy.matches(
            spec.source,
            NotificationAlertPolicyTarget::Environment {
                project_id: ProjectId::new(),
                environment_id: spec.target.environment_id().expect("environment target"),
            },
            created_at
        ));
    }
}
