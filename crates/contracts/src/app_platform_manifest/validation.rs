use super::inventory::{REQUIRED_CAPABILITIES, REQUIRED_REFERENCES};
use super::{
    AppPlatformCapability, AppPlatformCapabilityAvailability, AppPlatformCapabilityCategory,
    AppPlatformGate, AppPlatformGateState, AppPlatformReference, CAPABILITY_ATTRIBUTES,
    CAPABILITY_BLOCK, EVIDENCE_KINDS, GATE_ATTRIBUTES, GATE_BLOCK, MANIFEST_BASELINE,
    MANIFEST_BLOCK, MANIFEST_ID, OWNERS, REFERENCE_ATTRIBUTES, REFERENCE_BLOCK, ROOT_ATTRIBUTES,
};
use a3s_acl::{Block, Document, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ParsedManifestEntries {
    pub(super) references: Vec<AppPlatformReference>,
    pub(super) gates: Vec<AppPlatformGate>,
    pub(super) capabilities: Vec<AppPlatformCapability>,
}

pub(super) fn exact_root_block(document: &Document) -> Result<&Block, String> {
    if document.blocks.len() != 1 {
        return Err("application-platform parity manifest must contain one root block".into());
    }
    let root = &document.blocks[0];
    if root.name != MANIFEST_BLOCK || root.labels != [MANIFEST_ID] {
        return Err("application-platform parity manifest root identity is invalid".into());
    }
    exact_attributes(root, &ROOT_ATTRIBUTES, "parity manifest")?;
    Ok(root)
}

pub(super) fn parse_entries(root: &Block) -> Result<ParsedManifestEntries, String> {
    let mut references = Vec::new();
    let mut gates = Vec::new();
    let mut capabilities = Vec::new();
    let mut seen_gate = false;
    let mut seen_capability = false;
    for block in &root.blocks {
        match block.name.as_str() {
            REFERENCE_BLOCK if !seen_gate && !seen_capability => {
                references.push(parse_reference(block)?)
            }
            REFERENCE_BLOCK => {
                return Err("application-platform references must precede gates".into())
            }
            GATE_BLOCK if !seen_capability => {
                seen_gate = true;
                gates.push(parse_gate(block)?);
            }
            GATE_BLOCK => return Err("application-platform gates must precede capabilities".into()),
            CAPABILITY_BLOCK => {
                seen_capability = true;
                capabilities.push(parse_capability(block)?);
            }
            name => {
                return Err(format!(
                    "application-platform parity manifest contains unknown block {name:?}"
                ))
            }
        }
    }
    require_strict_id_order(references.iter().map(AppPlatformReference::id), "reference")?;
    require_strict_id_order(gates.iter().map(AppPlatformGate::id), "gate")?;
    require_strict_id_order(
        capabilities.iter().map(AppPlatformCapability::id),
        "capability",
    )?;
    Ok(ParsedManifestEntries {
        references,
        gates,
        capabilities,
    })
}

fn parse_reference(block: &Block) -> Result<AppPlatformReference, String> {
    exact_labeled_leaf(block, REFERENCE_BLOCK, &REFERENCE_ATTRIBUTES, "reference")?;
    let id = block.labels[0].clone();
    validate_reference_id(&id)?;
    let observed_on = required_string(block, "observed_on")?;
    if observed_on != MANIFEST_BASELINE {
        return Err(format!(
            "application-platform reference {id:?} observation date must match the baseline"
        ));
    }
    let url = required_string(block, "url")?;
    if url.len() > 512
        || !url.starts_with("https://")
        || url.chars().any(char::is_whitespace)
        || url.contains(['#', '?'])
    {
        return Err(format!(
            "application-platform reference {id:?} URL is invalid"
        ));
    }
    Ok(AppPlatformReference {
        id,
        observed_on,
        url,
    })
}

fn parse_gate(block: &Block) -> Result<AppPlatformGate, String> {
    exact_labeled_leaf(block, GATE_BLOCK, &GATE_ATTRIBUTES, "gate")?;
    let id = block.labels[0].clone();
    validate_gate_id(&id)?;
    let state = AppPlatformGateState::parse(&required_string(block, "state")?)?;
    let evidence = required_string_list(block, "evidence")?;
    validate_evidence(&evidence, "gate")?;
    if state == AppPlatformGateState::Verified
        && !evidence.iter().any(|item| item.starts_with("test:"))
    {
        return Err(format!("verified gate {id:?} requires test evidence"));
    }
    Ok(AppPlatformGate {
        id,
        state,
        evidence,
    })
}

fn parse_capability(block: &Block) -> Result<AppPlatformCapability, String> {
    exact_labeled_leaf(
        block,
        CAPABILITY_BLOCK,
        &CAPABILITY_ATTRIBUTES,
        "capability",
    )?;
    let id = block.labels[0].clone();
    validate_capability_id(&id)?;
    let category = AppPlatformCapabilityCategory::parse(&required_string(block, "category")?)?;
    if !id.starts_with(category.id_prefix()) {
        return Err(format!(
            "application-platform capability {id:?} does not match category {:?}",
            category.as_str()
        ));
    }
    let label = required_string(block, "label")?;
    validate_label(&label)?;
    let owner = required_string(block, "owner")?;
    if !OWNERS.contains(&owner.as_str()) {
        return Err(format!(
            "application-platform capability {id:?} has unknown owner {owner:?}"
        ));
    }
    let gate = required_string(block, "gate")?;
    validate_gate_id(&gate)?;
    let dependencies = required_string_list(block, "dependencies")?;
    validate_sorted_unique(&dependencies, "capability dependencies", true)?;
    for dependency in &dependencies {
        validate_gate_id(dependency)?;
        if dependency == &gate {
            return Err(format!(
                "application-platform capability {id:?} repeats its owning gate as a dependency"
            ));
        }
    }
    let availability =
        AppPlatformCapabilityAvailability::parse(&required_string(block, "availability")?)?;
    let evidence = required_string_list(block, "evidence")?;
    validate_evidence(&evidence, "capability")?;
    let references = required_string_list(block, "references")?;
    validate_sorted_unique(&references, "capability references", false)?;
    for reference in &references {
        validate_reference_id(reference)?;
    }
    if availability != AppPlatformCapabilityAvailability::Unavailable
        && !evidence
            .iter()
            .any(|item| item.starts_with("implementation:") || item.starts_with("test:"))
    {
        return Err(format!(
            "available application-platform capability {id:?} requires implementation or test evidence"
        ));
    }
    Ok(AppPlatformCapability {
        id,
        category,
        label,
        owner,
        gate,
        dependencies,
        availability,
        evidence,
        references,
    })
}

pub(super) fn validate_manifest(
    public_claim_gate: &str,
    parity_claim: bool,
    references: &[AppPlatformReference],
    gates: &[AppPlatformGate],
    capabilities: &[AppPlatformCapability],
) -> Result<(), String> {
    let actual_references = references
        .iter()
        .map(|reference| (reference.id.as_str(), reference.url.as_str()))
        .collect::<BTreeSet<_>>();
    let required_references = REQUIRED_REFERENCES.iter().copied().collect::<BTreeSet<_>>();
    let mut used_references = BTreeSet::new();
    let gate_states = gates
        .iter()
        .map(|gate| (gate.id.as_str(), gate.state))
        .collect::<BTreeMap<_, _>>();
    if !gate_states.contains_key(public_claim_gate) {
        return Err(format!(
            "public parity claim references unknown gate {public_claim_gate:?}"
        ));
    }
    let mut referenced_gates = BTreeSet::from([public_claim_gate]);
    // APP0.1 is the immutable Application/ApplicationRelease authority gate.
    // Every application mode relies on that aggregate even when its behavioral
    // delivery gate is APP0.2 or APP0.4.
    referenced_gates.insert("APP0.1");
    for capability in capabilities {
        for reference in &capability.references {
            if !actual_references
                .iter()
                .any(|(id, _)| id == &reference.as_str())
            {
                return Err(format!(
                    "application-platform capability {:?} references unknown public source {:?}",
                    capability.id, reference
                ));
            }
            used_references.insert(reference.as_str());
        }
        let owning_state = gate_states.get(capability.gate.as_str()).ok_or_else(|| {
            format!(
                "application-platform capability {:?} references unknown gate {:?}",
                capability.id, capability.gate
            )
        })?;
        referenced_gates.insert(capability.gate.as_str());
        for dependency in &capability.dependencies {
            if !gate_states.contains_key(dependency.as_str()) {
                return Err(format!(
                    "application-platform capability {:?} references unknown dependency {:?}",
                    capability.id, dependency
                ));
            }
            referenced_gates.insert(dependency);
        }
        if capability.availability == AppPlatformCapabilityAvailability::Public {
            if *owning_state != AppPlatformGateState::Verified {
                return Err(format!(
                    "public application-platform capability {:?} has an unverified owning gate",
                    capability.id
                ));
            }
            if capability.dependencies.iter().any(|dependency| {
                gate_states.get(dependency.as_str()) != Some(&AppPlatformGateState::Verified)
            }) {
                return Err(format!(
                    "public application-platform capability {:?} has an unverified dependency",
                    capability.id
                ));
            }
            if !capability
                .evidence
                .iter()
                .any(|item| item.starts_with("test:"))
            {
                return Err(format!(
                    "public application-platform capability {:?} requires test evidence",
                    capability.id
                ));
            }
        }
    }
    if used_references.len() != references.len() {
        return Err(
            "application-platform parity manifest contains an unused public reference".into(),
        );
    }
    if actual_references != required_references {
        return Err("application-platform public reference registry drifted".into());
    }
    for gate in gates {
        if !referenced_gates.contains(gate.id.as_str()) {
            return Err(format!(
                "application-platform parity manifest contains unreferenced gate {:?}",
                gate.id
            ));
        }
    }
    validate_required_inventory(capabilities)?;
    if parity_claim {
        if gate_states.get(public_claim_gate) != Some(&AppPlatformGateState::Verified) {
            return Err("full application-platform parity claim gate is not verified".into());
        }
        if capabilities
            .iter()
            .any(|capability| capability.availability != AppPlatformCapabilityAvailability::Public)
        {
            return Err(
                "full application-platform parity claim includes unavailable capabilities".into(),
            );
        }
    }
    Ok(())
}

fn validate_required_inventory(capabilities: &[AppPlatformCapability]) -> Result<(), String> {
    let actual = capabilities
        .iter()
        .map(|capability| (capability.id.as_str(), capability.category))
        .collect::<BTreeSet<_>>();
    let required = REQUIRED_CAPABILITIES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if actual == required {
        return Ok(());
    }
    let missing = required
        .difference(&actual)
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let unexpected = actual
        .difference(&required)
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    Err(format!(
        "application-platform parity inventory drifted; missing={missing:?}, unexpected={unexpected:?}"
    ))
}

fn exact_labeled_leaf(
    block: &Block,
    expected_name: &str,
    expected_attributes: &[&str],
    label: &str,
) -> Result<(), String> {
    if block.name != expected_name || block.labels.len() != 1 || !block.blocks.is_empty() {
        return Err(format!(
            "application-platform {label} block shape is invalid"
        ));
    }
    exact_attributes(block, expected_attributes, label)
}

fn exact_attributes(block: &Block, expected: &[&str], label: &str) -> Result<(), String> {
    if block.attributes.len() != expected.len()
        || block
            .attributes
            .keys()
            .any(|key| !expected.contains(&key.as_str()))
    {
        return Err(format!(
            "application-platform {label} contains missing or unknown fields"
        ));
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("application-platform field {name:?} is required"))
}

pub(super) fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("application-platform field {name:?} must be a string"))
}

