use super::{
    DurableCellApplication, DurableCellApplicationRevision, DurableCellProjectionIdentity,
    DurableCellServiceProfile,
};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::{ServiceTemplate, WorkloadRevision};
use serde::{Deserialize, Serialize};

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
        workload_revision: &WorkloadRevision,
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
        validate_workload_projection(
            application_revision,
            projection,
            service_profile,
            workload_revision,
        )?;

        let template = workload_revision.resolved_template()?;
        let binding = Self {
            application_id: application.id,
            application_revision_id: application_revision.id,
            application_revision_number: application_revision.revision_number,
            application_definition_digest: application_revision.definition.digest().clone(),
            workload_id: workload_revision.workload_id,
            workload_revision_id: workload_revision.id,
            workload_generation: workload_revision.generation,
            service_profile_digest: service_profile.digest().clone(),
            service_template_digest: Sha256Digest::parse(template.digest()?)?,
            provider_artifact_digest: Sha256Digest::parse(&template.artifact.digest)?,
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
        workload_revision: &WorkloadRevision,
    ) -> Result<Self, String> {
        self.validate()?;
        let expected = Self::for_current_revision(
            application,
            application_revision,
            projection,
            service_profile,
            workload_revision,
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

    pub fn validate_workload_revision(
        &self,
        service_profile: &DurableCellServiceProfile,
        workload_revision: &WorkloadRevision,
    ) -> Result<(), String> {
        self.validate()?;
        DurableCellServiceProfile::restore(
            service_profile.canonical_acl(),
            service_profile.digest().as_str(),
        )?;
        let template = workload_revision.resolved_template()?;
        if workload_revision.workload_id != self.workload_id
            || workload_revision.id != self.workload_revision_id
            || workload_revision.generation != self.workload_generation
            || service_profile.digest() != &self.service_profile_digest
            || Sha256Digest::parse(template.digest()?)? != self.service_template_digest
            || Sha256Digest::parse(&template.artifact.digest)? != self.provider_artifact_digest
        {
            return Err(
                "Durable Cell provider binding does not match the exact Workload revision".into(),
            );
        }
        validate_service_template(template, service_profile)
    }
}

fn validate_workload_projection(
    application_revision: &DurableCellApplicationRevision,
    projection: &DurableCellProjectionIdentity,
    service_profile: &DurableCellServiceProfile,
    workload_revision: &WorkloadRevision,
) -> Result<(), String> {
    if workload_revision.workload_id != projection.workload_id
        || workload_revision.id != projection.workload_revision_id
        || workload_revision.generation != application_revision.revision_number
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
    validate_service_template(workload_revision.resolved_template()?, service_profile)
}

fn validate_service_template(
    template: &ServiceTemplate,
    profile: &DurableCellServiceProfile,
) -> Result<(), String> {
    template.validate()?;
    DurableCellServiceProfile::restore(profile.canonical_acl(), profile.digest().as_str())?;
    let profile = profile.spec();
    if template.ports.len() != 2 {
        return Err(
            "Durable Cell provider Service must declare only its public and internal ports".into(),
        );
    }
    let public = template
        .ports
        .iter()
        .find(|port| port.name == profile.public_runtime_port)
        .ok_or_else(|| {
            "Durable Cell provider Service omitted its public Runtime port".to_owned()
        })?;
    let internal = template
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
    let health = template
        .health
        .as_ref()
        .ok_or_else(|| "Durable Cell provider Service requires an HTTP health check".to_owned())?;
    if health.port_name != profile.internal_runtime_port || health.path != profile.health_path {
        return Err(
            "Durable Cell provider health check must use the exact internal profile endpoint"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::durable_cells::domain::{
        DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
        DurableCellClassSpec, DurableCellRollbackPolicy, DurableCellServiceProfileSpec,
        DurableCellStateSchema,
    };
    use crate::modules::durable_cells::infrastructure::{
        admit_durable_cell_runtime_apply, project_durable_cell_runtime_spec,
    };
    use crate::modules::shared_kernel::domain::{
        BuildRunId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
        OrganizationId, PrincipalId, ProjectId, ResourceName,
    };
    use crate::modules::workloads::{
        HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources,
    };
    use a3s_cloud_contracts::{
        NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
        NodeCommandPayload, NodeCommandResult,
    };
    use a3s_runtime::contract::{
        RuntimeApplyRequest, RuntimeEvidence, RuntimeHealthObservation, RuntimeHealthState,
        RuntimeObservation, RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitSpec,
        RuntimeUnitState,
    };
    use chrono::{Duration, Utc};
    use std::collections::{BTreeMap, BTreeSet};
    use uuid::Uuid;

    struct Fixture {
        application: DurableCellApplication,
        application_revision: DurableCellApplicationRevision,
        projection: DurableCellProjectionIdentity,
        profile: DurableCellServiceProfile,
        workload_revision: WorkloadRevision,
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

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

    fn service_template(profile: &DurableCellServiceProfile) -> ServiceTemplate {
        let artifact_digest = digest('d');
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!(
                    "oci://registry.example/a3s/cell-provider@{}",
                    artifact_digest.as_str()
                ),
                digest: artifact_digest.to_string(),
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ServiceProcess {
                command: vec!["/app/a3s-cell-provider".into()],
                args: vec!["serve".into()],
                working_directory: Some("/app".into()),
                environment: BTreeMap::new(),
            },
            secrets: Vec::new(),
            resources: ServiceResources {
                cpu_millis: 500,
                memory_bytes: 256 * 1024 * 1024,
                pids: 128,
                ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
            },
            ports: vec![
                ServicePort {
                    name: profile.spec().public_runtime_port.clone(),
                    container_port: 8080,
                },
                ServicePort {
                    name: profile.spec().internal_runtime_port.clone(),
                    container_port: 9090,
                },
            ],
            health: Some(HttpHealthCheck {
                port_name: profile.spec().internal_runtime_port.clone(),
                path: profile.spec().health_path.clone(),
                interval_ms: 5_000,
                timeout_ms: 1_000,
                healthy_threshold: 1,
                unhealthy_threshold: 3,
                stabilization_window_ms: 10_000,
            }),
        }
    }

    fn fixture_with_template(
        make_template: impl FnOnce(&DurableCellServiceProfile) -> ServiceTemplate,
    ) -> Fixture {
        let profile = profile();
        let application_id = DurableCellApplicationId::new();
        let definition =
            DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
                build_run_id: BuildRunId::new(),
                bundle_digest: digest('a'),
                bundle_size_bytes: 4096,
                main_module: "worker.mjs".into(),
                compatibility_date: "2026-08-16".into(),
                compatibility_flags: Vec::new(),
                cell_classes: vec![DurableCellClassSpec {
                    name: "Counter".into(),
                    state_schema: DurableCellStateSchema {
                        minimum_readable_version: 1,
                        maximum_readable_version: 1,
                        write_version: 1,
                    },
                }],
                service_profile_digest: profile.digest().clone(),
                rollback_policy: DurableCellRollbackPolicy::Compatible,
            })
            .expect("definition");
        let application_revision = DurableCellApplicationRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition,
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("application revision");
        let application = DurableCellApplication::create(
            application_id,
            ResourceName::parse("Counters").expect("name"),
            &application_revision,
        )
        .expect("application");
        let projection = DurableCellProjectionIdentity::for_current_revision(
            &application,
            &application_revision,
        )
        .expect("projection");
        let workload_revision = WorkloadRevision::create(
            projection.workload_revision_id,
            projection.workload_id,
            application_revision.revision_number,
            make_template(&profile),
            application_revision.created_at,
        )
        .expect("Workload revision");
        Fixture {
            application,
            application_revision,
            projection,
            profile,
            workload_revision,
        }
    }

    fn fixture() -> Fixture {
        fixture_with_template(service_template)
    }

    fn binding(fixture: &Fixture) -> DurableCellProviderBinding {
        DurableCellProviderBinding::for_current_revision(
            &fixture.application,
            &fixture.application_revision,
            &fixture.projection,
            &fixture.profile,
            &fixture.workload_revision,
        )
        .expect("provider binding")
    }

    #[test]
    fn provider_selection_binds_one_existing_digest_pinned_workload_revision() {
        let fixture = fixture();
        let binding = binding(&fixture);
        assert_eq!(binding.application_id, fixture.application.id);
        assert_eq!(binding.workload_id, fixture.projection.workload_id);
        assert_eq!(
            binding.workload_revision_id,
            fixture.projection.workload_revision_id
        );
        assert_eq!(binding.service_profile_digest, *fixture.profile.digest());
        assert_eq!(
            binding.provider_artifact_digest.as_str(),
            fixture
                .workload_revision
                .resolved_template()
                .expect("template")
                .artifact
                .digest
        );
        binding
            .clone()
            .restore(
                &fixture.application,
                &fixture.application_revision,
                &fixture.projection,
                &fixture.profile,
                &fixture.workload_revision,
            )
            .expect("restored binding");

        let mut drifted = binding;
        drifted.provider_artifact_digest = digest('e');
        assert!(drifted
            .restore(
                &fixture.application,
                &fixture.application_revision,
                &fixture.projection,
                &fixture.profile,
                &fixture.workload_revision,
            )
            .is_err());
    }

    #[test]
    fn provider_template_rejects_extra_surface_shared_socket_or_public_health() {
        let extra = fixture_with_template(|profile| {
            let mut template = service_template(profile);
            template.ports.push(ServicePort {
                name: "debug".into(),
                container_port: 9191,
            });
            template
        });
        assert!(DurableCellProviderBinding::for_current_revision(
            &extra.application,
            &extra.application_revision,
            &extra.projection,
            &extra.profile,
            &extra.workload_revision,
        )
        .is_err());

        let shared = fixture_with_template(|profile| {
            let mut template = service_template(profile);
            template.ports[1].container_port = template.ports[0].container_port;
            template
        });
        assert!(DurableCellProviderBinding::for_current_revision(
            &shared.application,
            &shared.application_revision,
            &shared.projection,
            &shared.profile,
            &shared.workload_revision,
        )
        .is_err());

        let public_health = fixture_with_template(|profile| {
            let mut template = service_template(profile);
            template.health.as_mut().expect("health").port_name =
                profile.spec().public_runtime_port.clone();
            template
        });
        assert!(DurableCellProviderBinding::for_current_revision(
            &public_health.application,
            &public_health.application_revision,
            &public_health.projection,
            &public_health.profile,
            &public_health.workload_revision,
        )
        .is_err());
    }

    #[test]
    fn provider_projects_only_an_ordinary_profile_bound_runtime_service() {
        let fixture = fixture();
        let binding = binding(&fixture);
        let spec = project_durable_cell_runtime_spec(
            &binding,
            &fixture.profile,
            &fixture.workload_revision,
        )
        .expect("Runtime Service");
        assert_eq!(spec.class, RuntimeUnitClass::Service);
        assert_eq!(
            spec.semantics_profile_digest.as_deref(),
            Some(fixture.profile.digest().as_str())
        );
        assert_eq!(spec.network.ports.len(), 2);
        assert!(spec.outputs.is_empty());
        let names = spec
            .network
            .ports
            .iter()
            .map(|port| port.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(names, BTreeSet::from(["cell-internal", "cell-public"]));
    }

    #[test]
    fn runtime_admission_consumes_the_exact_existing_fleet_receipt() {
        let fixture = fixture();
        let binding = binding(&fixture);
        let spec = project_durable_cell_runtime_spec(
            &binding,
            &fixture.profile,
            &fixture.workload_revision,
        )
        .expect("Runtime Service");
        let observation = healthy_observation(&spec, RuntimeHealthState::Healthy);
        let (command, acknowledgement) = runtime_apply_receipt(spec, observation);
        let endpoints = admit_durable_cell_runtime_apply(
            &binding,
            &fixture.profile,
            &fixture.workload_revision,
            &command,
            &acknowledgement,
        )
        .expect("admitted Runtime receipt");
        assert_eq!(endpoints.public.port_name, "cell-public");
        assert_eq!(endpoints.internal.port_name, "cell-internal");
        assert_ne!(
            endpoints.public.socket_addr(),
            endpoints.internal.socket_addr()
        );

        let mut forged = acknowledgement.clone();
        forged.payload_digest = digest('f').to_string();
        assert!(admit_durable_cell_runtime_apply(
            &binding,
            &fixture.profile,
            &fixture.workload_revision,
            &command,
            &forged,
        )
        .is_err());

        let unhealthy = healthy_observation(
            match &command.payload {
                NodeCommandPayload::RuntimeApply { request, .. } => &request.spec,
                _ => unreachable!(),
            },
            RuntimeHealthState::Unhealthy,
        );
        let (unhealthy_command, unhealthy_ack) = runtime_apply_receipt(
            match &command.payload {
                NodeCommandPayload::RuntimeApply { request, .. } => request.spec.clone(),
                _ => unreachable!(),
            },
            unhealthy,
        );
        assert!(admit_durable_cell_runtime_apply(
            &binding,
            &fixture.profile,
            &fixture.workload_revision,
            &unhealthy_command,
            &unhealthy_ack,
        )
        .is_err());
    }

    fn healthy_observation(
        spec: &RuntimeUnitSpec,
        health_state: RuntimeHealthState,
    ) -> RuntimeObservation {
        let now_ms = u64::try_from(Utc::now().timestamp_millis()).expect("current timestamp");
        let spec_digest = spec.digest().expect("spec digest");
        let claims = spec
            .network
            .ports
            .iter()
            .enumerate()
            .map(|(index, port)| {
                let endpoint = RuntimeServiceEndpoint::node_local_tcp(
                    &port.name,
                    49_152 + u16::try_from(index).expect("port index"),
                )
                .expect("endpoint");
                (endpoint.claim_key(), endpoint.claim_value())
            })
            .collect();
        let observation = RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.into(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            spec_digest: spec_digest.clone(),
            class: RuntimeUnitClass::Service,
            state: RuntimeUnitState::Running,
            provider_resource_id: Some("cell-provider-fixture".into()),
            provider_build: Some("box-fixture".into()),
            observed_at_ms: now_ms,
            started_at_ms: Some(now_ms),
            finished_at_ms: None,
            health: Some(RuntimeHealthObservation {
                state: health_state,
                checked_at_ms: now_ms,
                message: None,
            }),
            outputs: Vec::new(),
            usage: None,
            evidence: Some(RuntimeEvidence {
                provider_build: "box-fixture".into(),
                spec_digest,
                semantics_profile_digest: spec.semantics_profile_digest.clone(),
                claims,
            }),
            provider_attestation: None,
            failure: None,
        };
        observation
            .validate_against(spec)
            .expect("Runtime observation");
        observation
    }

    fn runtime_apply_receipt(
        spec: RuntimeUnitSpec,
        observation: RuntimeObservation,
    ) -> (NodeCommandEnvelope, NodeCommandAck) {
        let issued_at = Utc::now();
        let command = NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: Uuid::now_v7(),
                lease_id: Uuid::now_v7(),
                node_id: Uuid::now_v7(),
                sequence: 1,
                aggregate_id: Uuid::now_v7(),
                issued_at,
                not_after: issued_at + Duration::minutes(1),
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("durable-cell-test:{}", Uuid::now_v7()),
                    deadline_at_ms: None,
                    spec,
                }),
                resource_claim: None,
            },
        )
        .expect("Fleet command");
        let acknowledgement = NodeCommandAck {
            schema: NodeCommandAck::SCHEMA.into(),
            command_id: command.command_id,
            lease_id: command.lease_id,
            node_id: command.node_id,
            sequence: command.sequence,
            payload_digest: command.payload_digest.clone(),
            completed_at: issued_at + Duration::seconds(1),
            outcome: NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeApplied {
                    observation: Box::new(observation),
                }),
            },
        };
        acknowledgement
            .validate_against(&command)
            .expect("Fleet acknowledgement");
        (command, acknowledgement)
    }
}
