use super::DurableCellServiceProfile;
use crate::modules::shared_kernel::domain::{BuildRunId, Sha256Digest};
use a3s_acl::builder::{integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
pub use a3s_cloud_contracts::DURABLE_CELL_BUNDLE_MEDIA_TYPE;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_BUNDLE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CELL_CLASSES: usize = 128;
const DEFINITION_BLOCK: &str = "durable_cell_application";
const DEFINITION_ATTRIBUTES: [&str; 10] = [
    "build_run_id",
    "bundle_digest",
    "bundle_media_type",
    "bundle_size_bytes",
    "compatibility_date",
    "compatibility_flags",
    "main_module",
    "rollback_policy",
    "schema",
    "service_profile_digest",
];
const CELL_CLASS_ATTRIBUTES: [&str; 3] = [
    "maximum_readable_state_version",
    "minimum_readable_state_version",
    "write_state_version",
];

pub const DURABLE_CELL_APPLICATION_SCHEMA: &str = "cloud.durable-cell.application.v1";
pub const DURABLE_CELL_APPLICATION_MAX_ACL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableCellRollbackPolicy {
    Compatible,
    ForwardOnly,
}

impl DurableCellRollbackPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::ForwardOnly => "forward_only",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "compatible" => Ok(Self::Compatible),
            "forward_only" => Ok(Self::ForwardOnly),
            _ => Err(format!(
                "unsupported Durable Cell rollback policy {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellStateSchema {
    pub minimum_readable_version: u64,
    pub maximum_readable_version: u64,
    pub write_version: u64,
}

impl DurableCellStateSchema {
    pub fn validate(&self) -> Result<(), String> {
        if self.minimum_readable_version == 0
            || self.minimum_readable_version > self.write_version
            || self.write_version > self.maximum_readable_version
            || self.maximum_readable_version > MAX_SAFE_ACL_INTEGER
        {
            return Err("Durable Cell state schema range is invalid".into());
        }
        Ok(())
    }

    pub const fn can_read(&self, version: u64) -> bool {
        version >= self.minimum_readable_version && version <= self.maximum_readable_version
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellClassSpec {
    pub name: String,
    pub state_schema: DurableCellStateSchema,
}

impl DurableCellClassSpec {
    pub fn validate(&self) -> Result<(), String> {
        validate_cell_class_name(&self.name)?;
        self.state_schema.validate()
    }
}

/// Immutable application intent. The bundle is addressed by digest and linked
/// to the existing BuildRun provenance authority; its S0 location and provider
/// deployment pointer are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellApplicationDefinitionSpec {
    pub build_run_id: BuildRunId,
    pub bundle_digest: Sha256Digest,
    pub bundle_size_bytes: u64,
    pub main_module: String,
    pub compatibility_date: String,
    pub compatibility_flags: Vec<String>,
    pub cell_classes: Vec<DurableCellClassSpec>,
    pub service_profile_digest: Sha256Digest,
    pub rollback_policy: DurableCellRollbackPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellApplicationDefinition {
    spec: DurableCellApplicationDefinitionSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl DurableCellApplicationDefinition {
    pub fn from_spec(spec: DurableCellApplicationDefinitionSpec) -> Result<Self, String> {
        spec.validate()?;
        let document = definition_document(&spec)?;
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > DURABLE_CELL_APPLICATION_MAX_ACL_BYTES {
            return Err("Durable Cell application ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated Durable Cell application ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("Durable Cell application is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > DURABLE_CELL_APPLICATION_MAX_ACL_BYTES {
            return Err("Durable Cell application ACL size is invalid".into());
        }
        let document = parse_acl(acl)
            .map_err(|error| format!("Durable Cell application ACL is invalid: {error}"))?;
        Self::from_spec(parse_definition(&document)?)
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let definition = Self::parse_acl(acl)?;
        if definition.canonical_acl != acl || definition.digest.as_str() != stored_digest {
            return Err("stored Durable Cell application ACL and digest do not match".into());
        }
        Ok(definition)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(&self.canonical_acl, self.digest.as_str())?;
        if &restored != self {
            return Err("Durable Cell application definition drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub fn validate_service_profile(
        &self,
        profile: &DurableCellServiceProfile,
    ) -> Result<(), String> {
        self.validate()?;
        DurableCellServiceProfile::restore(profile.canonical_acl(), profile.digest().as_str())?;
        if &self.spec.service_profile_digest != profile.digest() {
            return Err("Durable Cell application and Service profile digests differ".into());
        }
        Ok(())
    }

    /// Proves that a target revision can consume every state lineage produced
    /// by its parent. A compatible rollback additionally proves that the parent
    /// can read every state version the target may write.
    pub fn validate_successor_of(&self, parent: &Self) -> Result<(), String> {
        self.validate()?;
        parent.validate()?;
        if self.digest == parent.digest {
            return Err("successor Durable Cell revision must change application intent".into());
        }
        if compatibility_date(&self.spec.compatibility_date)?
            < compatibility_date(&parent.spec.compatibility_date)?
        {
            return Err("Durable Cell compatibility date cannot regress".into());
        }
        for previous in &parent.spec.cell_classes {
            let index = self
                .spec
                .cell_classes
                .binary_search_by(|candidate| candidate.name.cmp(&previous.name))
                .map_err(|_| {
                    "Durable Cell classes cannot be removed without a future tombstone contract"
                        .to_owned()
                })?;
            let next = &self.spec.cell_classes[index];
            if next.state_schema.write_version < previous.state_schema.write_version
                || !next
                    .state_schema
                    .can_read(previous.state_schema.write_version)
            {
                return Err(
                    "Durable Cell successor cannot read or monotonically advance parent state"
                        .into(),
                );
            }
            if self.spec.rollback_policy == DurableCellRollbackPolicy::Compatible
                && !previous
                    .state_schema
                    .can_read(next.state_schema.write_version)
            {
                return Err(
                    "Durable Cell compatible rollback cannot read the successor state version"
                        .into(),
                );
            }
        }
        Ok(())
    }

    pub const fn spec(&self) -> &DurableCellApplicationDefinitionSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

impl DurableCellApplicationDefinitionSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.build_run_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.bundle_digest.as_str())? != self.bundle_digest
            || self.bundle_size_bytes == 0
            || self.bundle_size_bytes > MAX_BUNDLE_BYTES
            || Sha256Digest::parse(self.service_profile_digest.as_str())?
                != self.service_profile_digest
        {
            return Err("Durable Cell bundle or Service profile binding is invalid".into());
        }
        validate_main_module(&self.main_module)?;
        compatibility_date(&self.compatibility_date)?;
        if self.compatibility_flags.len() > 64
            || self
                .compatibility_flags
                .iter()
                .any(|flag| !valid_compatibility_flag(flag))
            || self
                .compatibility_flags
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("Durable Cell compatibility flags must be unique and ordered".into());
        }
        if self.cell_classes.is_empty()
            || self.cell_classes.len() > MAX_CELL_CLASSES
            || self
                .cell_classes
                .windows(2)
                .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err("Durable Cell classes must be non-empty, unique, and ordered".into());
        }
        for class in &self.cell_classes {
            class.validate()?;
        }
        Ok(())
    }
}

fn definition_document(spec: &DurableCellApplicationDefinitionSpec) -> Result<Document, String> {
    let mut root = BlockBuilder::new(DEFINITION_BLOCK)
        .attr("schema", string(DURABLE_CELL_APPLICATION_SCHEMA))
        .attr("build_run_id", string(&spec.build_run_id.to_string()))
        .attr("bundle_digest", string(spec.bundle_digest.as_str()))
        .attr("bundle_media_type", string(DURABLE_CELL_BUNDLE_MEDIA_TYPE))
        .attr(
            "bundle_size_bytes",
            acl_integer("bundle_size_bytes", spec.bundle_size_bytes)?,
        )
        .attr("main_module", string(&spec.main_module))
        .attr("compatibility_date", string(&spec.compatibility_date))
        .attr(
            "compatibility_flags",
            list(
                spec.compatibility_flags
                    .iter()
                    .map(|flag| string(flag))
                    .collect(),
            ),
        )
        .attr(
            "service_profile_digest",
            string(spec.service_profile_digest.as_str()),
        )
        .attr("rollback_policy", string(spec.rollback_policy.as_str()));
    for class in &spec.cell_classes {
        root = root.nested_block(
            BlockBuilder::new("cell_class")
                .label(&class.name)
                .attr(
                    "minimum_readable_state_version",
                    acl_integer(
                        "minimum_readable_state_version",
                        class.state_schema.minimum_readable_version,
                    )?,
                )
                .attr(
                    "maximum_readable_state_version",
                    acl_integer(
                        "maximum_readable_state_version",
                        class.state_schema.maximum_readable_version,
                    )?,
                )
                .attr(
                    "write_state_version",
                    acl_integer("write_state_version", class.state_schema.write_version)?,
                )
                .build(),
        );
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

fn parse_definition(document: &Document) -> Result<DurableCellApplicationDefinitionSpec, String> {
    if document.blocks.len() != 1 {
        return Err("Durable Cell application must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    if root.name != DEFINITION_BLOCK
        || !root.labels.is_empty()
        || root.attributes.len() != DEFINITION_ATTRIBUTES.len()
        || root
            .attributes
            .keys()
            .any(|key| !DEFINITION_ATTRIBUTES.contains(&key.as_str()))
        || root.blocks.iter().any(|block| block.name != "cell_class")
    {
        return Err("Durable Cell application block shape is invalid".into());
    }
    require_exact_string(root, "schema", DURABLE_CELL_APPLICATION_SCHEMA)?;
    require_exact_string(root, "bundle_media_type", DURABLE_CELL_BUNDLE_MEDIA_TYPE)?;
    let build_run_id = Uuid::parse_str(&required_string(root, "build_run_id")?)
        .map(BuildRunId::from_uuid)
        .map_err(|_| "Durable Cell build_run_id must be a UUID".to_owned())?;
    let mut cell_classes = Vec::with_capacity(root.blocks.len());
    for block in &root.blocks {
        exact_cell_class_block(block)?;
        cell_classes.push(DurableCellClassSpec {
            name: block.labels[0].clone(),
            state_schema: DurableCellStateSchema {
                minimum_readable_version: required_u64(block, "minimum_readable_state_version")?,
                maximum_readable_version: required_u64(block, "maximum_readable_state_version")?,
                write_version: required_u64(block, "write_state_version")?,
            },
        });
    }
    let spec = DurableCellApplicationDefinitionSpec {
        build_run_id,
        bundle_digest: Sha256Digest::parse(required_string(root, "bundle_digest")?)?,
        bundle_size_bytes: required_u64(root, "bundle_size_bytes")?,
        main_module: required_string(root, "main_module")?,
        compatibility_date: required_string(root, "compatibility_date")?,
        compatibility_flags: required_strings(root, "compatibility_flags")?,
        cell_classes,
        service_profile_digest: Sha256Digest::parse(required_string(
            root,
            "service_profile_digest",
        )?)?,
        rollback_policy: DurableCellRollbackPolicy::parse(&required_string(
            root,
            "rollback_policy",
        )?)?,
    };
    spec.validate()?;
    Ok(spec)
}

fn exact_cell_class_block(block: &Block) -> Result<(), String> {
    if block.name != "cell_class"
        || block.labels.len() != 1
        || !block.blocks.is_empty()
        || block.attributes.len() != CELL_CLASS_ATTRIBUTES.len()
        || block
            .attributes
            .keys()
            .any(|key| !CELL_CLASS_ATTRIBUTES.contains(&key.as_str()))
    {
        return Err("Durable Cell class block shape is invalid".into());
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Durable Cell application field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Durable Cell application field {name:?} must be a string"))
}

fn require_exact_string(block: &Block, name: &str, expected: &str) -> Result<(), String> {
    if required_string(block, name)? != expected {
        return Err(format!(
            "Durable Cell application field {name:?} must be exactly {expected:?}"
        ));
    }
    Ok(())
}

fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "Durable Cell application field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                format!("Durable Cell application field {name:?} must be a string list")
            })
        })
        .collect()
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("Durable Cell application field {name:?} must be an integer"))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value <= 0.0
        || value > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "Durable Cell application field {name:?} must be a positive exactly representable integer"
        ));
    }
    Ok(value as u64)
}

fn acl_integer(name: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "Durable Cell application field {name:?} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}

fn compatibility_date(value: &str) -> Result<NaiveDate, String> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "Durable Cell compatibility date must use YYYY-MM-DD".to_owned())?;
    if date.format("%Y-%m-%d").to_string() != value {
        return Err("Durable Cell compatibility date must be canonical".into());
    }
    Ok(date)
}

