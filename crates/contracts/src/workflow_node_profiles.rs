use crate::{AppPlatformCapabilityCategory, AppPlatformParityManifest};
use a3s_acl::{canonical_bytes, canonical_digest, parse_acl, Block, Document, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_NODE_PROFILES_SCHEMA: &str = "a3s.cloud.app-platform.workflow-node-profiles.v1";
pub const WORKFLOW_NODE_PROFILES_REVISION: &str = "1.0.0";

const PROFILE_SET_ID: &str = "application-platform-core-2026-08-13";
const PROFILE_SET_BLOCK: &str = "workflow_node_profiles";
const PROFILE_BLOCK: &str = "node";
const MAX_PROFILE_SET_BYTES: usize = 32 * 1024;
const ROOT_ATTRIBUTES: [&str; 3] = ["parity_manifest_digest", "revision", "schema"];
const PROFILE_REQUIRED_ATTRIBUTES: [&str; 2] = ["execution_class", "semantic_profiles"];
const PROFILE_OPTIONAL_ATTRIBUTES: [&str; 1] = ["kind"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkflowNodeExecutionClass {
    WorkflowLocal,
    CompositeRegion,
    OwningApplicationPort,
    InvocationOnly,
}

impl WorkflowNodeExecutionClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowLocal => "workflow_local",
            Self::CompositeRegion => "composite_region",
            Self::OwningApplicationPort => "owning_application_port",
            Self::InvocationOnly => "invocation_only",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "workflow_local" => Ok(Self::WorkflowLocal),
            "composite_region" => Ok(Self::CompositeRegion),
            "owning_application_port" => Ok(Self::OwningApplicationPort),
            "invocation_only" => Ok(Self::InvocationOnly),
            _ => Err(format!("unknown Workflow node execution class {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkflowNodeKind {
    Input,
    Output,
    Transform,
    Branch,
    HumanDecision,
    Execution,
    Agent,
    Mcp,
    Model,
    Tool,
    Service,
    Memory,
    Subworkflow,
}

impl WorkflowNodeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::Transform => "transform",
            Self::Branch => "branch",
            Self::HumanDecision => "human_decision",
            Self::Execution => "execution",
            Self::Agent => "agent",
            Self::Mcp => "mcp",
            Self::Model => "model",
            Self::Tool => "tool",
            Self::Service => "service",
            Self::Memory => "memory",
            Self::Subworkflow => "subworkflow",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "input" => Ok(Self::Input),
            "output" => Ok(Self::Output),
            "transform" => Ok(Self::Transform),
            "branch" => Ok(Self::Branch),
            "human_decision" => Ok(Self::HumanDecision),
            "execution" => Ok(Self::Execution),
            "agent" => Ok(Self::Agent),
            "mcp" => Ok(Self::Mcp),
            "model" => Ok(Self::Model),
            "tool" => Ok(Self::Tool),
            "service" => Ok(Self::Service),
            "memory" => Ok(Self::Memory),
            "subworkflow" => Ok(Self::Subworkflow),
            _ => Err(format!("unknown Workflow node kind {value:?}")),
        }
    }

    const fn is_workflow_local(self) -> bool {
        matches!(
            self,
            Self::Input | Self::Output | Self::Transform | Self::Branch | Self::HumanDecision
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowNodeProfile {
    capability_id: String,
    kind: Option<WorkflowNodeKind>,
    execution_class: WorkflowNodeExecutionClass,
    semantic_profiles: Vec<String>,
}

impl WorkflowNodeProfile {
    pub fn capability_id(&self) -> &str {
        &self.capability_id
    }

    pub const fn kind(&self) -> Option<WorkflowNodeKind> {
        self.kind
    }

    pub const fn execution_class(&self) -> WorkflowNodeExecutionClass {
        self.execution_class
    }

    pub fn semantic_profiles(&self) -> &[String] {
        &self.semantic_profiles
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowNodeProfiles {
    parity_manifest_digest: String,
    profiles: Vec<WorkflowNodeProfile>,
    canonical_acl: String,
    digest: String,
}

impl WorkflowNodeProfiles {
    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > MAX_PROFILE_SET_BYTES {
            return Err("Workflow node profile ACL size is invalid".into());
        }
        if source.contains('\r') && !source.contains("\r\n") {
            return Err("Workflow node profiles contain a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Workflow node profile ACL is invalid: {error}"))?;
        let canonical = canonical_bytes(&document)
            .map_err(|error| format!("Workflow node profiles are not canonicalizable: {error}"))?;
        if normalized.as_bytes() != canonical {
            return Err("Workflow node profile ACL is not canonical".into());
        }
        let root = exact_root_block(&document)?;
        if required_string(root, "schema")? != WORKFLOW_NODE_PROFILES_SCHEMA
            || required_string(root, "revision")? != WORKFLOW_NODE_PROFILES_REVISION
        {
            return Err("Workflow node profile schema or revision is unsupported".into());
        }
        let parity_manifest_digest = required_string(root, "parity_manifest_digest")?;
        validate_digest(&parity_manifest_digest)?;
        let profiles = root
            .blocks
            .iter()
            .map(parse_profile)
            .collect::<Result<Vec<_>, _>>()?;
        require_strict_id_order(profiles.iter().map(WorkflowNodeProfile::capability_id))?;
        validate_profile_semantics(&profiles)?;
        let digest = canonical_digest(&document)
            .map_err(|error| format!("Workflow node profile digest failed: {error}"))?;
        let canonical_acl = String::from_utf8(canonical)
            .map_err(|_| "Workflow node profile ACL is not UTF-8".to_owned())?;
        Ok(Self {
            parity_manifest_digest,
            profiles,
            canonical_acl,
            digest,
        })
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let profiles = Self::parse_acl(source)?;
        if profiles.digest != stored_digest {
            return Err("stored Workflow node profile ACL and digest do not match".into());
        }
        Ok(profiles)
    }

    pub fn validate_manifest(&self, manifest: &AppPlatformParityManifest) -> Result<(), String> {
        if self.parity_manifest_digest != manifest.digest() {
            return Err(format!(
                "Workflow node profiles reference parity manifest {} but the current manifest is {}",
                self.parity_manifest_digest,
                manifest.digest()
            ));
        }
        let capabilities = manifest
            .capabilities()
            .iter()
            .filter(|capability| capability.category() == AppPlatformCapabilityCategory::Node)
            .map(|capability| (capability.id(), capability))
            .collect::<BTreeMap<_, _>>();
        let profile_ids = self
            .profiles
            .iter()
            .map(|profile| profile.capability_id.as_str())
            .collect::<BTreeSet<_>>();
        if capabilities.keys().copied().collect::<BTreeSet<_>>() != profile_ids {
            return Err("Workflow node profiles do not exactly cover the parity inventory".into());
        }
        for profile in &self.profiles {
            let capability = capabilities
                .get(profile.capability_id.as_str())
                .ok_or_else(|| "Workflow node profile lost its parity capability".to_owned())?;
            validate_owner_boundary(capability.owner(), profile)?;
        }
        Ok(())
    }

    pub fn parity_manifest_digest(&self) -> &str {
        &self.parity_manifest_digest
    }

    pub fn profiles(&self) -> &[WorkflowNodeProfile] {
        &self.profiles
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

fn exact_root_block(document: &Document) -> Result<&Block, String> {
    if document.blocks.len() != 1 {
        return Err("Workflow node profiles require exactly one root block".into());
    }
    let root = &document.blocks[0];
    if root.name != PROFILE_SET_BLOCK || root.labels != [PROFILE_SET_ID] {
        return Err("Workflow node profile root identity is invalid".into());
    }
    exact_attributes(root, &ROOT_ATTRIBUTES, &[])?;
    if root.blocks.iter().any(|block| block.name != PROFILE_BLOCK) {
        return Err("Workflow node profiles contain an unknown block".into());
    }
    Ok(root)
}

fn parse_profile(block: &Block) -> Result<WorkflowNodeProfile, String> {
    if block.name != PROFILE_BLOCK || block.labels.len() != 1 || !block.blocks.is_empty() {
        return Err("Workflow node profile block shape is invalid".into());
    }
    exact_attributes(
        block,
        &PROFILE_REQUIRED_ATTRIBUTES,
        &PROFILE_OPTIONAL_ATTRIBUTES,
    )?;
    let capability_id = block.labels[0].clone();
    if !capability_id.starts_with("node.") {
        return Err(format!(
            "Workflow node profile capability {capability_id:?} is invalid"
        ));
    }
    let kind = block
        .attributes
        .get("kind")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "Workflow node profile kind must be a string".to_owned())
                .and_then(WorkflowNodeKind::parse)
        })
        .transpose()?;
    let execution_class =
        WorkflowNodeExecutionClass::parse(&required_string(block, "execution_class")?)?;
    let semantic_profiles = required_string_list(block, "semantic_profiles")?;
    if semantic_profiles.is_empty()
        || semantic_profiles.len() > 8
        || !semantic_profiles.windows(2).all(|pair| pair[0] < pair[1])
    {
        return Err("Workflow node semantic profiles must be a sorted unique list".into());
    }
    for profile in &semantic_profiles {
        validate_dotted_identifier("Workflow node semantic profile", profile)?;
    }
    Ok(WorkflowNodeProfile {
        capability_id,
        kind,
        execution_class,
        semantic_profiles,
    })
}

fn validate_profile_semantics(profiles: &[WorkflowNodeProfile]) -> Result<(), String> {
    if profiles.is_empty() || profiles.len() > 128 {
        return Err("Workflow node profile bounds are invalid".into());
    }
    let mut semantic_profiles = BTreeSet::new();
    for profile in profiles {
        for semantic_profile in &profile.semantic_profiles {
            if !semantic_profiles.insert(semantic_profile.as_str()) {
                return Err(format!(
                    "Workflow semantic profile {semantic_profile:?} is ambiguous"
                ));
            }
        }
        match (profile.execution_class, profile.kind) {
            (WorkflowNodeExecutionClass::InvocationOnly, None) => {}
            (WorkflowNodeExecutionClass::WorkflowLocal, Some(kind)) if kind.is_workflow_local() => {
            }
            (WorkflowNodeExecutionClass::CompositeRegion, Some(WorkflowNodeKind::Subworkflow)) => {}
            (WorkflowNodeExecutionClass::OwningApplicationPort, Some(_)) => {}
            _ => {
                return Err(format!(
                    "Workflow node profile {:?} has an invalid execution-class/kind pair",
                    profile.capability_id
                ))
            }
        }
    }
    Ok(())
}

fn validate_owner_boundary(owner: &str, profile: &WorkflowNodeProfile) -> Result<(), String> {
    let matches = match (profile.execution_class, profile.kind) {
        (WorkflowNodeExecutionClass::InvocationOnly, None) => owner == "automations",
        (WorkflowNodeExecutionClass::WorkflowLocal, Some(_))
        | (WorkflowNodeExecutionClass::CompositeRegion, Some(WorkflowNodeKind::Subworkflow)) => {
            owner == "workflow"
        }
        (WorkflowNodeExecutionClass::OwningApplicationPort, Some(kind)) => match kind {
            WorkflowNodeKind::Agent => owner == "agents",
            WorkflowNodeKind::Execution => owner == "executions",
            WorkflowNodeKind::Mcp => owner == "assets",
            WorkflowNodeKind::Model => owner == "inference",
            WorkflowNodeKind::Tool | WorkflowNodeKind::Memory => owner == "use",
            WorkflowNodeKind::Service => {
                matches!(owner, "applications" | "connectors" | "knowledge")
            }
            WorkflowNodeKind::Output => owner == "applications",
            WorkflowNodeKind::Input
            | WorkflowNodeKind::Transform
            | WorkflowNodeKind::Branch
            | WorkflowNodeKind::HumanDecision
            | WorkflowNodeKind::Subworkflow => false,
        },
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(format!(
            "Workflow node profile {:?} conflicts with owner {owner:?}",
            profile.capability_id
        ))
    }
}

fn exact_attributes(block: &Block, required: &[&str], optional: &[&str]) -> Result<(), String> {
    if required
        .iter()
        .any(|attribute| !block.attributes.contains_key(*attribute))
        || block.attributes.keys().any(|attribute| {
            !required.contains(&attribute.as_str()) && !optional.contains(&attribute.as_str())
        })
    {
        return Err("Workflow node profile contains missing or unknown fields".into());
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Workflow node profile field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Workflow node profile field {name:?} must be a string"))
}

fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "Workflow node profile field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                format!("Workflow node profile field {name:?} must be a string list")
            })
        })
        .collect()
}

fn require_strict_id_order<'a>(values: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let values = values.collect::<Vec<_>>();
    if values.windows(2).all(|pair| pair[0] < pair[1]) {
        Ok(())
    } else {
        Err("Workflow node profile blocks must use strict identifier order".into())
    }
}

fn validate_dotted_identifier(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value.split('.').any(|segment| {
            segment.is_empty()
                || segment.starts_with('-')
                || segment.ends_with('-')
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(format!("{label} must use portable dotted lowercase syntax"));
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), String> {
    let Some(hexadecimal) = value.strip_prefix("sha256:") else {
        return Err("Workflow node profile digest must use canonical sha256 syntax".into());
    };
    if hexadecimal.len() != 64
        || !hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Workflow node profile digest must use canonical sha256 syntax".into());
    }
    Ok(())
}
