use crate::modules::workflow::domain::value_objects::OntologyMigrationPolicy;
use crate::modules::workflow::domain::{OntologyContract, OntologyRuleKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyResourceKind {
    Metadata,
    ObjectType,
    RelationType,
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyChangeKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyChangeCompatibility {
    Compatible,
    Breaking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyChange {
    pub resource_kind: OntologyResourceKind,
    pub resource_id: String,
    pub change_kind: OntologyChangeKind,
    pub compatibility: OntologyChangeCompatibility,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyDiff {
    pub from_digest: String,
    pub to_digest: String,
    pub breaking: bool,
    pub changes: Vec<OntologyChange>,
}

pub fn diff_ontology_contracts(from: &OntologyContract, to: &OntologyContract) -> OntologyDiff {
    let mut changes = Vec::new();
    let from_spec = from.spec();
    let to_spec = to.spec();
    let mut metadata_fields = Vec::new();
    if from_spec.name != to_spec.name {
        metadata_fields.push("name".to_owned());
    }
    if from_spec.description != to_spec.description {
        metadata_fields.push("description".to_owned());
    }
    if !metadata_fields.is_empty() {
        changes.push(OntologyChange {
            resource_kind: OntologyResourceKind::Metadata,
            resource_id: "ontology".into(),
            change_kind: OntologyChangeKind::Changed,
            compatibility: OntologyChangeCompatibility::Compatible,
            changed_fields: metadata_fields,
        });
    }

    let from_objects = from_spec
        .object_types
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let to_objects = to_spec
        .object_types
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    for id in union_keys(&from_objects, &to_objects) {
        match (from_objects.get(id), to_objects.get(id)) {
            (None, Some(_)) => changes.push(change(
                OntologyResourceKind::ObjectType,
                id,
                OntologyChangeKind::Added,
                OntologyChangeCompatibility::Compatible,
                Vec::new(),
            )),
            (Some(_), None) => changes.push(change(
                OntologyResourceKind::ObjectType,
                id,
                OntologyChangeKind::Removed,
                OntologyChangeCompatibility::Breaking,
                Vec::new(),
            )),
            (Some(from), Some(to)) if from != to => {
                let mut fields = Vec::new();
                if from.label != to.label {
                    fields.push("label".to_owned());
                }
                if from.schema_digest != to.schema_digest {
                    fields.push("schema_digest".to_owned());
                }
                if from.key_fields != to.key_fields {
                    fields.push("key_fields".to_owned());
                }
                let compatibility = if fields.iter().all(|field| field == "label") {
                    OntologyChangeCompatibility::Compatible
                } else {
                    OntologyChangeCompatibility::Breaking
                };
                changes.push(change(
                    OntologyResourceKind::ObjectType,
                    id,
                    OntologyChangeKind::Changed,
                    compatibility,
                    fields,
                ));
            }
            _ => {}
        }
    }

    let from_relations = from_spec
        .relation_types
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let to_relations = to_spec
        .relation_types
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    for id in union_keys(&from_relations, &to_relations) {
        match (from_relations.get(id), to_relations.get(id)) {
            (None, Some(_)) => changes.push(change(
                OntologyResourceKind::RelationType,
                id,
                OntologyChangeKind::Added,
                OntologyChangeCompatibility::Compatible,
                Vec::new(),
            )),
            (Some(_), None) => changes.push(change(
                OntologyResourceKind::RelationType,
                id,
                OntologyChangeKind::Removed,
                OntologyChangeCompatibility::Breaking,
                Vec::new(),
            )),
            (Some(from), Some(to)) if from != to => {
                let mut fields = Vec::new();
                if from.label != to.label {
                    fields.push("label".to_owned());
                }
                if from.source_object != to.source_object {
                    fields.push("source_object".to_owned());
                }
                if from.target_object != to.target_object {
                    fields.push("target_object".to_owned());
                }
                if from.cardinality != to.cardinality {
                    fields.push("cardinality".to_owned());
                }
                if from.schema_digest != to.schema_digest {
                    fields.push("schema_digest".to_owned());
                }
                let compatibility = if fields.iter().all(|field| field == "label") {
                    OntologyChangeCompatibility::Compatible
                } else {
                    OntologyChangeCompatibility::Breaking
                };
                changes.push(change(
                    OntologyResourceKind::RelationType,
                    id,
                    OntologyChangeKind::Changed,
                    compatibility,
                    fields,
                ));
            }
            _ => {}
        }
    }

    let from_rules = from_spec
        .rules
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let to_rules = to_spec
        .rules
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    for id in union_keys(&from_rules, &to_rules) {
        match (from_rules.get(id), to_rules.get(id)) {
            (None, Some(rule)) => {
                let compatibility = match rule.kind {
                    OntologyRuleKind::Constraint | OntologyRuleKind::Eligibility => {
                        OntologyChangeCompatibility::Breaking
                    }
                    OntologyRuleKind::Derivation | OntologyRuleKind::Migration => {
                        OntologyChangeCompatibility::Compatible
                    }
                };
                changes.push(change(
                    OntologyResourceKind::Rule,
                    id,
                    OntologyChangeKind::Added,
                    compatibility,
                    Vec::new(),
                ));
            }
            (Some(_), None) => changes.push(change(
                OntologyResourceKind::Rule,
                id,
                OntologyChangeKind::Removed,
                OntologyChangeCompatibility::Breaking,
                Vec::new(),
            )),
            (Some(from), Some(to)) if from != to => {
                let mut fields = Vec::new();
                if from.label != to.label {
                    fields.push("label".to_owned());
                }
                if from.kind != to.kind {
                    fields.push("kind".to_owned());
                }
                if from.expression_digest != to.expression_digest {
                    fields.push("expression_digest".to_owned());
                }
                let compatibility = if fields.iter().all(|field| field == "label") {
                    OntologyChangeCompatibility::Compatible
                } else {
                    OntologyChangeCompatibility::Breaking
                };
                changes.push(change(
                    OntologyResourceKind::Rule,
                    id,
                    OntologyChangeKind::Changed,
                    compatibility,
                    fields,
                ));
            }
            _ => {}
        }
    }

    let breaking = changes
        .iter()
        .any(|change| change.compatibility == OntologyChangeCompatibility::Breaking);
    OntologyDiff {
        from_digest: from.digest().as_str().to_owned(),
        to_digest: to.digest().as_str().to_owned(),
        breaking,
        changes,
    }
}

pub fn resolve_migration_policy(
    target: &OntologyContract,
    diff: &OntologyDiff,
    explicit_rule_id: Option<&str>,
) -> Result<OntologyMigrationPolicy, String> {
    if diff.changes.is_empty() {
        return Err("Ontology revision must change canonical content".into());
    }
    match explicit_rule_id {
        None if diff.breaking => Err(
            "breaking Ontology revisions must reference a migration rule from the target ACL"
                .into(),
        ),
        None => Ok(OntologyMigrationPolicy::Compatible),
        Some(rule_id) => {
            let rule = target
                .spec()
                .rules
                .iter()
                .find(|rule| rule.id == rule_id)
                .ok_or_else(|| {
                    "explicit Ontology migration rule does not exist in the target ACL".to_owned()
                })?;
            if rule.kind != OntologyRuleKind::Migration {
                return Err("explicit Ontology migration rule must use kind migration".into());
            }
            Ok(OntologyMigrationPolicy::Explicit {
                rule_id: rule.id.clone(),
                expression_digest: rule.expression_digest.clone(),
            })
        }
    }
}

fn union_keys<'a, T>(
    left: &'a BTreeMap<&'a str, T>,
    right: &'a BTreeMap<&'a str, T>,
) -> Vec<&'a str> {
    left.keys()
        .chain(right.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn change(
    resource_kind: OntologyResourceKind,
    resource_id: &str,
    change_kind: OntologyChangeKind,
    compatibility: OntologyChangeCompatibility,
    changed_fields: Vec<String>,
) -> OntologyChange {
    OntologyChange {
        resource_kind,
        resource_id: resource_id.to_owned(),
        change_kind,
        compatibility,
        changed_fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use crate::modules::workflow::domain::{OntologyObjectType, OntologyRule, OntologySpec};

    fn digest(value: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", value.to_string().repeat(64))).expect("digest")
    }

    fn contract(object_digest: char, rules: Vec<OntologyRule>) -> OntologyContract {
        OntologyContract::from_spec(OntologySpec {
            name: "Commerce".into(),
            description: "Commerce graph".into(),
            object_types: vec![OntologyObjectType {
                id: "customer".into(),
                label: "Customer".into(),
                schema_digest: digest(object_digest),
                key_fields: vec!["id".into()],
            }],
            relation_types: Vec::new(),
            rules,
        })
        .expect("contract")
    }

    #[test]
    fn requires_target_migration_rule_for_breaking_change() {
        let from = contract('a', Vec::new());
        let to_without_rule = contract('b', Vec::new());
        let diff = diff_ontology_contracts(&from, &to_without_rule);
        assert!(diff.breaking);
        assert!(resolve_migration_policy(&to_without_rule, &diff, None).is_err());

        let to = contract(
            'b',
            vec![OntologyRule {
                id: "migrate_customer_v2".into(),
                label: "Migrate customer".into(),
                kind: OntologyRuleKind::Migration,
                expression_digest: digest('c'),
            }],
        );
        let diff = diff_ontology_contracts(&from, &to);
        let policy = resolve_migration_policy(&to, &diff, Some("migrate_customer_v2"))
            .expect("explicit migration");
        assert_eq!(policy.rule_id(), Some("migrate_customer_v2"));
        assert_eq!(policy.expression_digest(), Some(&digest('c')));
    }

    #[test]
    fn rejects_canonical_no_op_revision() {
        let from = contract('a', Vec::new());
        let diff = diff_ontology_contracts(&from, &from);
        assert!(diff.changes.is_empty());
        assert!(resolve_migration_policy(&from, &diff, None).is_err());
    }
}
