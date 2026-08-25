use super::{
    DurableCellApplication, DurableCellApplicationRevision, DurableCellProjectionIdentity,
    DurableCellServiceProfile,
};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Consumer-owned, immutable view of the only Workload revision facts that
/// Durable Cells may admit. It deliberately excludes process, Secret,
/// placement, replica, command, and lifecycle authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellProviderWorkloadProjection {
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_generation: u64,
    pub service_template_digest: Sha256Digest,
    pub provider_artifact_digest: Sha256Digest,
    pub ports: Vec<DurableCellProviderPortProjection>,
    pub health: Option<DurableCellProviderHealthProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellProviderPortProjection {
    pub name: String,
    pub container_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellProviderHealthProjection {
    pub port_name: String,
    pub path: String,
}

impl DurableCellProviderWorkloadProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.workload_generation == 0
            || Sha256Digest::parse(self.service_template_digest.as_str())?
                != self.service_template_digest
            || Sha256Digest::parse(self.provider_artifact_digest.as_str())?
                != self.provider_artifact_digest
        {
            return Err("Durable Cell provider Workload projection identity is invalid".into());
        }
        let mut names = BTreeSet::new();
        for port in &self.ports {
            if port.name.trim().is_empty()
                || port.name.trim() != port.name
                || port.container_port == 0
                || !names.insert(port.name.as_str())
            {
                return Err("Durable Cell provider Workload port projection is invalid".into());
            }
        }
        if let Some(health) = &self.health {
            if health.port_name.trim().is_empty()
                || health.port_name.trim() != health.port_name
                || !health.path.starts_with('/')
                || health.path.chars().any(char::is_whitespace)
            {
                return Err("Durable Cell provider Workload health projection is invalid".into());
            }
        }
        Ok(())
    }
}

/// Immutable selection of one reviewed provider artifact through an existing
/// ordinary Workload Service revision.
///
/// This value does not own a Service template, Runtime unit, deployment,
/// provider configuration, command journal, endpoint registry, or lifecycle.
/// Those remain with Workloads, Runtime/Box, and Fleet. The repeated digests
/// are correlation fences for the exact records owned by those contexts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellProviderBinding {
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub application_revision_number: u64,
    pub application_definition_digest: Sha256Digest,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_generation: u64,
    pub service_profile_digest: Sha256Digest,
    pub service_template_digest: Sha256Digest,
    pub provider_artifact_digest: Sha256Digest,
}

