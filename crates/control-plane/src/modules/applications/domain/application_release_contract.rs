use super::{ApplicationWorkflowBinding, ApplicationWorkflowRevisionEvidence};
use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_acl::builder::{list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const APPLICATION_RELEASE_CONTRACT_SCHEMA: &str = "cloud.application.release.v1";
pub const APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES: usize = 64 * 1024;
const APPLICATION_RELEASE_BLOCK: &str = "application_release";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationExperience {
    Chatbot,
    TextGenerator,
    ClassicAgent,
    NewAgent,
    Chatflow,
    Workflow,
}

impl ApplicationExperience {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chatbot => "chatbot",
            Self::TextGenerator => "text_generator",
            Self::ClassicAgent => "classic_agent",
            Self::NewAgent => "new_agent",
            Self::Chatflow => "chatflow",
            Self::Workflow => "workflow",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "chatbot" => Ok(Self::Chatbot),
            "text_generator" => Ok(Self::TextGenerator),
            "classic_agent" => Ok(Self::ClassicAgent),
            "new_agent" => Ok(Self::NewAgent),
            "chatflow" => Ok(Self::Chatflow),
            "workflow" => Ok(Self::Workflow),
            _ => Err("unsupported Application experience".into()),
        }
    }

    pub const fn interaction_mode(self) -> ApplicationInteractionMode {
        match self {
            Self::Chatbot | Self::ClassicAgent | Self::NewAgent | Self::Chatflow => {
                ApplicationInteractionMode::Conversation
            }
            Self::TextGenerator | Self::Workflow => ApplicationInteractionMode::Invocation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationInteractionMode {
    Conversation,
    Invocation,
}

impl ApplicationInteractionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Invocation => "invocation",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "conversation" => Ok(Self::Conversation),
            "invocation" => Ok(Self::Invocation),
            _ => Err("unsupported Application interaction mode".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationResponseMode {
    Asynchronous,
    Blocking,
    Streaming,
}

impl ApplicationResponseMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asynchronous => "asynchronous",
            Self::Blocking => "blocking",
            Self::Streaming => "streaming",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "asynchronous" => Ok(Self::Asynchronous),
            "blocking" => Ok(Self::Blocking),
            "streaming" => Ok(Self::Streaming),
            _ => Err("unsupported Application response mode".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationDeliveryPolicy {
    pub interaction_mode: ApplicationInteractionMode,
    pub response_modes: Vec<ApplicationResponseMode>,
}

impl ApplicationDeliveryPolicy {
    fn normalize(mut self, experience: ApplicationExperience) -> Result<Self, String> {
        if self.interaction_mode != experience.interaction_mode() {
            return Err("Application delivery mode does not match its experience".into());
        }
        if self.response_modes.is_empty() || self.response_modes.len() > 3 {
            return Err(
                "Application delivery must admit between one and three response modes".into(),
            );
        }
        let unique = self.response_modes.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != self.response_modes.len() {
            return Err("Application delivery contains duplicate response modes".into());
        }
        self.response_modes = unique.into_iter().collect();
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationAudience {
    ProjectMembers,
    AuthenticatedEndUsers,
    Anonymous,
}

impl ApplicationAudience {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectMembers => "project_members",
            Self::AuthenticatedEndUsers => "authenticated_end_users",
            Self::Anonymous => "anonymous",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "project_members" => Ok(Self::ProjectMembers),
            "authenticated_end_users" => Ok(Self::AuthenticatedEndUsers),
            "anonymous" => Ok(Self::Anonymous),
            _ => Err("unsupported Application audience".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationReleaseContractSpec {
    pub experience: ApplicationExperience,
    pub audience: ApplicationAudience,
    pub delivery: ApplicationDeliveryPolicy,
    pub workflow: ApplicationWorkflowBinding,
    pub presentation_digest: Sha256Digest,
}

/// Canonical Applications-owned publication contract.
///
/// It retains only immutable product policy and exact Workflow identity. The
/// graph, Flow history, provider state, credentials, sessions, and routes stay
/// with their existing owners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationReleaseContract {
    spec: ApplicationReleaseContractSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl ApplicationReleaseContract {
    pub fn from_spec(mut spec: ApplicationReleaseContractSpec) -> Result<Self, String> {
        spec.workflow.validate()?;
        spec.delivery = spec.delivery.normalize(spec.experience)?;
        if Sha256Digest::parse(spec.presentation_digest.as_str())? != spec.presentation_digest {
            return Err("Application presentation digest is not canonical".into());
        }
        let document = contract_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES {
            return Err("Application release ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Application release ACL is invalid: {error}"))?;
        let digest =
            Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
                format!("Application release is not canonicalizable: {error}")
            })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES {
            return Err("Application release ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Application release ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Application release ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_contract(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("Application release ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored Application release ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(self.canonical_acl(), self.digest.as_str())?;
        if restored != *self {
            return Err("Application release contract drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub fn validate_workflow_evidence(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        evidence: &ApplicationWorkflowRevisionEvidence,
    ) -> Result<(), String> {
        self.spec
            .workflow
            .validate_evidence(organization_id, project_id, evidence)
    }

    pub const fn spec(&self) -> &ApplicationReleaseContractSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &ApplicationReleaseContractSpec) -> Document {
    let workflow = &spec.workflow;
    let workflow_block = BlockBuilder::new("workflow")
        .attr(
            "workflow_contract_digest",
            string(workflow.workflow_contract_digest.as_str()),
        )
        .attr(
            "workflow_definition_id",
            string(&workflow.workflow_definition_id.to_string()),
        )
        .attr(
            "workflow_payload_set_digest",
            string(workflow.workflow_payload_set_digest.as_str()),
        )
        .attr(
            "workflow_revision_id",
            string(&workflow.workflow_revision_id.to_string()),
        )
        .attr(
            "workflow_semantic_contract_set_digest",
            string(workflow.workflow_semantic_contract_set_digest.as_str()),
        )
        .attr(
            "input_schema_digest",
            string(workflow.input_schema_digest.as_str()),
        )
        .attr(
            "output_schema_digest",
            string(workflow.output_schema_digest.as_str()),
        )
        .build();
    let delivery_block = BlockBuilder::new("delivery")
        .attr(
            "interaction_mode",
            string(spec.delivery.interaction_mode.as_str()),
        )
        .attr(
            "response_modes",
            list(
                spec.delivery
                    .response_modes
                    .iter()
                    .map(|mode| string(mode.as_str()))
                    .collect(),
            ),
        )
        .build();
    Document {
        blocks: vec![BlockBuilder::new(APPLICATION_RELEASE_BLOCK)
            .attr("audience", string(spec.audience.as_str()))
            .attr("experience", string(spec.experience.as_str()))
            .attr(
                "presentation_digest",
                string(spec.presentation_digest.as_str()),
            )
            .attr("schema", string(APPLICATION_RELEASE_CONTRACT_SCHEMA))
            .nested_block(delivery_block)
            .nested_block(workflow_block)
            .build()],
    }
}

fn parse_contract(document: &Document) -> Result<ApplicationReleaseContractSpec, String> {
    if document.blocks.len() != 1 {
        return Err("Application release must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        APPLICATION_RELEASE_BLOCK,
        &["audience", "experience", "presentation_digest", "schema"],
        &["delivery", "workflow"],
    )?;
    if required_string(root, "schema")? != APPLICATION_RELEASE_CONTRACT_SCHEMA {
        return Err("Application release schema is unsupported".into());
    }
    let experience = ApplicationExperience::parse(&required_string(root, "experience")?)?;
    let delivery = exact_child(root, "delivery")?;
    exact_shape(
        delivery,
        "delivery",
        &["interaction_mode", "response_modes"],
        &[],
    )?;
    let workflow = exact_child(root, "workflow")?;
    exact_shape(
        workflow,
        "workflow",
        &[
            "input_schema_digest",
            "output_schema_digest",
            "workflow_contract_digest",
            "workflow_definition_id",
            "workflow_payload_set_digest",
            "workflow_revision_id",
            "workflow_semantic_contract_set_digest",
        ],
        &[],
    )?;
    Ok(ApplicationReleaseContractSpec {
        experience,
        audience: ApplicationAudience::parse(&required_string(root, "audience")?)?,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::parse(&required_string(
                delivery,
                "interaction_mode",
            )?)?,
            response_modes: required_string_list(delivery, "response_modes")?
                .into_iter()
                .map(|value| ApplicationResponseMode::parse(&value))
                .collect::<Result<Vec<_>, _>>()?,
        },
        workflow: ApplicationWorkflowBinding {
            workflow_definition_id: WorkflowDefinitionId::from_uuid(required_uuid(
                workflow,
                "workflow_definition_id",
            )?),
            workflow_revision_id: WorkflowRevisionId::from_uuid(required_uuid(
                workflow,
                "workflow_revision_id",
            )?),
            workflow_contract_digest: required_digest(workflow, "workflow_contract_digest")?,
            workflow_payload_set_digest: required_digest(workflow, "workflow_payload_set_digest")?,
            workflow_semantic_contract_set_digest: required_digest(
                workflow,
                "workflow_semantic_contract_set_digest",
            )?,
            input_schema_digest: required_digest(workflow, "input_schema_digest")?,
            output_schema_digest: required_digest(workflow, "output_schema_digest")?,
        },
        presentation_digest: required_digest(root, "presentation_digest")?,
    })
}

fn exact_shape(
    block: &Block,
    name: &str,
    attributes: &[&str],
    children: &[&str],
) -> Result<(), String> {
    if block.name != name
        || !block.labels.is_empty()
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || block.blocks.len() != children.len()
        || block
            .blocks
            .iter()
            .any(|child| !children.contains(&child.name.as_str()))
    {
        return Err(format!("Application release {name} block shape is invalid"));
    }
    Ok(())
}

fn exact_child<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("Application release {name} block is required"))?;
    if matches.next().is_some() {
        return Err(format!("Application release {name} block must be unique"));
    }
    Ok(value)
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Application release field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match required_value(block, name)? {
        Value::String(value) => Ok(value.clone()),
        _ => Err(format!(
            "Application release field {name:?} must be a string"
        )),
    }
}

fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!("Application release field {name:?} must be a list"));
    };
    values
        .iter()
        .map(|value| match value {
            Value::String(value) => Ok(value.clone()),
            _ => Err(format!(
                "Application release field {name:?} must contain only strings"
            )),
        })
        .collect()
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    let value = Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("Application release field {name:?} must be a UUID"))?;
    if value.is_nil() {
        return Err(format!("Application release field {name:?} cannot be nil"));
    }
    Ok(value)
}

fn required_digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
        .map_err(|_| format!("Application release field {name:?} must be a SHA-256 digest"))
}
