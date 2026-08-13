use super::validation::{optional_string, required_string};
use super::{CapabilityOwner, CapabilityReference, CapabilityType};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{
    canonical_digest_with_schema, generate_acl, parse_acl, validate_document, AttributeSchema,
    Block, BlockSchema, Cardinality, Document, Schema, ValueSchema,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const WORKFLOW_DEFINITION_SCHEMA: &str = "cloud.workflow.definition.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowContractQuotas {
    pub max_acl_bytes: usize,
    pub max_steps: usize,
    pub max_edges: usize,
}

impl Default for WorkflowContractQuotas {
    fn default() -> Self {
        Self {
            max_acl_bytes: 1024 * 1024,
            max_steps: 512,
            max_edges: 4_096,
        }
    }
}

impl WorkflowContractQuotas {
    fn validate(self) -> Result<Self, String> {
        if self.max_acl_bytes == 0
            || self.max_acl_bytes > 16 * 1024 * 1024
            || self.max_steps < 2
            || self.max_steps > 10_000
            || self.max_edges == 0
            || self.max_edges > 100_000
        {
            return Err("Workflow contract quotas are invalid".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepKind {
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

impl WorkflowStepKind {
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

    pub fn parse(value: &str) -> Result<Self, String> {
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
            _ => Err(format!("unsupported Workflow step kind {value:?}")),
        }
    }

    /// Translate the ten standalone A3S Workflow node names into the Cloud
    /// semantic contract. This is a migration map, not a second wire protocol.
    pub fn from_standalone_kind(value: &str) -> Result<Self, String> {
        match value {
            "start" => Ok(Self::Input),
            "template" => Ok(Self::Transform),
            "llm" => Ok(Self::Model),
            "agent" => Ok(Self::Agent),
            "tool" => Ok(Self::Tool),
            "router" => Ok(Self::Branch),
            "memory" => Ok(Self::Memory),
            "http" => Ok(Self::Service),
            "approval" => Ok(Self::HumanDecision),
            "output" => Ok(Self::Output),
            _ => Err(format!(
                "unsupported standalone A3S Workflow node kind {value:?}"
            )),
        }
    }

    pub const fn is_workflow_local(self) -> bool {
        matches!(
            self,
            Self::Input | Self::Output | Self::Transform | Self::Branch | Self::HumanDecision
        )
    }

    /// Returns the capability types admitted by the existing Workflow graph
    /// contract for this coarse dispatch kind.
    ///
    /// Step descriptors reuse this mapping so descriptor admission cannot
    /// diverge from the graph that current Plan revisions compile.
    pub(super) const fn allowed_capability_types(self) -> &'static [CapabilityType] {
        match self {
            Self::Input | Self::Output | Self::Transform | Self::Branch => &[],
            Self::HumanDecision => &[CapabilityType::FormRelease],
            Self::Execution => &[CapabilityType::ExecutionTemplate],
            Self::Agent => &[CapabilityType::AgentRelease],
            Self::Mcp => &[CapabilityType::McpServiceProfile],
            Self::Model => &[CapabilityType::ModelRevision],
            Self::Tool | Self::Memory => &[CapabilityType::UsePackage],
            Self::Service => &[CapabilityType::ConnectorRevision],
            Self::Subworkflow => &[CapabilityType::WorkflowRevision],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepSpec {
    pub id: String,
    pub label: String,
    pub kind: WorkflowStepKind,
    pub configuration_digest: Sha256Digest,
    pub input_schema_digest: Sha256Digest,
    pub output_schema_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_digest: Option<Sha256Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<CapabilityReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEdgeSpec {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_handle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<WorkflowStepSpec>,
    pub edges: Vec<WorkflowEdgeSpec>,
}

impl WorkflowSpec {
    pub fn validate(&self, quotas: WorkflowContractQuotas) -> Result<(), String> {
        self.topological_order(quotas).map(|_| ())
    }

    pub fn topological_order(&self, quotas: WorkflowContractQuotas) -> Result<Vec<String>, String> {
        super::workflow_graph::validate_workflow(self, quotas.validate()?)
    }
}

/// Canonical closed ACL for one immutable Workflow revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowContract {
    spec: WorkflowSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkflowContract {
    pub fn from_spec(spec: WorkflowSpec) -> Result<Self, String> {
        Self::from_spec_with_quotas(spec, WorkflowContractQuotas::default())
    }

    pub fn from_spec_with_quotas(
        mut spec: WorkflowSpec,
        quotas: WorkflowContractQuotas,
    ) -> Result<Self, String> {
        let quotas = quotas.validate()?;
        spec.steps.sort_by(|left, right| left.id.cmp(&right.id));
        spec.edges.sort_by(|left, right| left.id.cmp(&right.id));
        spec.validate(quotas)?;
        let schema = workflow_schema(quotas)?;
        let document = workflow_document(&spec);
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > quotas.max_acl_bytes {
            return Err("Workflow definition ACL exceeds its quota".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Workflow definition ACL is invalid: {error}"))?;
        validate_schema(&reparsed, &schema)?;
        let digest = Sha256Digest::parse(
            canonical_digest_with_schema(&reparsed, &schema)
                .map_err(|error| format!("Workflow definition is not canonicalizable: {error}"))?,
        )?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        Self::parse_acl_with_quotas(acl, WorkflowContractQuotas::default())
    }

    pub fn parse_acl_with_quotas(
        acl: &str,
        quotas: WorkflowContractQuotas,
    ) -> Result<Self, String> {
        let quotas = quotas.validate()?;
        if acl.is_empty() || acl.len() > quotas.max_acl_bytes {
            return Err("Workflow definition ACL size is invalid".into());
        }
        let document = parse_acl(acl)
            .map_err(|error| format!("Workflow definition ACL is invalid: {error}"))?;
        let schema = workflow_schema(quotas)?;
        validate_schema(&document, &schema)?;
        let root = document
            .blocks
            .first()
            .ok_or_else(|| "Workflow definition block is missing".to_owned())?;
        if required_string(root, "schema")? != WORKFLOW_DEFINITION_SCHEMA {
            return Err("Workflow definition schema is unsupported".into());
        }
        let spec = WorkflowSpec {
            name: required_string(root, "name")?,
            description: required_string(root, "description")?,
            steps: root
                .blocks
                .iter()
                .filter(|block| block.name == "step")
                .map(parse_step)
                .collect::<Result<Vec<_>, _>>()?,
            edges: root
                .blocks
                .iter()
                .filter(|block| block.name == "edge")
                .map(parse_edge)
                .collect::<Result<Vec<_>, _>>()?,
        };
        Self::from_spec_with_quotas(spec, quotas)
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let contract = Self::parse_acl(acl)?;
        if contract.digest.as_str() != stored_digest {
            return Err("stored Workflow definition ACL and digest do not match".into());
        }
        Ok(contract)
    }

    pub const fn spec(&self) -> &WorkflowSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn workflow_schema(quotas: WorkflowContractQuotas) -> Result<Schema, String> {
    let capability = Schema::new()
        .attribute("owner", AttributeSchema::required(ValueSchema::string()))
        .attribute("type", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "resource_id",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute("revision", AttributeSchema::required(ValueSchema::string()))
        .attribute("digest", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "capability",
            AttributeSchema::required(ValueSchema::string()),
        );
    let step = Schema::new()
        .attribute("label", AttributeSchema::required(ValueSchema::string()))
        .attribute("kind", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "configuration_digest",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "input_schema_digest",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "output_schema_digest",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "policy_digest",
            AttributeSchema::optional(ValueSchema::string()),
        )
        .block(
            "capability",
            BlockSchema::new(capability).occurrences(
                Cardinality::new(0, Some(1))
                    .map_err(|error| format!("Workflow capability schema is invalid: {error}"))?,
            ),
        );
    let edge = Schema::new()
        .attribute("source", AttributeSchema::required(ValueSchema::string()))
        .attribute("target", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "source_handle",
            AttributeSchema::optional(ValueSchema::string()),
        );
    let workflow = Schema::new()
        .attribute("schema", AttributeSchema::required(ValueSchema::string()))
        .attribute("name", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "description",
            AttributeSchema::required(ValueSchema::string()),
        )
        .block(
            "step",
            BlockSchema::new(step)
                .occurrences(
                    Cardinality::new(2, Some(quotas.max_steps))
                        .map_err(|error| format!("Workflow step schema is invalid: {error}"))?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        )
        .block(
            "edge",
            BlockSchema::new(edge)
                .occurrences(
                    Cardinality::new(1, Some(quotas.max_edges))
                        .map_err(|error| format!("Workflow edge schema is invalid: {error}"))?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        );
    Ok(Schema::new().block(
        "workflow",
        BlockSchema::new(workflow).occurrences(Cardinality::exactly(1)),
    ))
}

fn validate_schema(document: &Document, schema: &Schema) -> Result<(), String> {
    let report = validate_document(document, schema);
    if report.is_empty() {
        return Ok(());
    }
    let diagnostics = report
        .diagnostics
        .iter()
        .take(8)
        .map(|item| format!("{} at {}", item.code, item.path))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "Workflow definition ACL does not match its closed schema: {diagnostics}"
    ))
}

fn workflow_document(spec: &WorkflowSpec) -> Document {
    let mut root = BlockBuilder::new("workflow")
        .attr("schema", string(WORKFLOW_DEFINITION_SCHEMA))
        .attr("name", string(&spec.name))
        .attr("description", string(&spec.description));
    for step in &spec.steps {
        let mut block = BlockBuilder::new("step")
            .label(&step.id)
            .attr("label", string(&step.label))
            .attr("kind", string(step.kind.as_str()))
            .attr(
                "configuration_digest",
                string(step.configuration_digest.as_str()),
            )
            .attr(
                "input_schema_digest",
                string(step.input_schema_digest.as_str()),
            )
            .attr(
                "output_schema_digest",
                string(step.output_schema_digest.as_str()),
            );
        if let Some(digest) = &step.policy_digest {
            block = block.attr("policy_digest", string(digest.as_str()));
        }
        if let Some(reference) = &step.capability {
            block = block.nested_block(capability_block(reference));
        }
        root = root.nested_block(block.build());
    }
    for edge in &spec.edges {
        let mut block = BlockBuilder::new("edge")
            .label(&edge.id)
            .attr("source", string(&edge.source))
            .attr("target", string(&edge.target));
        if let Some(handle) = &edge.source_handle {
            block = block.attr("source_handle", string(handle));
        }
        root = root.nested_block(block.build());
    }
    Document {
        blocks: vec![root.build()],
    }
}

fn capability_block(reference: &CapabilityReference) -> Block {
    BlockBuilder::new("capability")
        .attr("owner", string(reference.owner.as_str()))
        .attr("type", string(reference.capability_type.as_str()))
        .attr("resource_id", string(&reference.resource_id.to_string()))
        .attr("revision", string(&reference.revision))
        .attr("digest", string(reference.digest.as_str()))
        .attr("capability", string(&reference.capability))
        .build()
}

fn parse_step(block: &Block) -> Result<WorkflowStepSpec, String> {
    let id = block
        .labels
        .first()
        .cloned()
        .ok_or_else(|| "Workflow step label is missing".to_owned())?;
    let capability = block
        .blocks
        .iter()
        .find(|nested| nested.name == "capability")
        .map(parse_capability)
        .transpose()?;
    Ok(WorkflowStepSpec {
        id,
        label: required_string(block, "label")?,
        kind: WorkflowStepKind::parse(&required_string(block, "kind")?)?,
        configuration_digest: parse_digest(block, "configuration_digest")?,
        input_schema_digest: parse_digest(block, "input_schema_digest")?,
        output_schema_digest: parse_digest(block, "output_schema_digest")?,
        policy_digest: optional_string(block, "policy_digest")?
            .map(Sha256Digest::parse)
            .transpose()?,
        capability,
    })
}

fn parse_capability(block: &Block) -> Result<CapabilityReference, String> {
    Ok(CapabilityReference {
        owner: CapabilityOwner::parse(&required_string(block, "owner")?)?,
        capability_type: CapabilityType::parse(&required_string(block, "type")?)?,
        resource_id: Uuid::parse_str(&required_string(block, "resource_id")?)
            .map_err(|error| format!("Workflow capability resource ID is invalid: {error}"))?,
        revision: required_string(block, "revision")?,
        digest: parse_digest(block, "digest")?,
        capability: required_string(block, "capability")?,
    })
}

fn parse_edge(block: &Block) -> Result<WorkflowEdgeSpec, String> {
    Ok(WorkflowEdgeSpec {
        id: block
            .labels
            .first()
            .cloned()
            .ok_or_else(|| "Workflow edge label is missing".to_owned())?,
        source: required_string(block, "source")?,
        target: required_string(block, "target")?,
        source_handle: optional_string(block, "source_handle")?,
    })
}

fn parse_digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn step(id: &str, kind: WorkflowStepKind) -> WorkflowStepSpec {
        WorkflowStepSpec {
            id: id.into(),
            label: id.into(),
            kind,
            configuration_digest: digest('a'),
            input_schema_digest: digest('b'),
            output_schema_digest: digest('c'),
            policy_digest: None,
            capability: None,
        }
    }

    fn edge(id: &str, source: &str, target: &str) -> WorkflowEdgeSpec {
        WorkflowEdgeSpec {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            source_handle: None,
        }
    }

    fn capability(capability_type: CapabilityType) -> CapabilityReference {
        CapabilityReference {
            owner: capability_type.owner(),
            capability_type,
            resource_id: Uuid::now_v7(),
            revision: if matches!(
                capability_type,
                CapabilityType::FormRelease | CapabilityType::ExecutionTemplate
            ) {
                Uuid::now_v7().to_string()
            } else {
                "revision-1".into()
            },
            digest: digest('d'),
            capability: if capability_type == CapabilityType::ExecutionTemplate {
                "execution.run".into()
            } else {
                "workflow.test".into()
            },
        }
    }

    fn fixture() -> WorkflowSpec {
        WorkflowSpec {
            name: "Triage request".into(),
            description: "Deterministic contract fixture".into(),
            steps: vec![
                step("input", WorkflowStepKind::Input),
                step("transform", WorkflowStepKind::Transform),
                step("output", WorkflowStepKind::Output),
            ],
            edges: vec![
                edge("input-transform", "input", "transform"),
                edge("transform-output", "transform", "output"),
            ],
        }
    }

    #[test]
    fn closed_acl_round_trips_and_reordering_does_not_change_digest() {
        let contract = WorkflowContract::from_spec(fixture()).expect("contract");
        let restored = WorkflowContract::parse_acl(contract.canonical_acl()).expect("restore");
        assert_eq!(restored, contract);
        assert_eq!(
            restored
                .spec()
                .topological_order(Default::default())
                .expect("order"),
            ["input", "transform", "output"]
        );

        let mut reordered = fixture();
        reordered.steps.reverse();
        reordered.edges.reverse();
        assert_eq!(
            WorkflowContract::from_spec(reordered)
                .expect("reordered")
                .digest(),
            contract.digest()
        );
    }

    #[test]
    fn standalone_node_capabilities_have_an_explicit_cloud_mapping() {
        let cases = [
            ("start", WorkflowStepKind::Input),
            ("template", WorkflowStepKind::Transform),
            ("llm", WorkflowStepKind::Model),
            ("agent", WorkflowStepKind::Agent),
            ("tool", WorkflowStepKind::Tool),
            ("router", WorkflowStepKind::Branch),
            ("memory", WorkflowStepKind::Memory),
            ("http", WorkflowStepKind::Service),
            ("approval", WorkflowStepKind::HumanDecision),
            ("output", WorkflowStepKind::Output),
        ];
        for (standalone, cloud) in cases {
            assert_eq!(
                WorkflowStepKind::from_standalone_kind(standalone).expect("mapping"),
                cloud
            );
        }
        assert!(WorkflowStepKind::from_standalone_kind("runtime_provider").is_err());
    }

    #[test]
    fn graph_validation_rejects_cycles_dangling_edges_and_invalid_branch_handles() {
        let mut cyclic = fixture();
        cyclic
            .steps
            .insert(2, step("loop", WorkflowStepKind::Transform));
        cyclic.edges = vec![
            edge("a", "input", "transform"),
            edge("b", "transform", "loop"),
            edge("c", "loop", "transform"),
            edge("d", "loop", "output"),
        ];
        assert!(cyclic.validate(Default::default()).is_err());

        let mut dangling = fixture();
        dangling.edges[0].source = "missing".into();
        assert!(dangling.validate(Default::default()).is_err());

        let mut branch = fixture();
        branch.steps[1].kind = WorkflowStepKind::Branch;
        assert!(branch.validate(Default::default()).is_err());
        branch.edges[1].source_handle = Some("selected".into());
        branch.validate(Default::default()).expect("named branch");
        branch.edges.push(WorkflowEdgeSpec {
            id: "duplicate-handle".into(),
            source: "transform".into(),
            target: "output".into(),
            source_handle: Some("selected".into()),
        });
        assert!(branch.validate(Default::default()).is_err());
    }

    #[test]
    fn graph_validation_accepts_multiple_reachable_output_sinks() {
        let mut workflow = fixture();
        workflow.steps[2] = step("output-a", WorkflowStepKind::Output);
        workflow
            .steps
            .push(step("output-b", WorkflowStepKind::Output));
        workflow.edges = vec![
            edge("input-transform", "input", "transform"),
            edge("transform-output-a", "transform", "output-a"),
            edge("transform-output-b", "transform", "output-b"),
        ];

        assert_eq!(
            workflow
                .topological_order(Default::default())
                .expect("multiple output sinks"),
            ["input", "transform", "output-a", "output-b"]
        );

        workflow
            .edges
            .push(edge("output-a-output-b", "output-a", "output-b"));
        assert!(workflow.validate(Default::default()).is_err());
    }

    #[test]
    fn every_external_step_requires_its_exact_owning_capability_type() {
        let cases = [
            (WorkflowStepKind::HumanDecision, CapabilityType::FormRelease),
            (
                WorkflowStepKind::Execution,
                CapabilityType::ExecutionTemplate,
            ),
            (WorkflowStepKind::Agent, CapabilityType::AgentRelease),
            (WorkflowStepKind::Mcp, CapabilityType::McpServiceProfile),
            (WorkflowStepKind::Model, CapabilityType::ModelRevision),
            (WorkflowStepKind::Tool, CapabilityType::UsePackage),
            (WorkflowStepKind::Memory, CapabilityType::UsePackage),
            (WorkflowStepKind::Service, CapabilityType::ConnectorRevision),
            (
                WorkflowStepKind::Subworkflow,
                CapabilityType::WorkflowRevision,
            ),
        ];
        for (step_kind, capability_type) in cases {
            let mut spec = fixture();
            spec.steps[1].kind = step_kind;
            assert!(spec.validate(Default::default()).is_err());
            spec.steps[1].capability = Some(capability(capability_type));
            spec.validate(Default::default())
                .unwrap_or_else(|error| panic!("{step_kind:?} binding failed: {error}"));
        }

        let mut mismatched = fixture();
        mismatched.steps[1].kind = WorkflowStepKind::Agent;
        mismatched.steps[1].capability = Some(capability(CapabilityType::McpServiceProfile));
        assert!(mismatched.validate(Default::default()).is_err());

        let mut local = fixture();
        local.steps[1].capability = Some(capability(CapabilityType::ExecutionTemplate));
        assert!(local.validate(Default::default()).is_err());
    }

    #[test]
    fn human_decision_form_release_binding_round_trips_through_closed_acl() {
        let form_id = Uuid::now_v7();
        let release_id = Uuid::now_v7().to_string();
        let form_digest = digest('f');
        let mut spec = fixture();
        spec.steps[1].kind = WorkflowStepKind::HumanDecision;
        spec.steps[1].capability = Some(CapabilityReference {
            owner: CapabilityOwner::Forms,
            capability_type: CapabilityType::FormRelease,
            resource_id: form_id,
            revision: release_id.clone(),
            digest: form_digest.clone(),
            capability: "form.interact".into(),
        });

        let contract = WorkflowContract::from_spec(spec).expect("human-decision contract");
        assert!(contract.canonical_acl().contains("owner = \"forms\""));
        assert!(contract.canonical_acl().contains("type = \"form_release\""));

        let restored = WorkflowContract::parse_acl(contract.canonical_acl()).expect("restore");
        let binding = restored
            .spec()
            .steps
            .iter()
            .find(|step| step.id == "transform")
            .and_then(|step| step.capability.as_ref())
            .expect("FormRelease binding");
        assert_eq!(binding.owner, CapabilityOwner::Forms);
        assert_eq!(binding.capability_type, CapabilityType::FormRelease);
        assert_eq!(binding.resource_id, form_id);
        assert_eq!(binding.revision, release_id);
        assert_eq!(binding.digest, form_digest);
        assert_eq!(binding.capability, "form.interact");
    }

    #[test]
    fn graph_identifiers_remain_url_safe() {
        for invalid in ["contains.dot", "contains/slash", "contains space"] {
            let mut spec = fixture();
            spec.steps[1].id = invalid.into();
            spec.edges[0].target = invalid.into();
            spec.edges[1].source = invalid.into();
            let error = spec
                .validate(Default::default())
                .expect_err("unsafe Workflow step ID must fail");
            assert!(error.contains("letters, numbers, hyphens, or underscores"));
        }
    }

    #[test]
    fn closed_acl_rejects_runtime_provider_pool_secret_and_unknown_fields() {
        let contract = WorkflowContract::from_spec(fixture()).expect("contract");
        for injected in [
            "  provider = \"local\"\n",
            "  pool = \"gpu\"\n",
            "  runtime = \"process\"\n",
            "  secrets = [\"plaintext\"]\n",
            "  unknown = true\n",
        ] {
            let acl = contract.canonical_acl().replacen(
                "workflow {\n",
                &format!("workflow {{\n{injected}"),
                1,
            );
            assert!(WorkflowContract::parse_acl(&acl).is_err(), "{injected}");
        }
    }

    #[test]
    fn quotas_and_stored_digest_are_enforced() {
        let contract = WorkflowContract::from_spec(fixture()).expect("contract");
        assert!(WorkflowContract::parse_acl_with_quotas(
            contract.canonical_acl(),
            WorkflowContractQuotas {
                max_acl_bytes: 1,
                ..Default::default()
            }
        )
        .is_err());
        assert!(WorkflowContract::restore(
            contract.canonical_acl(),
            &format!("sha256:{}", "f".repeat(64))
        )
        .is_err());
    }

    #[test]
    fn public_w0_1_workflow_fixture_is_admitted() {
        let fixture = include_str!("../../../../../../contracts/w0.1/workflow.acl");
        let contract = WorkflowContract::parse_acl(fixture).expect("public Workflow fixture");
        assert_eq!(contract.spec().steps.len(), 3);
        assert_eq!(contract.spec().edges.len(), 2);
        assert!(contract.digest().as_str().starts_with("sha256:"));
    }
}
