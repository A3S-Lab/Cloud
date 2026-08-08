use super::validation::validate_text;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, EnvironmentId, OntologyId, OntologyRevisionId,
    Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{
    canonical_digest_with_schema, generate_acl, parse_acl, validate_document, AttributeSchema,
    Block, BlockSchema, Cardinality, Document, Schema, ValueSchema,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const WORKFLOW_GOAL_SCHEMA: &str = "cloud.workflow.goal.v1";
pub const WORKFLOW_GOAL_MAX_ACL_BYTES: usize = 256 * 1024;
pub const WORKFLOW_GOAL_MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowGoalSpec {
    pub name: String,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub workflow_revision_id: WorkflowRevisionId,
    pub workflow_digest: Sha256Digest,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    pub environment_id: Option<EnvironmentId>,
    pub input: serde_json::Value,
}

impl WorkflowGoalSpec {
    pub fn validate(&self) -> Result<(), String> {
        validate_text("Workflow goal name", &self.name, 1, 120)?;
        if self.workflow_definition_id.as_uuid().is_nil()
            || self.workflow_revision_id.as_uuid().is_nil()
            || self.ontology_id.as_uuid().is_nil()
            || self.ontology_revision_id.as_uuid().is_nil()
            || self
                .environment_id
                .is_some_and(|environment_id| environment_id.as_uuid().is_nil())
        {
            return Err("Workflow goal contains a nil authority identity".into());
        }
        canonical_json_bounded(
            &self.input,
            WORKFLOW_GOAL_MAX_INPUT_BYTES,
            "Workflow goal input",
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowGoalContract {
    spec: WorkflowGoalSpec,
    canonical_acl: String,
    digest: Sha256Digest,
    input_digest: Sha256Digest,
}

impl WorkflowGoalContract {
    pub fn from_spec(spec: WorkflowGoalSpec) -> Result<Self, String> {
        spec.validate()?;
        let canonical_input = canonical_json_bounded(
            &spec.input,
            WORKFLOW_GOAL_MAX_INPUT_BYTES,
            "Workflow goal input",
        )?;
        let input_digest = Sha256Digest::parse(sha256_digest(&canonical_input))?;
        let input_json = String::from_utf8(canonical_input)
            .map_err(|_| "Workflow goal input did not encode as UTF-8".to_owned())?;
        let schema = goal_schema();
        let document = goal_document(&spec, &input_json);
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > WORKFLOW_GOAL_MAX_ACL_BYTES {
            return Err("Workflow goal ACL exceeds its byte bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Workflow goal ACL is invalid: {error}"))?;
        validate_schema(&reparsed, &schema)?;
        let digest = Sha256Digest::parse(
            canonical_digest_with_schema(&reparsed, &schema)
                .map_err(|error| format!("Workflow goal is not canonicalizable: {error}"))?,
        )?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
            input_digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > WORKFLOW_GOAL_MAX_ACL_BYTES {
            return Err("Workflow goal ACL size is invalid".into());
        }
        let document =
            parse_acl(acl).map_err(|error| format!("Workflow goal ACL is invalid: {error}"))?;
        let schema = goal_schema();
        validate_schema(&document, &schema)?;
        let root = document
            .blocks
            .first()
            .ok_or_else(|| "Workflow goal block is missing".to_owned())?;
        if required_string(root, "schema")? != WORKFLOW_GOAL_SCHEMA {
            return Err("Workflow goal schema is unsupported".into());
        }
        let input_json = required_string(root, "input_json")?;
        if input_json.len() > WORKFLOW_GOAL_MAX_INPUT_BYTES {
            return Err("Workflow goal input exceeds its byte bound".into());
        }
        let input = serde_json::from_str(&input_json)
            .map_err(|error| format!("Workflow goal input JSON is invalid: {error}"))?;
        Self::from_spec(WorkflowGoalSpec {
            name: required_string(root, "name")?,
            workflow_definition_id: WorkflowDefinitionId::from_uuid(required_uuid(
                root,
                "workflow_definition_id",
            )?),
            workflow_revision_id: WorkflowRevisionId::from_uuid(required_uuid(
                root,
                "workflow_revision_id",
            )?),
            workflow_digest: Sha256Digest::parse(required_string(root, "workflow_digest")?)?,
            ontology_id: OntologyId::from_uuid(required_uuid(root, "ontology_id")?),
            ontology_revision_id: OntologyRevisionId::from_uuid(required_uuid(
                root,
                "ontology_revision_id",
            )?),
            ontology_digest: Sha256Digest::parse(required_string(root, "ontology_digest")?)?,
            environment_id: optional_string(root, "environment_id")?
                .map(|value| parse_uuid("environment_id", &value).map(EnvironmentId::from_uuid))
                .transpose()?,
            input,
        })
    }

    pub fn restore(
        acl: &str,
        stored_digest: &str,
        stored_input_digest: &str,
    ) -> Result<Self, String> {
        let value = Self::parse_acl(acl)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored Workflow goal ACL and digest do not match".into());
        }
        if value.input_digest.as_str() != stored_input_digest {
            return Err("stored Workflow goal input and digest do not match".into());
        }
        Ok(value)
    }

    pub const fn spec(&self) -> &WorkflowGoalSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub const fn input_digest(&self) -> &Sha256Digest {
        &self.input_digest
    }
}

fn goal_schema() -> Schema {
    let root = Schema::new()
        .attribute("schema", AttributeSchema::required(ValueSchema::string()))
        .attribute("name", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "workflow_definition_id",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "workflow_revision_id",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "workflow_digest",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "ontology_id",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "ontology_revision_id",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "ontology_digest",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "environment_id",
            AttributeSchema::optional(ValueSchema::string()),
        )
        .attribute(
            "input_json",
            AttributeSchema::required(ValueSchema::string()),
        );
    Schema::new().block(
        "goal",
        BlockSchema::new(root).occurrences(Cardinality::exactly(1)),
    )
}

fn goal_document(spec: &WorkflowGoalSpec, input_json: &str) -> Document {
    let mut root = BlockBuilder::new("goal")
        .attr("schema", string(WORKFLOW_GOAL_SCHEMA))
        .attr("name", string(&spec.name))
        .attr(
            "workflow_definition_id",
            string(&spec.workflow_definition_id.to_string()),
        )
        .attr(
            "workflow_revision_id",
            string(&spec.workflow_revision_id.to_string()),
        )
        .attr("workflow_digest", string(spec.workflow_digest.as_str()))
        .attr("ontology_id", string(&spec.ontology_id.to_string()))
        .attr(
            "ontology_revision_id",
            string(&spec.ontology_revision_id.to_string()),
        )
        .attr("ontology_digest", string(spec.ontology_digest.as_str()))
        .attr("input_json", string(input_json));
    if let Some(environment_id) = spec.environment_id {
        root = root.attr("environment_id", string(&environment_id.to_string()));
    }
    Document {
        blocks: vec![root.build()],
    }
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
        "Workflow goal ACL does not match its closed schema: {diagnostics}"
    ))
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    parse_uuid(name, &required_string(block, name)?)
}

fn parse_uuid(name: &str, value: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|error| format!("Workflow goal {name} is invalid: {error}"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    optional_string(block, name)?.ok_or_else(|| format!("Workflow goal {name} is missing"))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Workflow goal {name} must be a string"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn digest(value: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", value.to_string().repeat(64))).expect("digest")
    }

    #[test]
    fn goal_input_and_authority_bindings_are_canonical() {
        let spec = WorkflowGoalSpec {
            name: "Triage request".into(),
            workflow_definition_id: WorkflowDefinitionId::new(),
            workflow_revision_id: WorkflowRevisionId::new(),
            workflow_digest: digest('a'),
            ontology_id: OntologyId::new(),
            ontology_revision_id: OntologyRevisionId::new(),
            ontology_digest: digest('b'),
            environment_id: None,
            input: json!({"z": 2, "a": {"b": 2, "a": 1}}),
        };
        let goal = WorkflowGoalContract::from_spec(spec).expect("goal");
        assert!(goal
            .canonical_acl()
            .contains(r#"input_json = "{\"a\":{\"a\":1,\"b\":2},\"z\":2}""#));
        assert_eq!(
            WorkflowGoalContract::restore(
                goal.canonical_acl(),
                goal.digest().as_str(),
                goal.input_digest().as_str(),
            )
            .expect("restore"),
            goal
        );
    }

    #[test]
    fn goal_contract_rejects_unknown_fields() {
        let acl = format!(
            r#"goal {{
  schema = "{WORKFLOW_GOAL_SCHEMA}"
  name = "Goal"
  workflow_definition_id = "{}"
  workflow_revision_id = "{}"
  workflow_digest = "{}"
  ontology_id = "{}"
  ontology_revision_id = "{}"
  ontology_digest = "{}"
  input_json = "{{}}"
  unknown = "value"
}}"#,
            WorkflowDefinitionId::new(),
            WorkflowRevisionId::new(),
            digest('a'),
            OntologyId::new(),
            OntologyRevisionId::new(),
            digest('b'),
        );
        assert!(WorkflowGoalContract::parse_acl(&acl).is_err());
    }
}
