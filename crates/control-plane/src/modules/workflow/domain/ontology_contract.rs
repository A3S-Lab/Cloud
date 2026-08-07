use super::validation::{required_string, required_strings, validate_identifier, validate_text};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{list, string, BlockBuilder};
use a3s_acl::{
    canonical_digest_with_schema, generate_acl, parse_acl, validate_document, AttributeSchema,
    Block, BlockSchema, Cardinality, Document, Schema, ValueSchema,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const ONTOLOGY_SCHEMA: &str = "cloud.workflow.ontology.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OntologyContractQuotas {
    pub max_acl_bytes: usize,
    pub max_object_types: usize,
    pub max_relation_types: usize,
    pub max_rules: usize,
}

impl Default for OntologyContractQuotas {
    fn default() -> Self {
        Self {
            max_acl_bytes: 1024 * 1024,
            max_object_types: 512,
            max_relation_types: 4_096,
            max_rules: 4_096,
        }
    }
}

impl OntologyContractQuotas {
    fn validate(self) -> Result<Self, String> {
        if self.max_acl_bytes == 0
            || self.max_acl_bytes > 16 * 1024 * 1024
            || self.max_object_types == 0
            || self.max_object_types > 10_000
            || self.max_relation_types > 100_000
            || self.max_rules > 100_000
        {
            return Err("Ontology contract quotas are invalid".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntologyObjectType {
    pub id: String,
    pub label: String,
    pub schema_digest: Sha256Digest,
    pub key_fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyRelationCardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

impl OntologyRelationCardinality {
    const fn as_str(self) -> &'static str {
        match self {
            Self::OneToOne => "one_to_one",
            Self::OneToMany => "one_to_many",
            Self::ManyToOne => "many_to_one",
            Self::ManyToMany => "many_to_many",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "one_to_one" => Ok(Self::OneToOne),
            "one_to_many" => Ok(Self::OneToMany),
            "many_to_one" => Ok(Self::ManyToOne),
            "many_to_many" => Ok(Self::ManyToMany),
            _ => Err(format!(
                "unsupported Ontology relation cardinality {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntologyRelationType {
    pub id: String,
    pub label: String,
    pub source_object: String,
    pub target_object: String,
    pub cardinality: OntologyRelationCardinality,
    pub schema_digest: Sha256Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyRuleKind {
    Constraint,
    Derivation,
    Eligibility,
    Migration,
}

impl OntologyRuleKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Constraint => "constraint",
            Self::Derivation => "derivation",
            Self::Eligibility => "eligibility",
            Self::Migration => "migration",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "constraint" => Ok(Self::Constraint),
            "derivation" => Ok(Self::Derivation),
            "eligibility" => Ok(Self::Eligibility),
            "migration" => Ok(Self::Migration),
            _ => Err(format!("unsupported Ontology rule kind {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntologyRule {
    pub id: String,
    pub label: String,
    pub kind: OntologyRuleKind,
    pub expression_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntologySpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub object_types: Vec<OntologyObjectType>,
    #[serde(default)]
    pub relation_types: Vec<OntologyRelationType>,
    #[serde(default)]
    pub rules: Vec<OntologyRule>,
}

impl OntologySpec {
    pub fn validate(&self, quotas: OntologyContractQuotas) -> Result<(), String> {
        let quotas = quotas.validate()?;
        validate_text("Ontology name", &self.name, 1, 120)?;
        validate_text("Ontology description", &self.description, 0, 4_096)?;
        if self.object_types.is_empty() || self.object_types.len() > quotas.max_object_types {
            return Err(format!(
                "Ontology must contain between 1 and {} object types",
                quotas.max_object_types
            ));
        }
        if self.relation_types.len() > quotas.max_relation_types {
            return Err("Ontology exceeds its relation-type quota".into());
        }
        if self.rules.len() > quotas.max_rules {
            return Err("Ontology exceeds its rule quota".into());
        }

        let mut objects = BTreeMap::new();
        for object in &self.object_types {
            validate_identifier("Ontology object ID", &object.id)?;
            validate_text("Ontology object label", &object.label, 1, 120)?;
            if object.key_fields.is_empty() || object.key_fields.len() > 64 {
                return Err(format!(
                    "Ontology object {:?} must declare 1-64 key fields",
                    object.id
                ));
            }
            let mut keys = BTreeSet::new();
            for field in &object.key_fields {
                validate_identifier("Ontology key field", field)?;
                if !keys.insert(field) {
                    return Err(format!(
                        "Ontology object {:?} contains duplicate key field {field:?}",
                        object.id
                    ));
                }
            }
            if objects.insert(object.id.as_str(), object).is_some() {
                return Err(format!(
                    "Ontology contains duplicate object ID {:?}",
                    object.id
                ));
            }
        }

        let mut relations = BTreeSet::new();
        for relation in &self.relation_types {
            validate_identifier("Ontology relation ID", &relation.id)?;
            validate_text("Ontology relation label", &relation.label, 1, 120)?;
            if !relations.insert(relation.id.as_str()) {
                return Err(format!(
                    "Ontology contains duplicate relation ID {:?}",
                    relation.id
                ));
            }
            if !objects.contains_key(relation.source_object.as_str())
                || !objects.contains_key(relation.target_object.as_str())
            {
                return Err(format!(
                    "Ontology relation {:?} references an unknown object type",
                    relation.id
                ));
            }
        }

        let mut rules = BTreeSet::new();
        for rule in &self.rules {
            validate_identifier("Ontology rule ID", &rule.id)?;
            validate_text("Ontology rule label", &rule.label, 1, 120)?;
            if !rules.insert(rule.id.as_str()) {
                return Err(format!("Ontology contains duplicate rule ID {:?}", rule.id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyContract {
    spec: OntologySpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl OntologyContract {
    pub fn from_spec(spec: OntologySpec) -> Result<Self, String> {
        Self::from_spec_with_quotas(spec, OntologyContractQuotas::default())
    }

    pub fn from_spec_with_quotas(
        mut spec: OntologySpec,
        quotas: OntologyContractQuotas,
    ) -> Result<Self, String> {
        let quotas = quotas.validate()?;
        spec.object_types
            .sort_by(|left, right| left.id.cmp(&right.id));
        for object in &mut spec.object_types {
            object.key_fields.sort_unstable();
        }
        spec.relation_types
            .sort_by(|left, right| left.id.cmp(&right.id));
        spec.rules.sort_by(|left, right| left.id.cmp(&right.id));
        spec.validate(quotas)?;
        let schema = ontology_schema(quotas)?;
        let document = ontology_document(&spec);
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > quotas.max_acl_bytes {
            return Err("Ontology ACL exceeds its quota".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Ontology ACL is invalid: {error}"))?;
        validate_schema(&reparsed, &schema)?;
        let digest = Sha256Digest::parse(
            canonical_digest_with_schema(&reparsed, &schema)
                .map_err(|error| format!("Ontology is not canonicalizable: {error}"))?,
        )?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        Self::parse_acl_with_quotas(acl, OntologyContractQuotas::default())
    }

    pub fn parse_acl_with_quotas(
        acl: &str,
        quotas: OntologyContractQuotas,
    ) -> Result<Self, String> {
        let quotas = quotas.validate()?;
        if acl.is_empty() || acl.len() > quotas.max_acl_bytes {
            return Err("Ontology ACL size is invalid".into());
        }
        let document =
            parse_acl(acl).map_err(|error| format!("Ontology ACL is invalid: {error}"))?;
        let schema = ontology_schema(quotas)?;
        validate_schema(&document, &schema)?;
        let root = document
            .blocks
            .first()
            .ok_or_else(|| "Ontology block is missing".to_owned())?;
        if required_string(root, "schema")? != ONTOLOGY_SCHEMA {
            return Err("Ontology schema is unsupported".into());
        }
        let spec = OntologySpec {
            name: required_string(root, "name")?,
            description: required_string(root, "description")?,
            object_types: root
                .blocks
                .iter()
                .filter(|block| block.name == "object_type")
                .map(parse_object)
                .collect::<Result<Vec<_>, _>>()?,
            relation_types: root
                .blocks
                .iter()
                .filter(|block| block.name == "relation_type")
                .map(parse_relation)
                .collect::<Result<Vec<_>, _>>()?,
            rules: root
                .blocks
                .iter()
                .filter(|block| block.name == "rule")
                .map(parse_rule)
                .collect::<Result<Vec<_>, _>>()?,
        };
        Self::from_spec_with_quotas(spec, quotas)
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let contract = Self::parse_acl(acl)?;
        if contract.digest.as_str() != stored_digest {
            return Err("stored Ontology ACL and digest do not match".into());
        }
        Ok(contract)
    }

    pub const fn spec(&self) -> &OntologySpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn ontology_schema(quotas: OntologyContractQuotas) -> Result<Schema, String> {
    let object = Schema::new()
        .attribute("label", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "schema_digest",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "key_fields",
            AttributeSchema::required(ValueSchema::list(ValueSchema::string())),
        );
    let relation = Schema::new()
        .attribute("label", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "source_object",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "target_object",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "cardinality",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "schema_digest",
            AttributeSchema::required(ValueSchema::string()),
        );
    let rule = Schema::new()
        .attribute("label", AttributeSchema::required(ValueSchema::string()))
        .attribute("kind", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "expression_digest",
            AttributeSchema::required(ValueSchema::string()),
        );
    let ontology = Schema::new()
        .attribute("schema", AttributeSchema::required(ValueSchema::string()))
        .attribute("name", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "description",
            AttributeSchema::required(ValueSchema::string()),
        )
        .block(
            "object_type",
            BlockSchema::new(object)
                .occurrences(
                    Cardinality::new(1, Some(quotas.max_object_types))
                        .map_err(|error| format!("Ontology object schema is invalid: {error}"))?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        )
        .block(
            "relation_type",
            BlockSchema::new(relation)
                .occurrences(
                    Cardinality::new(0, Some(quotas.max_relation_types))
                        .map_err(|error| format!("Ontology relation schema is invalid: {error}"))?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        )
        .block(
            "rule",
            BlockSchema::new(rule)
                .occurrences(
                    Cardinality::new(0, Some(quotas.max_rules))
                        .map_err(|error| format!("Ontology rule schema is invalid: {error}"))?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        );
    Ok(Schema::new().block(
        "ontology",
        BlockSchema::new(ontology).occurrences(Cardinality::exactly(1)),
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
        "Ontology ACL does not match its closed schema: {diagnostics}"
    ))
}

fn ontology_document(spec: &OntologySpec) -> Document {
    let mut root = BlockBuilder::new("ontology")
        .attr("schema", string(ONTOLOGY_SCHEMA))
        .attr("name", string(&spec.name))
        .attr("description", string(&spec.description));
    for object in &spec.object_types {
        root = root.nested_block(
            BlockBuilder::new("object_type")
                .label(&object.id)
                .attr("label", string(&object.label))
                .attr("schema_digest", string(object.schema_digest.as_str()))
                .attr(
                    "key_fields",
                    list(
                        object
                            .key_fields
                            .iter()
                            .map(|value| string(value))
                            .collect(),
                    ),
                )
                .build(),
        );
    }
    for relation in &spec.relation_types {
        root = root.nested_block(
            BlockBuilder::new("relation_type")
                .label(&relation.id)
                .attr("label", string(&relation.label))
                .attr("source_object", string(&relation.source_object))
                .attr("target_object", string(&relation.target_object))
                .attr("cardinality", string(relation.cardinality.as_str()))
                .attr("schema_digest", string(relation.schema_digest.as_str()))
                .build(),
        );
    }
    for rule in &spec.rules {
        root = root.nested_block(
            BlockBuilder::new("rule")
                .label(&rule.id)
                .attr("label", string(&rule.label))
                .attr("kind", string(rule.kind.as_str()))
                .attr("expression_digest", string(rule.expression_digest.as_str()))
                .build(),
        );
    }
    Document {
        blocks: vec![root.build()],
    }
}

fn parse_object(block: &Block) -> Result<OntologyObjectType, String> {
    Ok(OntologyObjectType {
        id: label(block, "object type")?,
        label: required_string(block, "label")?,
        schema_digest: digest(block, "schema_digest")?,
        key_fields: required_strings(block, "key_fields")?,
    })
}

fn parse_relation(block: &Block) -> Result<OntologyRelationType, String> {
    Ok(OntologyRelationType {
        id: label(block, "relation type")?,
        label: required_string(block, "label")?,
        source_object: required_string(block, "source_object")?,
        target_object: required_string(block, "target_object")?,
        cardinality: OntologyRelationCardinality::parse(&required_string(block, "cardinality")?)?,
        schema_digest: digest(block, "schema_digest")?,
    })
}

fn parse_rule(block: &Block) -> Result<OntologyRule, String> {
    Ok(OntologyRule {
        id: label(block, "rule")?,
        label: required_string(block, "label")?,
        kind: OntologyRuleKind::parse(&required_string(block, "kind")?)?,
        expression_digest: digest(block, "expression_digest")?,
    })
}

fn label(block: &Block, kind: &str) -> Result<String, String> {
    block
        .labels
        .first()
        .cloned()
        .ok_or_else(|| format!("Ontology {kind} label is missing"))
}

fn digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_value(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn fixture() -> OntologySpec {
        OntologySpec {
            name: "Support ontology".into(),
            description: "Ticket ownership and eligibility".into(),
            object_types: vec![
                OntologyObjectType {
                    id: "ticket".into(),
                    label: "Ticket".into(),
                    schema_digest: digest_value('a'),
                    key_fields: vec!["id".into()],
                },
                OntologyObjectType {
                    id: "principal".into(),
                    label: "Principal".into(),
                    schema_digest: digest_value('b'),
                    key_fields: vec!["id".into()],
                },
            ],
            relation_types: vec![OntologyRelationType {
                id: "assigned_to".into(),
                label: "Assigned to".into(),
                source_object: "ticket".into(),
                target_object: "principal".into(),
                cardinality: OntologyRelationCardinality::ManyToOne,
                schema_digest: digest_value('c'),
            }],
            rules: vec![OntologyRule {
                id: "assignee_is_active".into(),
                label: "Assignee is active".into(),
                kind: OntologyRuleKind::Constraint,
                expression_digest: digest_value('d'),
            }],
        }
    }

    #[test]
    fn closed_ontology_acl_round_trips_with_stable_digest() {
        let contract = OntologyContract::from_spec(fixture()).expect("ontology");
        assert_eq!(
            OntologyContract::parse_acl(contract.canonical_acl()).expect("parse"),
            contract
        );
        let mut reordered = fixture();
        reordered.object_types.reverse();
        assert_eq!(
            OntologyContract::from_spec(reordered)
                .expect("reordered")
                .digest(),
            contract.digest()
        );
    }

    #[test]
    fn ontology_rejects_unknown_relations_duplicate_keys_and_unknown_fields() {
        let mut dangling = fixture();
        dangling.relation_types[0].target_object = "missing".into();
        assert!(dangling.validate(Default::default()).is_err());

        let mut duplicate_key = fixture();
        duplicate_key.object_types[0].key_fields.push("id".into());
        assert!(duplicate_key.validate(Default::default()).is_err());

        let contract = OntologyContract::from_spec(fixture()).expect("ontology");
        let unknown = contract.canonical_acl().replacen(
            "ontology {\n",
            "ontology {\n  graph_database = true\n",
            1,
        );
        assert!(OntologyContract::parse_acl(&unknown).is_err());
    }

    #[test]
    fn ontology_identifiers_remain_url_safe() {
        for invalid in ["contains.dot", "contains/slash", "contains space"] {
            let mut spec = fixture();
            spec.object_types[0].id = invalid.into();
            spec.relation_types[0].source_object = invalid.into();
            let error = spec
                .validate(Default::default())
                .expect_err("unsafe Ontology object ID must fail");
            assert!(error.contains("letters, numbers, hyphens, or underscores"));
        }
    }

    #[test]
    fn ontology_quota_and_stored_digest_are_enforced() {
        let contract = OntologyContract::from_spec(fixture()).expect("ontology");
        assert!(OntologyContract::parse_acl_with_quotas(
            contract.canonical_acl(),
            OntologyContractQuotas {
                max_object_types: 1,
                ..Default::default()
            }
        )
        .is_err());
        assert!(OntologyContract::restore(
            contract.canonical_acl(),
            &format!("sha256:{}", "f".repeat(64))
        )
        .is_err());
    }

    #[test]
    fn public_w0_1_ontology_fixture_is_admitted() {
        let fixture = include_str!("../../../../../../contracts/w0.1/ontology.acl");
        let contract = OntologyContract::parse_acl(fixture).expect("public Ontology fixture");
        assert_eq!(contract.spec().object_types.len(), 2);
        assert_eq!(contract.spec().relation_types.len(), 1);
        assert_eq!(contract.spec().rules.len(), 1);
    }
}
