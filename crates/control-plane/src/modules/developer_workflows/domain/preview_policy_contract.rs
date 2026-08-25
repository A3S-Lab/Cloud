use super::{
    GitBranch, GithubInstallationRef, PreviewForkPolicy, PreviewQuota, PullRequestPreviewPolicy,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, Sha256Digest, SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use a3s_acl::builder::{boolean, integer, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use uuid::Uuid;

pub const PULL_REQUEST_PREVIEW_POLICY_SCHEMA: &str = "a3s.cloud.pull-request-preview-policy.v1";
pub const PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES: usize = 16 * 1024;

const POLICY_BLOCK: &str = "pull_request_preview_policy";
const POLICY_LABEL: &str = "github";
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const POLICY_ATTRIBUTES: [&str; 12] = [
    "allow_protected_secrets_for_trusted_sources",
    "base_branch",
    "base_repository",
    "fork_policy",
    "installation_id",
    "lifetime_seconds",
    "maximum_active_previews",
    "organization_id",
    "owner_principal_id",
    "project_id",
    "schema",
    "source_subscription_id",
];
const QUOTA_ATTRIBUTES: [&str; 4] = [
    "cpu_millis",
    "ephemeral_storage_bytes",
    "maximum_workloads",
    "memory_bytes",
];

/// Canonical, reviewable configuration for one pull-request Preview policy.
///
/// The contract contains references and policy only. It carries no webhook
/// credential, payload, source checkout, Environment, deployment, or cleanup
/// mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestPreviewPolicyContract {
    policy: PullRequestPreviewPolicy,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl PullRequestPreviewPolicyContract {
    pub fn from_policy(policy: PullRequestPreviewPolicy) -> Result<Self, String> {
        policy.validate()?;
        let document = policy_document(&policy)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES {
            return Err("pull-request Preview policy ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated pull-request Preview policy ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("pull-request Preview policy ACL is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            policy,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES {
            return Err("pull-request Preview policy ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("pull-request Preview policy ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("pull-request Preview policy ACL is invalid: {error}"))?;
        let value = Self::from_policy(parse_policy(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("pull-request Preview policy ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored pull-request Preview policy ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("pull-request Preview policy drifted from its canonical ACL".into());
        }
        Ok(())
    }

    pub const fn policy(&self) -> &PullRequestPreviewPolicy {
        &self.policy
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub const fn schema(&self) -> &'static str {
        PULL_REQUEST_PREVIEW_POLICY_SCHEMA
    }
}

fn policy_document(policy: &PullRequestPreviewPolicy) -> Result<Document, String> {
    Ok(Document {
        blocks: vec![BlockBuilder::new(POLICY_BLOCK)
            .label(POLICY_LABEL)
            .attr(
                "allow_protected_secrets_for_trusted_sources",
                boolean(policy.allow_protected_secrets_for_trusted_sources),
            )
            .attr("base_branch", string(policy.base_branch.as_str()))
            .attr(
                "base_repository",
                string(policy.base_repository.canonical_url()),
            )
            .attr("fork_policy", string(policy.fork_policy.as_str()))
            .attr(
                "installation_id",
                acl_integer("installation_id", policy.installation_id.as_u64())?,
            )
            .attr(
                "lifetime_seconds",
                acl_integer("lifetime_seconds", u64::from(policy.lifetime_seconds))?,
            )
            .attr(
                "maximum_active_previews",
                acl_integer(
                    "maximum_active_previews",
                    u64::from(policy.maximum_active_previews),
                )?,
            )
            .attr(
                "organization_id",
                string(&policy.organization_id.to_string()),
            )
            .attr(
                "owner_principal_id",
                string(&policy.owner_principal_id.to_string()),
            )
            .attr("project_id", string(&policy.project_id.to_string()))
            .attr("schema", string(PULL_REQUEST_PREVIEW_POLICY_SCHEMA))
            .attr(
                "source_subscription_id",
                string(&policy.source_subscription_id.to_string()),
            )
            .nested_block(
                BlockBuilder::new("quota")
                    .attr(
                        "cpu_millis",
                        acl_integer("cpu_millis", policy.quota.cpu_millis)?,
                    )
                    .attr(
                        "ephemeral_storage_bytes",
                        acl_integer(
                            "ephemeral_storage_bytes",
                            policy.quota.ephemeral_storage_bytes,
                        )?,
                    )
                    .attr(
                        "maximum_workloads",
                        acl_integer(
                            "maximum_workloads",
                            u64::from(policy.quota.maximum_workloads),
                        )?,
                    )
                    .attr(
                        "memory_bytes",
                        acl_integer("memory_bytes", policy.quota.memory_bytes)?,
                    )
                    .build(),
            )
            .build()],
    })
}

fn parse_policy(document: &Document) -> Result<PullRequestPreviewPolicy, String> {
    if document.blocks.len() != 1 {
        return Err("pull-request Preview policy must contain exactly one block".into());
    }
    let block = &document.blocks[0];
    strict_block(
        block,
        POLICY_BLOCK,
        &[POLICY_LABEL],
        &POLICY_ATTRIBUTES,
        &["quota"],
    )?;
    require_exact_string(block, "schema", PULL_REQUEST_PREVIEW_POLICY_SCHEMA)?;
    let quota = required_nested_block(block, "quota")?;
    strict_block(quota, "quota", &[], &QUOTA_ATTRIBUTES, &[])?;
    Ok(PullRequestPreviewPolicy {
        organization_id: OrganizationId::from_uuid(required_uuid(block, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(block, "project_id")?),
        source_subscription_id: SourceSubscriptionId::from_uuid(required_uuid(
            block,
            "source_subscription_id",
        )?),
        owner_principal_id: PrincipalId::from_uuid(required_uuid(block, "owner_principal_id")?),
        installation_id: GithubInstallationRef::parse(required_u64(block, "installation_id")?)?,
        base_repository: GitRepository::parse(
            GitProvider::Github,
            &required_string(block, "base_repository")?,
        )?,
        base_branch: GitBranch::parse(required_string(block, "base_branch")?)?,
        lifetime_seconds: required_u32(block, "lifetime_seconds")?,
        maximum_active_previews: required_u16(block, "maximum_active_previews")?,
        fork_policy: PreviewForkPolicy::parse(&required_string(block, "fork_policy")?)?,
        allow_protected_secrets_for_trusted_sources: required_boolean(
            block,
            "allow_protected_secrets_for_trusted_sources",
        )?,
        quota: PreviewQuota {
            maximum_workloads: required_u16(quota, "maximum_workloads")?,
            cpu_millis: required_u64(quota, "cpu_millis")?,
            memory_bytes: required_u64(quota, "memory_bytes")?,
            ephemeral_storage_bytes: required_u64(quota, "ephemeral_storage_bytes")?,
        },
    })
}

fn strict_block(
    block: &Block,
    expected_name: &str,
    expected_labels: &[&str],
    expected_attributes: &[&str],
    expected_nested_names: &[&str],
) -> Result<(), String> {
    if block.name != expected_name
        || block.labels.len() != expected_labels.len()
        || block
            .labels
            .iter()
            .zip(expected_labels)
            .any(|(actual, expected)| actual != expected)
        || block.attributes.len() != expected_attributes.len()
        || block
            .attributes
            .keys()
            .any(|name| !expected_attributes.contains(&name.as_str()))
        || block.blocks.len() != expected_nested_names.len()
        || block
            .blocks
            .iter()
            .any(|nested| !expected_nested_names.contains(&nested.name.as_str()))
    {
        return Err(format!(
            "pull-request Preview policy block {expected_name:?} shape is invalid"
        ));
    }
    Ok(())
}

fn required_nested_block<'a>(block: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = block
        .blocks
        .iter()
        .filter(|candidate| candidate.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("pull-request Preview policy nested block {name:?} is required"))?;
    if matches.next().is_some() {
        return Err(format!(
            "pull-request Preview policy nested block {name:?} must be unique"
        ));
    }
    Ok(value)
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("pull-request Preview policy field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("pull-request Preview policy field {name:?} must be a string"))
}

fn require_exact_string(block: &Block, name: &str, expected: &str) -> Result<(), String> {
    if required_string(block, name)? != expected {
        return Err(format!(
            "pull-request Preview policy field {name:?} must be exactly {expected:?}"
        ));
    }
    Ok(())
}

fn required_boolean(block: &Block, name: &str) -> Result<bool, String> {
    match required_value(block, name)? {
        Value::Bool(value) => Ok(*value),
        _ => Err(format!(
            "pull-request Preview policy field {name:?} must be a boolean"
        )),
    }
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("pull-request Preview policy field {name:?} must be a UUID"))
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("pull-request Preview policy field {name:?} must be an integer"))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value <= 0.0
        || value > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "pull-request Preview policy field {name:?} must be an exactly representable positive integer"
        ));
    }
    Ok(value as u64)
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    u32::try_from(required_u64(block, name)?)
        .map_err(|_| format!("pull-request Preview policy field {name:?} exceeds u32"))
}

fn required_u16(block: &Block, name: &str) -> Result<u16, String> {
    u16::try_from(required_u64(block, name)?)
        .map_err(|_| format!("pull-request Preview policy field {name:?} exceeds u16"))
}

fn acl_integer(name: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "pull-request Preview policy field {name:?} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}
