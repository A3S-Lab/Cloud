use super::notification::{encode_descending_cursor, parse_descending_cursor};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, NotificationAlertPolicyId, OrganizationId, PrincipalId,
    ProjectId, Sha256Digest,
};
use a3s_acl::builder::{boolean, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const NOTIFICATION_ALERT_POLICY_SCHEMA: &str = "cloud.notification.alert-policy.v1";
pub const NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES: usize = 16 * 1024;
const NOTIFICATION_ALERT_POLICY_BLOCK: &str = "notification_alert_policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAlertSource {
    EdgeDomainClaimStatusV1,
}

impl NotificationAlertSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EdgeDomainClaimStatusV1 => "edge.domain-claim-status.v1",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "edge.domain-claim-status.v1" => Ok(Self::EdgeDomainClaimStatusV1),
            _ => Err("notification alert source is unsupported".into()),
        }
    }

    pub const fn event_keys(self) -> &'static [&'static str] {
        match self {
            Self::EdgeDomainClaimStatusV1 => {
                &["edge.domain-claim.rejected", "edge.domain-claim.verified"]
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAlertPolicySpec {
    pub source: NotificationAlertSource,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub notify_on_recovery: bool,
}

impl NotificationAlertPolicySpec {
    pub fn validate(self) -> Result<(), String> {
        if self.project_id.as_uuid().is_nil() || self.environment_id.as_uuid().is_nil() {
            return Err("notification alert policy scope identifiers must not be nil".into());
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
        project_id: ProjectId,
        environment_id: EnvironmentId,
        occurred_at: DateTime<Utc>,
    ) -> bool {
        let spec = self.definition.spec();
        self.is_active()
            && spec.source == source
            && spec.project_id == project_id
            && spec.environment_id == environment_id
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
    Document {
        blocks: vec![BlockBuilder::new(NOTIFICATION_ALERT_POLICY_BLOCK)
            .attr("schema", string(NOTIFICATION_ALERT_POLICY_SCHEMA))
            .attr("source", string(spec.source.as_str()))
            .attr("project_id", string(&spec.project_id.to_string()))
            .attr("environment_id", string(&spec.environment_id.to_string()))
            .attr("notify_on_recovery", boolean(spec.notify_on_recovery))
            .build()],
    }
}

fn parse_definition(document: &Document) -> Result<NotificationAlertPolicySpec, String> {
    if document.blocks.len() != 1 {
        return Err("notification alert policy must contain one top-level block".into());
    }
    let root = &document.blocks[0];
    let attributes = [
        "schema",
        "source",
        "project_id",
        "environment_id",
        "notify_on_recovery",
    ];
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
    if required_string(root, "schema")? != NOTIFICATION_ALERT_POLICY_SCHEMA {
        return Err("notification alert policy schema is unsupported".into());
    }
    let spec = NotificationAlertPolicySpec {
        source: NotificationAlertSource::parse(&required_string(root, "source")?)?,
        project_id: ProjectId::from_uuid(parse_id(root, "project_id")?),
        environment_id: EnvironmentId::from_uuid(parse_id(root, "environment_id")?),
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
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            notify_on_recovery: true,
        }
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
        assert!(policy.matches(
            spec.source,
            spec.project_id,
            spec.environment_id,
            created_at
        ));
        assert!(!policy.matches(
            spec.source,
            spec.project_id,
            spec.environment_id,
            created_at - chrono::Duration::microseconds(1)
        ));
        assert!(!policy.matches(
            spec.source,
            ProjectId::new(),
            spec.environment_id,
            created_at
        ));
    }
}
