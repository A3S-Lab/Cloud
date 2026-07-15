use crate::modules::workloads::domain::entities::WorkloadRevision;
use a3s_runtime::contract::{
    ArtifactRef, HealthProbe, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
    RuntimeHealthCheck, RuntimeNetworkSpec, RuntimePort, RuntimeProcessSpec, RuntimeUnitClass,
    RuntimeUnitSpec, TransportProtocol,
};

pub fn project_runtime_spec(revision: &WorkloadRevision) -> Result<RuntimeUnitSpec, String> {
    let template = revision.resolved_template()?;
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: format!("workload:{}", revision.workload_id),
        generation: revision.generation,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: template.artifact.uri.clone(),
            digest: template.artifact.digest.clone(),
            media_type: template.artifact.media_type.clone(),
        },
        process: RuntimeProcessSpec {
            command: template.process.command.clone(),
            args: template.process.args.clone(),
            working_directory: template.process.working_directory.clone(),
            environment: template.process.environment.clone(),
        },
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::Service,
            ports: template
                .ports
                .iter()
                .map(|port| RuntimePort {
                    name: port.name.clone(),
                    container_port: port.container_port,
                    protocol: TransportProtocol::Tcp,
                })
                .collect(),
        },
        resources: ResourceLimits {
            cpu_millis: template.resources.cpu_millis,
            memory_bytes: template.resources.memory_bytes,
            pids: template.resources.pids,
            ephemeral_storage_bytes: template.resources.ephemeral_storage_bytes,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Container,
        health: Some(RuntimeHealthCheck {
            probe: HealthProbe::Http {
                port: template.health.port_name.clone(),
                path: template.health.path.clone(),
                expected_statuses: vec![200],
            },
            interval_ms: template.health.interval_ms,
            timeout_ms: template.health.timeout_ms,
            start_period_ms: template.health.stabilization_window_ms,
            success_threshold: u32::from(template.health.healthy_threshold),
            failure_threshold: u32::from(template.health.unhealthy_threshold),
        }),
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
    };
    spec.validate()?;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{WorkloadId, WorkloadRevisionId};
    use crate::modules::workloads::domain::entities::{
        HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources,
        ServiceTemplate,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;

    #[test]
    fn projects_digest_bound_service_without_provider_fields() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let revision = WorkloadRevision::create(
            WorkloadRevisionId::new(),
            WorkloadId::new(),
            3,
            ServiceTemplate {
                artifact: OciArtifact {
                    uri: format!("oci://registry.example/fixture@{digest}"),
                    digest: digest.clone(),
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ServiceProcess {
                    command: Vec::new(),
                    args: vec!["serve".into()],
                    working_directory: None,
                    environment: BTreeMap::new(),
                },
                resources: ServiceResources {
                    cpu_millis: 250,
                    memory_bytes: 64 * 1024 * 1024,
                    pids: 64,
                    ephemeral_storage_bytes: None,
                },
                ports: vec![ServicePort {
                    name: "http".into(),
                    container_port: 8080,
                }],
                health: HttpHealthCheck {
                    port_name: "http".into(),
                    path: "/health".into(),
                    interval_ms: 1_000,
                    timeout_ms: 500,
                    healthy_threshold: 2,
                    unhealthy_threshold: 3,
                    stabilization_window_ms: 5_000,
                },
            },
            Utc::now(),
        )
        .expect("revision");
        let spec = project_runtime_spec(&revision).expect("Runtime spec");
        assert_eq!(spec.generation, 3);
        assert_eq!(spec.artifact.digest, digest);
        assert_eq!(spec.class, RuntimeUnitClass::Service);
        assert!(spec.health.is_some());
        assert!(spec.secrets.is_empty());
        assert!(spec.mounts.is_empty());
    }
}