fn validate_main_module(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 512
        || value.starts_with(['/', '\\'])
        || value.contains(['\\', '?', '#', '\0'])
        || !(value.ends_with(".js") || value.ends_with(".mjs"))
        || value.split('/').any(|segment| {
            segment.is_empty()
                || matches!(segment, "." | "..")
                || segment
                    .chars()
                    .any(|character| character.is_control() || character.is_whitespace())
        })
    {
        return Err("Durable Cell main module must be one safe relative ESM path".into());
    }
    Ok(())
}

fn validate_cell_class_name(value: &str) -> Result<(), String> {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return Err("Durable Cell class name is invalid".into());
    };
    if value.len() > 128
        || !(first.is_ascii_alphabetic() || matches!(first, b'_' | b'$'))
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
    {
        return Err("Durable Cell class name must be one bounded ASCII identifier".into());
    }
    Ok(())
}

fn valid_compatibility_flag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::durable_cells::domain::DurableCellServiceProfileSpec;

    const APPLICATION_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/cell0.1/application.acl"
    ));
    const APPLICATION_FIXTURE_DIGEST: &str =
        "sha256:5c4047cc251bfde4f2c3ce2677347fdce91fe7199ecd4477e16ce21513c2ea87";
    const SERVICE_PROFILE_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/cell0.1/service-profile.acl"
    ));

    fn profile() -> DurableCellServiceProfile {
        DurableCellServiceProfile::from_spec(DurableCellServiceProfileSpec {
            public_runtime_port: "cell-public".into(),
            internal_runtime_port: "cell-internal".into(),
            health_path: "/__a3s/cell/health".into(),
            max_cell_name_bytes: 512,
            max_request_bytes: 16 * 1024 * 1024,
            max_response_bytes: 64 * 1024 * 1024,
            max_websocket_message_bytes: 1024 * 1024,
        })
        .expect("profile")
    }

    fn definition_spec(
        profile: &DurableCellServiceProfile,
    ) -> DurableCellApplicationDefinitionSpec {
        DurableCellApplicationDefinitionSpec {
            build_run_id: BuildRunId::new(),
            bundle_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                .expect("bundle digest"),
            bundle_size_bytes: 64 * 1024,
            main_module: "dist/worker.mjs".into(),
            compatibility_date: "2026-08-15".into(),
            compatibility_flags: vec!["nodejs-compat".into()],
            cell_classes: vec![DurableCellClassSpec {
                name: "Counter".into(),
                state_schema: DurableCellStateSchema {
                    minimum_readable_version: 1,
                    maximum_readable_version: 2,
                    write_version: 1,
                },
            }],
            service_profile_digest: profile.digest().clone(),
            rollback_policy: DurableCellRollbackPolicy::Compatible,
        }
    }

    #[test]
    fn canonical_acl_binds_build_profile_classes_and_migration_policy() {
        let profile = profile();
        let definition = DurableCellApplicationDefinition::from_spec(definition_spec(&profile))
            .expect("definition");
        assert_eq!(
            DurableCellApplicationDefinition::parse_acl(definition.canonical_acl())
                .expect("reparse"),
            definition
        );
        definition
            .validate_service_profile(&profile)
            .expect("profile binding");
        assert!(definition
            .canonical_acl()
            .contains("cell_class \"Counter\""));
        assert!(definition
            .canonical_acl()
            .contains(DURABLE_CELL_BUNDLE_MEDIA_TYPE));
    }

    #[test]
    fn shared_cell0_1_application_fixture_is_canonical_profile_bound_and_digest_locked() {
        let profile =
            DurableCellServiceProfile::parse_acl(SERVICE_PROFILE_FIXTURE).expect("profile");
        let definition =
            DurableCellApplicationDefinition::parse_acl(APPLICATION_FIXTURE).expect("application");
        definition
            .validate_service_profile(&profile)
            .expect("exact profile binding");
        assert_eq!(
            format!("{}\n", definition.canonical_acl()),
            APPLICATION_FIXTURE.replace("\r\n", "\n")
        );
        assert_eq!(definition.digest().as_str(), APPLICATION_FIXTURE_DIGEST);
    }

    #[test]
    fn parser_rejects_unknown_fields_noncanonical_storage_and_profile_drift() {
        let profile = profile();
        let definition = DurableCellApplicationDefinition::from_spec(definition_spec(&profile))
            .expect("definition");
        let unknown = definition.canonical_acl().replace(
            "main_module =",
            "provider_bucket = \"forbidden\"\n  main_module =",
        );
        assert!(DurableCellApplicationDefinition::parse_acl(&unknown).is_err());
        assert!(DurableCellApplicationDefinition::restore(
            &format!("\n{}", definition.canonical_acl()),
            definition.digest().as_str()
        )
        .is_err());

        let other_profile = DurableCellServiceProfile::from_spec(DurableCellServiceProfileSpec {
            max_request_bytes: 8 * 1024 * 1024,
            ..profile.spec().clone()
        })
        .expect("other profile");
        assert!(definition.validate_service_profile(&other_profile).is_err());
    }

    #[test]
    fn successor_requires_readable_monotonic_state_and_explicit_forward_only_rollout() {
        let profile = profile();
        let parent =
            DurableCellApplicationDefinition::from_spec(definition_spec(&profile)).expect("parent");
        let mut compatible_spec = definition_spec(&profile);
        compatible_spec.bundle_digest =
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("digest");
        compatible_spec.cell_classes[0].state_schema.write_version = 2;
        let compatible = DurableCellApplicationDefinition::from_spec(compatible_spec)
            .expect("compatible successor");
        compatible
            .validate_successor_of(&parent)
            .expect("rollback-compatible successor");

        let mut incompatible_spec = definition_spec(&profile);
        incompatible_spec.bundle_digest =
            Sha256Digest::parse(format!("sha256:{}", "c".repeat(64))).expect("digest");
        incompatible_spec.cell_classes[0].state_schema = DurableCellStateSchema {
            minimum_readable_version: 1,
            maximum_readable_version: 3,
            write_version: 3,
        };
        let incompatible = DurableCellApplicationDefinition::from_spec(incompatible_spec.clone())
            .expect("incompatible successor");
        assert!(incompatible.validate_successor_of(&parent).is_err());
        incompatible_spec.rollback_policy = DurableCellRollbackPolicy::ForwardOnly;
        DurableCellApplicationDefinition::from_spec(incompatible_spec)
            .expect("forward-only successor")
            .validate_successor_of(&parent)
            .expect("explicit forward-only rollout");
    }

    #[test]
    fn definition_rejects_ambiguous_paths_ordering_and_unbounded_bundle() {
        let profile = profile();
        let mut invalid = definition_spec(&profile);
        invalid.main_module = "../worker.mjs".into();
        assert!(DurableCellApplicationDefinition::from_spec(invalid).is_err());

        let mut invalid = definition_spec(&profile);
        invalid.compatibility_flags = vec!["z".into(), "a".into()];
        assert!(DurableCellApplicationDefinition::from_spec(invalid).is_err());

        let mut invalid = definition_spec(&profile);
        invalid.bundle_size_bytes = MAX_BUNDLE_BYTES + 1;
        assert!(DurableCellApplicationDefinition::from_spec(invalid).is_err());
    }
}
