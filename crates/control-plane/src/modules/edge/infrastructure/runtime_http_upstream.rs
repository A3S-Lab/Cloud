use crate::modules::edge::domain::UpstreamEndpoint;
use a3s_runtime::contract::{RuntimeServiceEndpoint, TransportProtocol};

/// Compiles one provider-owned Runtime TCP socket into the HTTP origin consumed
/// by A3S Gateway. Runtime remains the endpoint authority; this adapter adds no
/// endpoint identity, evidence prefix, registry, or lifecycle state.
pub(super) fn gateway_http_upstream(
    endpoint: &RuntimeServiceEndpoint,
) -> Result<UpstreamEndpoint, String> {
    endpoint.validate()?;
    if endpoint.protocol != TransportProtocol::Tcp {
        return Err("Gateway routes require a Runtime TCP Service endpoint".into());
    }
    UpstreamEndpoint::parse(format!("http://{}", endpoint.socket_addr()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[cfg(target_os = "linux")]
    use crate::modules::shared_kernel::domain::{WorkloadId, WorkloadRevisionId};
    #[cfg(target_os = "linux")]
    use crate::modules::workloads::domain::entities::{
        HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources,
        ServiceTemplate, WorkloadRevision,
    };
    #[cfg(target_os = "linux")]
    use crate::modules::workloads::infrastructure::project_runtime_spec;
    #[cfg(target_os = "linux")]
    use a3s_cloud_contracts::{
        NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
        NodeCommandPayload, NodeCommandResult,
    };
    #[cfg(target_os = "linux")]
    use a3s_cloud_node_agent::{
        build_box_runtime_client, BoxRuntimeConfig, CommandExecutor, FileCommandJournal,
    };
    #[cfg(target_os = "linux")]
    use a3s_runtime::contract::{
        RuntimeActionRequest, RuntimeApplyRequest, RuntimeHealthState, RuntimeInspection,
        RuntimeObservation, RuntimeUnitSpec, RuntimeUnitState,
    };
    #[cfg(target_os = "linux")]
    use chrono::{Duration as ChronoDuration, Utc};
    #[cfg(target_os = "linux")]
    use std::collections::BTreeMap;
    #[cfg(target_os = "linux")]
    use std::error::Error;
    #[cfg(target_os = "linux")]
    use std::io;
    #[cfg(target_os = "linux")]
    use std::path::PathBuf;
    #[cfg(target_os = "linux")]
    use std::time::Duration;

    #[test]
    fn compiles_only_typed_runtime_tcp_endpoints() {
        let tcp = RuntimeServiceEndpoint::node_local_tcp("http", 49_152)
            .expect("valid Runtime TCP endpoint");
        assert_eq!(
            gateway_http_upstream(&tcp)
                .expect("Gateway HTTP upstream")
                .as_str(),
            "http://127.0.0.1:49152/"
        );

        let udp = RuntimeServiceEndpoint::new(
            "dns",
            TransportProtocol::Udp,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            5_353,
        )
        .expect("valid Runtime UDP endpoint");
        assert!(gateway_http_upstream(&udp).is_err());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "requires A3S_CLOUD_TEST_BOX=1 on the dedicated real Box provider runner"]
    // Box certifies every probe kind. Cloud's current Workload product surface
    // emits HTTP policy and consumes the same kind-neutral health observation
    // that TCP, command, and future profile compilers receive from Runtime.
    async fn real_box_health_replays_and_compiles_to_live_gateway_origin(
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
            return Err(invalid("dedicated Box gate did not enable real-provider tests").into());
        }
        let home = PathBuf::from(std::env::var("A3S_HOME")?);
        if !home.is_absolute() {
            return Err(invalid("dedicated Box gate A3S_HOME is not absolute").into());
        }
        let runtime_state = tempfile::tempdir()?;
        let node_state = tempfile::tempdir()?;
        let node_id = uuid::Uuid::now_v7();
        let aggregate_id = uuid::Uuid::now_v7();
        let journal = FileCommandJournal::new(node_state.path(), node_id)?;
        let runtime = build_box_runtime_client(
            &BoxRuntimeConfig {
                home_dir: home.canonicalize()?,
                control_timeout_ms: 120_000,
                task_poll_interval_ms: 25,
            },
            runtime_state.path(),
        )?;
        let spec = real_box_service_spec()?;
        let apply_command = command(
            node_id,
            aggregate_id,
            1,
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("cloud-box-health-apply-{}", uuid::Uuid::now_v7()),
                    deadline_at_ms: None,
                    spec: spec.clone(),
                }),
                resource_claim: None,
            },
        )?;
        let executor = CommandExecutor::runtime_only(journal, runtime.clone());
        let applied = executor.execute(apply_command.clone()).await?;
        let applied_runtime = applied_observation(&applied)?.clone();
        require_current_healthy_observation(&applied_runtime, "apply")?;
        let applied_endpoint =
            RuntimeServiceEndpoint::from_observation(&applied_runtime, "http").map_err(invalid)?;

        drop(executor);
        drop(runtime);
        // Box owns listeners in memory. Runtime's durable observation remains
        // byte-stable for command replay, while a reconstructed driver closes
        // the old listener and publishes a fresh endpoint on inspection. Cloud
        // must consume that fresh typed observation instead of treating the
        // replayed socket as a second endpoint registry.
        require_endpoint_closed(applied_endpoint.socket_addr()).await?;
        let recovered_runtime = build_box_runtime_client(
            &BoxRuntimeConfig {
                home_dir: home.canonicalize()?,
                control_timeout_ms: 120_000,
                task_poll_interval_ms: 25,
            },
            runtime_state.path(),
        )?;
        let recovered_journal = FileCommandJournal::new(node_state.path(), node_id)?;
        let recovered_executor =
            CommandExecutor::runtime_only(recovered_journal, recovered_runtime.clone());
        let mut replayed_command = apply_command;
        replayed_command.lease_id = uuid::Uuid::now_v7();
        let replayed = recovered_executor.execute(replayed_command).await?;
        if applied_observation(&replayed)? != &applied_runtime {
            return Err(invalid(
                "Node Agent journal replay changed the durable healthy Runtime observation",
            )
            .into());
        }

        let inspected = recovered_executor
            .execute(command(
                node_id,
                aggregate_id,
                2,
                NodeCommandPayload::RuntimeInspect {
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                },
            )?)
            .await?;
        let inspected_observation = inspected_observation(&inspected)?;
        require_current_healthy_observation(inspected_observation, "inspection")?;
        if inspected_observation.provider_resource_id != applied_runtime.provider_resource_id
            || inspected_observation.spec_digest != applied_runtime.spec_digest
        {
            return Err(invalid(
                "Node Agent inspection changed the active Runtime generation identity",
            )
            .into());
        }
        let inspected_endpoint =
            RuntimeServiceEndpoint::from_observation(inspected_observation, "http")
                .map_err(invalid)?;
        let inspected_origin = gateway_http_upstream(&inspected_endpoint).map_err(invalid)?;
        let applied_health = applied_runtime
            .health
            .as_ref()
            .ok_or_else(|| invalid("healthy apply observation omitted health"))?;
        let inspected_health = inspected_observation
            .health
            .as_ref()
            .ok_or_else(|| invalid("healthy inspection omitted health"))?;
        if inspected_health.checked_at_ms < applied_health.checked_at_ms {
            return Err(invalid("Runtime health inspection returned a stale sample").into());
        }

        let verification = async {
            if inspected_origin.as_str() != format!("http://{}/", inspected_endpoint.socket_addr())
            {
                return Err(invalid("Cloud did not compile the exact Runtime socket").into());
            }
            require_gateway_response(inspected_origin.as_str()).await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(inspected_endpoint.socket_addr())
        }
        .await;

        let removal = recovered_executor
            .execute(command(
                node_id,
                aggregate_id,
                3,
                NodeCommandPayload::RuntimeRemove {
                    request: RuntimeActionRequest {
                        schema: RuntimeActionRequest::SCHEMA.into(),
                        request_id: format!("cloud-box-health-remove-{}", uuid::Uuid::now_v7()),
                        unit_id: spec.unit_id.clone(),
                        generation: spec.generation,
                        deadline_at_ms: None,
                    },
                },
            )?)
            .await;
        let address = verification?;
        expect_removed(&removal?)?;
        let absent = recovered_executor
            .execute(command(
                node_id,
                aggregate_id,
                4,
                NodeCommandPayload::RuntimeInspect {
                    unit_id: spec.unit_id,
                    generation: spec.generation,
                },
            )?)
            .await?;
        expect_not_found(&absent)?;
        require_endpoint_closed(address).await?;
        eprintln!("A3S_CLOUD_BOX_SERVICE_ENDPOINT_GATEWAY_ORIGIN_CERTIFIED");
        eprintln!("A3S_CLOUD_BOX_RUNTIME_HEALTH_CONSUMER_CERTIFIED");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn real_box_service_spec() -> Result<RuntimeUnitSpec, Box<dyn Error + Send + Sync>> {
        let image = std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_IMAGE")?;
        let (repository, digest) = image
            .rsplit_once('@')
            .ok_or_else(|| invalid("Box conformance image is not digest-pinned"))?;
        if repository.is_empty()
            || !digest.starts_with("sha256:")
            || digest.len() != "sha256:".len() + 64
        {
            return Err(invalid("Box conformance image identity is invalid").into());
        }
        let revision = WorkloadRevision::create(
            WorkloadRevisionId::new(),
            WorkloadId::new(),
            1,
            ServiceTemplate {
                artifact: OciArtifact {
                    uri: format!("oci://{image}"),
                    digest: digest.into(),
                    media_type: std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE")?,
                },
                process: ServiceProcess {
                    command: vec!["/bin/sh".into(), "-c".into()],
                    args: vec![
                        "while :; do { printf 'HTTP/1.1 200 OK\\r\\nContent-Length: 26\\r\\nConnection: close\\r\\n\\r\\ncloud-gateway-origin-ready'; } | nc -l -p 18080; done"
                            .into(),
                    ],
                    working_directory: Some("/".into()),
                    environment: BTreeMap::new(),
                },
                secrets: Vec::new(),
                resources: ServiceResources {
                    cpu_millis: 500,
                    memory_bytes: 128 * 1024 * 1024,
                    pids: 64,
                    ephemeral_storage_bytes: None,
                },
                ports: vec![ServicePort {
                    name: "http".into(),
                    container_port: 18_080,
                }],
                health: Some(HttpHealthCheck {
                    port_name: "http".into(),
                    path: "/ready".into(),
                    interval_ms: 200,
                    timeout_ms: 150,
                    healthy_threshold: 2,
                    unhealthy_threshold: 3,
                    stabilization_window_ms: 100,
                }),
            },
            Utc::now(),
        )
        .map_err(invalid)?;
        project_runtime_spec(&revision).map_err(|error| invalid(error).into())
    }

    #[cfg(target_os = "linux")]
    fn command(
        node_id: uuid::Uuid,
        aggregate_id: uuid::Uuid,
        sequence: u64,
        payload: NodeCommandPayload,
    ) -> Result<NodeCommandEnvelope, Box<dyn Error + Send + Sync>> {
        let issued_at = Utc::now() - ChronoDuration::seconds(1);
        NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: uuid::Uuid::now_v7(),
                lease_id: uuid::Uuid::now_v7(),
                node_id,
                sequence,
                aggregate_id,
                issued_at,
                not_after: issued_at + ChronoDuration::minutes(30),
                correlation_id: uuid::Uuid::now_v7(),
            },
            payload,
        )
        .map_err(|error| invalid(error).into())
    }

    #[cfg(target_os = "linux")]
    fn applied_observation(
        acknowledgement: &NodeCommandAck,
    ) -> Result<&RuntimeObservation, Box<dyn Error + Send + Sync>> {
        match succeeded_result(acknowledgement)? {
            NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
            result => Err(invalid(format!("unexpected apply result: {result:?}")).into()),
        }
    }

    #[cfg(target_os = "linux")]
    fn inspected_observation(
        acknowledgement: &NodeCommandAck,
    ) -> Result<&RuntimeObservation, Box<dyn Error + Send + Sync>> {
        match succeeded_result(acknowledgement)? {
            NodeCommandResult::RuntimeInspected {
                inspection: RuntimeInspection::Found { observation, .. },
            } => Ok(observation),
            result => Err(invalid(format!("unexpected inspect result: {result:?}")).into()),
        }
    }

    #[cfg(target_os = "linux")]
    fn require_current_healthy_observation(
        observation: &RuntimeObservation,
        operation: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let Some(health) = observation.health.as_ref() else {
            return Err(invalid(format!("{operation} omitted Runtime health")).into());
        };
        if observation.state != RuntimeUnitState::Running
            || health.state != RuntimeHealthState::Healthy
            || health.message.is_some()
            || observation
                .started_at_ms
                .is_none_or(|started| health.checked_at_ms < started)
            || health.checked_at_ms > observation.observed_at_ms
        {
            return Err(invalid(format!(
                "{operation} did not return a current healthy Runtime observation"
            ))
            .into());
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn expect_removed(
        acknowledgement: &NodeCommandAck,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        match succeeded_result(acknowledgement)? {
            NodeCommandResult::RuntimeRemoved { removal } if !removal.unit_id.is_empty() => Ok(()),
            result => Err(invalid(format!("unexpected remove result: {result:?}")).into()),
        }
    }

    #[cfg(target_os = "linux")]
    fn expect_not_found(
        acknowledgement: &NodeCommandAck,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        match succeeded_result(acknowledgement)? {
            NodeCommandResult::RuntimeInspected {
                inspection: RuntimeInspection::NotFound { .. },
            } => Ok(()),
            result => Err(invalid(format!("unexpected post-remove result: {result:?}")).into()),
        }
    }

    #[cfg(target_os = "linux")]
    fn succeeded_result(
        acknowledgement: &NodeCommandAck,
    ) -> Result<&NodeCommandResult, Box<dyn Error + Send + Sync>> {
        match &acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => Ok(result),
            outcome => Err(invalid(format!("Cloud command did not succeed: {outcome:?}")).into()),
        }
    }

    #[cfg(target_os = "linux")]
    async fn require_gateway_response(origin: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(500))
            .build()?;
        let mut last_failure = "no request completed".to_owned();
        for _ in 0..30 {
            match client.get(origin).send().await {
                Ok(response) if response.status().is_success() => match response.text().await {
                    Ok(body) if body == "cloud-gateway-origin-ready" => return Ok(()),
                    Ok(body) => last_failure = format!("unexpected response body {body:?}"),
                    Err(error) => last_failure = error.to_string(),
                },
                Ok(response) => last_failure = format!("HTTP status {}", response.status()),
                Err(error) => last_failure = error.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(invalid(format!(
            "Gateway origin did not reach the Box Service: {last_failure}"
        ))
        .into())
    }

    #[cfg(target_os = "linux")]
    async fn require_endpoint_closed(
        address: std::net::SocketAddr,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        for _ in 0..50 {
            match tokio::time::timeout(
                Duration::from_millis(100),
                tokio::net::TcpStream::connect(address),
            )
            .await
            {
                Ok(Err(_)) => return Ok(()),
                Ok(Ok(stream)) => drop(stream),
                Err(_) => {}
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(invalid(format!(
            "Runtime Service endpoint {address} remained open after removal"
        ))
        .into())
    }

    #[cfg(target_os = "linux")]
    fn invalid(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.into())
    }
}
