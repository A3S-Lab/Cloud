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
    use a3s_cloud_node_agent::{build_box_runtime_client, BoxRuntimeConfig};
    #[cfg(target_os = "linux")]
    use a3s_runtime::contract::{
        ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
        RuntimeActionRequest, RuntimeApplyRequest, RuntimeInspection, RuntimeNetworkSpec,
        RuntimePort, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
    };
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
    async fn real_box_endpoint_compiles_to_live_gateway_origin(
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
            return Err(invalid("dedicated Box gate did not enable real-provider tests").into());
        }
        let home = PathBuf::from(std::env::var("A3S_HOME")?);
        if !home.is_absolute() {
            return Err(invalid("dedicated Box gate A3S_HOME is not absolute").into());
        }
        let runtime_state = tempfile::tempdir()?;
        let runtime = build_box_runtime_client(
            &BoxRuntimeConfig {
                home_dir: home.canonicalize()?,
                control_timeout_ms: 120_000,
                task_poll_interval_ms: 25,
            },
            runtime_state.path(),
        )?;
        let spec = real_box_service_spec()?;
        let apply = RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.into(),
            request_id: format!("cloud-box-endpoint-apply-{}", uuid::Uuid::now_v7()),
            deadline_at_ms: None,
            spec: spec.clone(),
        };
        let observation = runtime.apply(&apply).await?;
        let verification = async {
            if observation.state != RuntimeUnitState::Running {
                return Err(invalid("Box Service did not reach running state").into());
            }
            let endpoint =
                RuntimeServiceEndpoint::from_observation(&observation, "http").map_err(invalid)?;
            let origin = gateway_http_upstream(&endpoint).map_err(invalid)?;
            if origin.as_str() != format!("http://{}/", endpoint.socket_addr()) {
                return Err(invalid("Cloud did not compile the exact Runtime socket").into());
            }
            let RuntimeInspection::Found {
                observation: inspected,
                ..
            } = runtime.inspect(&spec.unit_id).await?
            else {
                return Err(invalid("running Box Service disappeared during inspection").into());
            };
            let inspected_endpoint =
                RuntimeServiceEndpoint::from_observation(&inspected, "http").map_err(invalid)?;
            if inspected_endpoint != endpoint {
                return Err(invalid("Box inspection rotated the active Service endpoint").into());
            }
            require_gateway_response(origin.as_str()).await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(endpoint.socket_addr())
        }
        .await;

        let removal = runtime
            .remove(&RuntimeActionRequest {
                schema: RuntimeActionRequest::SCHEMA.into(),
                request_id: format!("cloud-box-endpoint-remove-{}", uuid::Uuid::now_v7()),
                unit_id: spec.unit_id,
                generation: spec.generation,
                deadline_at_ms: None,
            })
            .await;
        let address = verification?;
        removal?;
        require_endpoint_closed(address).await?;
        eprintln!("A3S_CLOUD_BOX_SERVICE_ENDPOINT_GATEWAY_ORIGIN_CERTIFIED");
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
        let spec = RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.into(),
            unit_id: format!("cloud-box-endpoint-{}", uuid::Uuid::now_v7().simple()),
            generation: 1,
            class: RuntimeUnitClass::Service,
            artifact: ArtifactRef {
                uri: format!("oci://{image}"),
                digest: digest.into(),
                media_type: std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE")?,
            },
            process: RuntimeProcessSpec {
                command: vec!["/bin/sh".into(), "-c".into()],
                args: vec![
                    "while :; do { printf 'HTTP/1.1 200 OK\\r\\nContent-Length: 26\\r\\nConnection: close\\r\\n\\r\\ncloud-gateway-origin-ready'; } | nc -l -p 18080; done"
                        .into(),
                ],
                working_directory: Some("/".into()),
                environment: BTreeMap::new(),
            },
            mounts: Vec::new(),
            secrets: Vec::new(),
            network: RuntimeNetworkSpec {
                mode: NetworkMode::Service,
                ports: vec![RuntimePort {
                    name: "http".into(),
                    container_port: 18_080,
                    protocol: TransportProtocol::Tcp,
                }],
            },
            resources: ResourceLimits {
                cpu_millis: 500,
                memory_bytes: 128 * 1024 * 1024,
                pids: 64,
                ephemeral_storage_bytes: None,
                execution_timeout_ms: None,
            },
            isolation: IsolationLevel::Sandbox,
            health: None,
            restart: RestartPolicy::Always,
            outputs: Vec::new(),
            semantics_profile_digest: None,
        };
        spec.validate().map_err(invalid)?;
        Ok(spec)
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
