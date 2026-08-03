use super::{
    boolean, optional_integer, optional_string, string, validate_optional_block, ConfigError,
};
use a3s_acl::Block;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxRuntimeConfig {
    pub home_dir: PathBuf,
    pub secret_root: PathBuf,
    pub isolation: BoxRuntimeIsolation,
    pub control_timeout_ms: u64,
    pub task_poll_interval_ms: u64,
    pub sev_snp: Option<BoxRuntimeSevSnpConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxRuntimeIsolation {
    Microvm,
    Sandbox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxRuntimeSevSnpConfig {
    pub generation: BoxRuntimeSevSnpGeneration,
    pub simulate: bool,
    pub expected_measurement: Option<String>,
    pub require_no_debug: bool,
    pub require_no_smt: bool,
    pub allowed_policy_mask: Option<u64>,
    pub min_boot_loader_svn: Option<u8>,
    pub min_tee_svn: Option<u8>,
    pub min_snp_svn: Option<u8>,
    pub min_microcode_svn: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxRuntimeSevSnpGeneration {
    Milan,
    Genoa,
}

impl BoxRuntimeConfig {
    pub(crate) fn validate_sev_snp(&self) -> Result<(), ConfigError> {
        let Some(sev_snp) = &self.sev_snp else {
            return Ok(());
        };
        if self.isolation != BoxRuntimeIsolation::Microvm {
            return Err(ConfigError::Invalid(
                "box.sev_snp requires box.isolation = \"microvm\"".into(),
            ));
        }
        if !sev_snp.simulate && sev_snp.expected_measurement.is_none() {
            return Err(ConfigError::Invalid(
                "box.sev_snp.expected_measurement is required in hardware mode".into(),
            ));
        }
        if !sev_snp.simulate && !sev_snp.require_no_debug {
            return Err(ConfigError::Invalid(
                "box.sev_snp.require_no_debug must be true in hardware mode".into(),
            ));
        }
        if sev_snp
            .expected_measurement
            .as_ref()
            .is_some_and(|measurement| !canonical_sha384(measurement))
        {
            return Err(ConfigError::Invalid(
                "box.sev_snp.expected_measurement must be 96 lowercase hexadecimal characters"
                    .into(),
            ));
        }
        Ok(())
    }
}

pub(super) fn isolation(block: &Block) -> Result<BoxRuntimeIsolation, ConfigError> {
    match string(block, "isolation")?.as_str() {
        "microvm" => Ok(BoxRuntimeIsolation::Microvm),
        "sandbox" => Ok(BoxRuntimeIsolation::Sandbox),
        _ => Err(ConfigError::Invalid(
            "box.isolation must be either microvm or sandbox".into(),
        )),
    }
}

pub(super) fn sev_snp(block: &Block) -> Result<Option<BoxRuntimeSevSnpConfig>, ConfigError> {
    let Some(sev_snp) = block.blocks.iter().find(|nested| nested.name == "sev_snp") else {
        return Ok(None);
    };
    validate_optional_block(
        sev_snp,
        &[
            "generation",
            "simulate",
            "require_no_debug",
            "require_no_smt",
        ],
        &[
            "expected_measurement",
            "allowed_policy_mask",
            "min_boot_loader_svn",
            "min_tee_svn",
            "min_snp_svn",
            "min_microcode_svn",
        ],
    )?;
    let generation = match string(sev_snp, "generation")?.as_str() {
        "milan" => BoxRuntimeSevSnpGeneration::Milan,
        "genoa" => BoxRuntimeSevSnpGeneration::Genoa,
        _ => {
            return Err(ConfigError::Invalid(
                "box.sev_snp.generation must be either milan or genoa".into(),
            ))
        }
    };
    let allowed_policy_mask = optional_integer(sev_snp, "allowed_policy_mask")?;
    if allowed_policy_mask.is_some_and(|mask| mask > (1_u64 << f64::MANTISSA_DIGITS) - 1) {
        return Err(ConfigError::Invalid(
            "box.sev_snp.allowed_policy_mask must be exactly representable as an ACL integer"
                .into(),
        ));
    }
    Ok(Some(BoxRuntimeSevSnpConfig {
        generation,
        simulate: boolean(sev_snp, "simulate")?,
        expected_measurement: optional_string(sev_snp, "expected_measurement")?,
        require_no_debug: boolean(sev_snp, "require_no_debug")?,
        require_no_smt: boolean(sev_snp, "require_no_smt")?,
        allowed_policy_mask,
        min_boot_loader_svn: optional_integer(sev_snp, "min_boot_loader_svn")?,
        min_tee_svn: optional_integer(sev_snp, "min_tee_svn")?,
        min_snp_svn: optional_integer(sev_snp, "min_snp_svn")?,
        min_microcode_svn: optional_integer(sev_snp, "min_microcode_svn")?,
    }))
}

fn canonical_sha384(value: &str) -> bool {
    value.len() == 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