impl DurableCellProviderBinding {
    pub fn for_current_revision(
        application: &DurableCellApplication,
        application_revision: &DurableCellApplicationRevision,
        projection: &DurableCellProjectionIdentity,
        service_profile: &DurableCellServiceProfile,
        workload: &DurableCellProviderWorkloadProjection,
    ) -> Result<Self, String> {
        application.validate()?;
        application_revision.validate()?;
        projection
            .clone()
            .restore(application, application_revision)
            .map(drop)?;
        application_revision
            .definition
            .validate_service_profile(service_profile)?;
        validate_workload_projection(application_revision, projection, service_profile, workload)?;

        let binding = Self {
            application_id: application.id,
            application_revision_id: application_revision.id,
            application_revision_number: application_revision.revision_number,
            application_definition_digest: application_revision.definition.digest().clone(),
            workload_id: workload.workload_id,
            workload_revision_id: workload.workload_revision_id,
            workload_generation: workload.workload_generation,
            service_profile_digest: service_profile.digest().clone(),
            service_template_digest: workload.service_template_digest.clone(),
            provider_artifact_digest: workload.provider_artifact_digest.clone(),
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn restore(
        self,
        application: &DurableCellApplication,
        application_revision: &DurableCellApplicationRevision,
        projection: &DurableCellProjectionIdentity,
        service_profile: &DurableCellServiceProfile,
        workload: &DurableCellProviderWorkloadProjection,
    ) -> Result<Self, String> {
        self.validate()?;
        let expected = Self::for_current_revision(
            application,
            application_revision,
            projection,
            service_profile,
            workload,
        )?;
        if self != expected {
            return Err("stored Durable Cell provider binding drifted".into());
        }
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.application_id.as_uuid().is_nil()
            || self.application_revision_id.as_uuid().is_nil()
            || self.application_revision_number == 0
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.workload_generation == 0
            || Sha256Digest::parse(self.application_definition_digest.as_str())?
                != self.application_definition_digest
            || Sha256Digest::parse(self.service_profile_digest.as_str())?
                != self.service_profile_digest
            || Sha256Digest::parse(self.service_template_digest.as_str())?
                != self.service_template_digest
            || Sha256Digest::parse(self.provider_artifact_digest.as_str())?
                != self.provider_artifact_digest
        {
            return Err("Durable Cell provider binding is invalid".into());
        }
        Ok(())
    }

    pub fn validate_workload_projection(
        &self,
        service_profile: &DurableCellServiceProfile,
        workload: &DurableCellProviderWorkloadProjection,
    ) -> Result<(), String> {
        self.validate()?;
        DurableCellServiceProfile::restore(
            service_profile.canonical_acl(),
            service_profile.digest().as_str(),
        )?;
        workload.validate()?;
        if workload.workload_id != self.workload_id
            || workload.workload_revision_id != self.workload_revision_id
            || workload.workload_generation != self.workload_generation
            || service_profile.digest() != &self.service_profile_digest
            || workload.service_template_digest != self.service_template_digest
            || workload.provider_artifact_digest != self.provider_artifact_digest
        {
            return Err(
                "Durable Cell provider binding does not match the exact Workload revision".into(),
            );
        }
        validate_service_projection(workload, service_profile)
    }
}

fn validate_workload_projection(
    application_revision: &DurableCellApplicationRevision,
    projection: &DurableCellProjectionIdentity,
    service_profile: &DurableCellServiceProfile,
    workload: &DurableCellProviderWorkloadProjection,
) -> Result<(), String> {
    workload.validate()?;
    if workload.workload_id != projection.workload_id
        || workload.workload_revision_id != projection.workload_revision_id
        || application_revision
            .definition
            .spec()
            .service_profile_digest
            != *service_profile.digest()
    {
        return Err(
            "Durable Cell provider Workload does not match the exact application projection".into(),
        );
    }
    // Application revisions can be admitted without immediately reaching a
    // deployable state. Workloads therefore owns its own contiguous
    // generation sequence; the deterministic revision identity and both
    // recorded generation numbers preserve the exact cross-owner mapping.
    validate_service_projection(workload, service_profile)
}

fn validate_service_projection(
    workload: &DurableCellProviderWorkloadProjection,
    profile: &DurableCellServiceProfile,
) -> Result<(), String> {
    workload.validate()?;
    DurableCellServiceProfile::restore(profile.canonical_acl(), profile.digest().as_str())?;
    let profile = profile.spec();
    if workload.ports.len() != 2 {
        return Err(
            "Durable Cell provider Service must declare only its public and internal ports".into(),
        );
    }
    let public = workload
        .ports
        .iter()
        .find(|port| port.name == profile.public_runtime_port)
        .ok_or_else(|| {
            "Durable Cell provider Service omitted its public Runtime port".to_owned()
        })?;
    let internal = workload
        .ports
        .iter()
        .find(|port| port.name == profile.internal_runtime_port)
        .ok_or_else(|| {
            "Durable Cell provider Service omitted its internal Runtime port".to_owned()
        })?;
    if public.container_port == internal.container_port {
        return Err(
            "Durable Cell public and internal Runtime ports must use distinct sockets".into(),
        );
    }
    let health = workload
        .health
        .as_ref()
        .ok_or_else(|| "Durable Cell provider Service requires an HTTP health check".to_owned())?;
    if health.port_name != profile.public_runtime_port || health.path != profile.health_path {
        return Err(
            "Durable Cell provider health check must use the exact public readiness endpoint"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "provider_binding/tests.rs"]
mod tests;
