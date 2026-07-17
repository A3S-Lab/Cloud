use super::fixture::{require, resource_id, DockerConformanceFixture};
use super::specs::{self, BUSYBOX_DIGEST};
use a3s_runtime::contract::{NetworkMode, RuntimeNetworkSpec, RuntimePort, TransportProtocol};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions};
use bollard::models::HostConfig;
use std::collections::HashMap;
use std::time::Duration;
use tokio::net::UdpSocket;
use uuid::Uuid;

impl DockerConformanceFixture {
    pub(crate) async fn run_networking(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let target = self.create_network_target().await?;
        let target_ip = self.container_bridge_ip(&target).await?;
        self.verify_none_and_outbound_modes(client, &target_ip)
            .await?;
        self.verify_tcp_publication_and_collision(client).await?;
        self.verify_udp_publication(client).await
    }

    async fn create_network_target(&self) -> RuntimeResult<String> {
        let name = format!(
            "a3s-{}-network-target-{}",
            self.namespace,
            &Uuid::now_v7().simple().to_string()[..8]
        );
        let labels = HashMap::from([("a3s.cloud.namespace".to_owned(), self.namespace.clone())]);
        let created = self
            .docker_call(
                "create network policy target",
                self.docker.create_container(
                    Some(CreateContainerOptions {
                        name: name.as_str(),
                        platform: None,
                    }),
                    Config {
                        image: Some(format!("docker.io/library/busybox@{BUSYBOX_DIGEST}")),
                        cmd: Some(vec![
                            "/bin/sh".into(),
                            "-c".into(),
                            "mkdir -p /www && printf 'network-target\\n' >/www/index.html && exec httpd -f -p 8090 -h /www".into(),
                        ]),
                        labels: Some(labels),
                        host_config: Some(HostConfig {
                            network_mode: Some("bridge".into()),
                            privileged: Some(false),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
            )
            .await?;
        self.docker_call(
            "start network policy target",
            self.docker
                .start_container(&created.id, None::<StartContainerOptions<String>>),
        )
        .await?;
        Ok(created.id)
    }

    async fn container_bridge_ip(&self, id: &str) -> RuntimeResult<String> {
        for _ in 0..20 {
            let inspection = self
                .docker_call(
                    "inspect network policy target",
                    self.docker.inspect_container(id, None),
                )
                .await?;
            if let Some(ip) = inspection
                .network_settings
                .as_ref()
                .and_then(|settings| settings.networks.as_ref())
                .and_then(|networks| networks.get("bridge"))
                .and_then(|network| network.ip_address.as_ref())
                .filter(|ip| !ip.is_empty())
            {
                return Ok(ip.clone());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(RuntimeError::ProviderUnavailable(
            "Docker network policy target received no bridge address".into(),
        ))
    }

    async fn verify_none_and_outbound_modes(
        &self,
        client: &dyn RuntimeClient,
        target_ip: &str,
    ) -> RuntimeResult<()> {
        let denied_script = format!(
            "test ! -e /sys/class/net/eth0 && ! wget -T 1 -q -O /dev/null http://{target_ip}:8090/"
        );
        let denied = specs::task_spec(
            specs::unit_id(&self.namespace, "network-none"),
            &denied_script,
        );
        let denied_observation = client
            .apply(&specs::apply("network-none-apply", denied.clone()))
            .await?;
        require(
            denied_observation.converges(&denied),
            "NetworkMode::None did not deny outbound traffic",
        )?;
        let denied_container = self
            .docker_call(
                "inspect network-none container",
                self.docker
                    .inspect_container(resource_id(&denied_observation)?, None),
            )
            .await?;
        require(
            denied_container
                .host_config
                .as_ref()
                .and_then(|config| config.network_mode.as_deref())
                == Some("none"),
            "NetworkMode::None was not configured as Docker none",
        )?;
        client
            .remove(&specs::action("network-none-remove", &denied))
            .await?;

        let allowed_script = format!(
            "test -e /sys/class/net/eth0 && test \"$(wget -T 2 -q -O - http://{target_ip}:8090/)\" = network-target"
        );
        let mut allowed = specs::task_spec(
            specs::unit_id(&self.namespace, "network-outbound"),
            &allowed_script,
        );
        allowed.network.mode = NetworkMode::Outbound;
        let allowed_observation = client
            .apply(&specs::apply("network-outbound-apply", allowed.clone()))
            .await?;
        require(
            allowed_observation.converges(&allowed),
            "NetworkMode::Outbound could not reach an isolated fixture target",
        )?;
        let allowed_container = self
            .docker_call(
                "inspect network-outbound container",
                self.docker
                    .inspect_container(resource_id(&allowed_observation)?, None),
            )
            .await?;
        require(
            allowed_container
                .host_config
                .as_ref()
                .and_then(|config| config.network_mode.as_deref())
                == Some("bridge"),
            "NetworkMode::Outbound was not configured as Docker bridge",
        )?;
        client
            .remove(&specs::action("network-outbound-remove", &allowed))
            .await?;
        Ok(())
    }

    async fn verify_tcp_publication_and_collision(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let first = tcp_service(&self.namespace, "network-tcp-one", "tcp-one");
        let second = tcp_service(&self.namespace, "network-tcp-two", "tcp-two");
        let first_observation = client
            .apply(&specs::apply("network-tcp-one-apply", first.clone()))
            .await?;
        let second_observation = client
            .apply(&specs::apply("network-tcp-two-apply", second.clone()))
            .await?;
        let (first_ip, first_port) = self
            .published_port(resource_id(&first_observation)?, 8080, "tcp")
            .await?;
        let (second_ip, second_port) = self
            .published_port(resource_id(&second_observation)?, 8080, "tcp")
            .await?;
        require(
            first_ip == "127.0.0.1" && second_ip == "127.0.0.1",
            "Docker Service ports were not restricted to host loopback",
        )?;
        require(
            first_port != second_port,
            "two Docker Services collided on the same container TCP port",
        )?;
        self.wait_http(first_port, "tcp-one").await?;
        self.wait_http(second_port, "tcp-two").await?;
        client
            .remove(&specs::action("network-tcp-one-remove", &first))
            .await?;
        client
            .remove(&specs::action("network-tcp-two-remove", &second))
            .await?;
        Ok(())
    }

    async fn verify_udp_publication(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let mut service = specs::service_spec(
            specs::unit_id(&self.namespace, "network-udp"),
            "while true; do nc -u -l -p 8081 -e /bin/cat; done",
        );
        service.network = RuntimeNetworkSpec {
            mode: NetworkMode::Service,
            ports: vec![RuntimePort {
                name: "echo".into(),
                container_port: 8081,
                protocol: TransportProtocol::Udp,
            }],
        };
        let observation = client
            .apply(&specs::apply("network-udp-apply", service.clone()))
            .await?;
        let (host_ip, host_port) = self
            .published_port(resource_id(&observation)?, 8081, "udp")
            .await?;
        require(
            host_ip == "127.0.0.1",
            "Docker UDP Service port was not restricted to host loopback",
        )?;
        let socket = UdpSocket::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        socket
            .connect(("127.0.0.1", host_port))
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        let token = b"runtime-udp-probe";
        let mut response = [0_u8; 64];
        let mut received = None;
        for _ in 0..20 {
            socket
                .send(token)
                .await
                .map_err(|error| RuntimeError::Transport(error.to_string()))?;
            if let Ok(Ok(length)) =
                tokio::time::timeout(Duration::from_millis(250), socket.recv(&mut response)).await
            {
                received = Some(response[..length].to_vec());
                break;
            }
        }
        require(
            received.as_deref() == Some(token.as_slice()),
            "Docker UDP publication did not carry a datagram round trip",
        )?;
        client
            .remove(&specs::action("network-udp-remove", &service))
            .await?;
        Ok(())
    }

    async fn published_port(
        &self,
        container_id: &str,
        container_port: u16,
        protocol: &str,
    ) -> RuntimeResult<(String, u16)> {
        let inspection = self
            .docker_call(
                "inspect published Service port",
                self.docker.inspect_container(container_id, None),
            )
            .await?;
        let key = format!("{container_port}/{protocol}");
        let binding = inspection
            .network_settings
            .as_ref()
            .and_then(|settings| settings.ports.as_ref())
            .and_then(|ports| ports.get(&key))
            .and_then(Option::as_ref)
            .and_then(|bindings| bindings.first())
            .ok_or_else(|| RuntimeError::Protocol(format!("Docker did not publish {key}")))?;
        let host_ip = binding.host_ip.clone().ok_or_else(|| {
            RuntimeError::Protocol(format!("Docker {key} binding omitted host IP"))
        })?;
        let host_port = binding
            .host_port
            .as_deref()
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port > 0)
            .ok_or_else(|| {
                RuntimeError::Protocol(format!("Docker {key} binding omitted host port"))
            })?;
        Ok((host_ip, host_port))
    }

    async fn wait_http(&self, port: u16, expected: &str) -> RuntimeResult<()> {
        let url = format!("http://127.0.0.1:{port}/");
        for _ in 0..40 {
            if let Ok(response) = reqwest::get(&url).await {
                if response.status().is_success() {
                    if let Ok(body) = response.text().await {
                        if body == expected {
                            return Ok(());
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(RuntimeError::ProviderUnavailable(format!(
            "Docker published HTTP endpoint {url} did not become ready"
        )))
    }
}

fn tcp_service(
    namespace: &str,
    suffix: &str,
    body: &str,
) -> a3s_runtime::contract::RuntimeUnitSpec {
    let mut service = specs::service_spec(
        specs::unit_id(namespace, suffix),
        &format!(
            "mkdir -p /www && printf '{body}' >/www/index.html && exec httpd -f -p 8080 -h /www"
        ),
    );
    service.network = RuntimeNetworkSpec {
        mode: NetworkMode::Service,
        ports: vec![RuntimePort {
            name: "http".into(),
            container_port: 8080,
            protocol: TransportProtocol::Tcp,
        }],
    };
    service
}