pub(super) fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    required_value(block, name)?
        .as_bool()
        .ok_or_else(|| format!("application-platform field {name:?} must be a boolean"))
}

fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "application-platform field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("application-platform field {name:?} must be a string list"))
        })
        .collect()
}

fn validate_gate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 32
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
        || value
            .bytes()
            .next()
            .is_none_or(|byte| !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "application-platform gate identifier {value:?} is invalid"
        ));
    }
    Ok(())
}

fn validate_capability_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 96
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
    {
        return Err(format!(
            "application-platform capability identifier {value:?} is invalid"
        ));
    }
    Ok(())
}

fn validate_reference_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!(
            "application-platform public reference identifier {value:?} is invalid"
        ));
    }
    Ok(())
}

fn validate_label(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 120
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err("application-platform capability label is invalid".into());
    }
    Ok(())
}

fn validate_evidence(values: &[String], label: &str) -> Result<(), String> {
    validate_sorted_unique(values, &format!("{label} evidence"), false)?;
    for value in values {
        let (kind, reference) = value.split_once(':').ok_or_else(|| {
            format!("application-platform {label} evidence {value:?} is not typed")
        })?;
        if !EVIDENCE_KINDS.contains(&kind) {
            return Err(format!(
                "application-platform {label} evidence kind {kind:?} is invalid"
            ));
        }
        let path = reference.split('#').next().unwrap_or_default();
        if path.is_empty()
            || path.len() > 255
            || path.starts_with('/')
            || path.contains(['\\', '?'])
            || path
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
        {
            return Err(format!(
                "application-platform {label} evidence path is invalid"
            ));
        }
    }
    Ok(())
}

fn validate_sorted_unique(values: &[String], label: &str, allow_empty: bool) -> Result<(), String> {
    if (!allow_empty && values.is_empty()) || !values.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(format!(
            "application-platform {label} must be a sorted unique list"
        ));
    }
    Ok(())
}

fn require_strict_id_order<'a>(
    values: impl Iterator<Item = &'a str>,
    label: &str,
) -> Result<(), String> {
    let values = values.collect::<Vec<_>>();
    if !values.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(format!(
            "application-platform {label} blocks must use strict identifier order"
        ));
    }
    Ok(())
}
