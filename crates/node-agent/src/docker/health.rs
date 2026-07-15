use super::container::{container_id, container_is_running, now_ms};
use super::{docker_error, DockerRuntimeDriver};
use a3s_runtime::contract::{
    HealthProbe, RuntimeFailure, RuntimeHealthObservation, RuntimeHealthState, RuntimeObservation,
    RuntimeUnitSpec, RuntimeUnitState, TransportProtocol,
};
use a3s_runtime::{RuntimeError, RuntimeResult};
use bollard::container::StopContainerOptions;
use bollard::models::{ContainerInspectResponse, HealthStatusEnum};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

impl DockerRuntimeDriver {
    pub(super) async fn wait_for_task(
        &self,
        spec: &RuntimeUnitSpec,
        mut container: ContainerInspectResponse,
        provider_build: &str,
    ) -> RuntimeResult<RuntimeObservation> {
        let execution_timeout =
            Duration::from_millis(spec.resources.execution_timeout_ms.ok_or_else(|| {
                RuntimeError::InvalidRequest("Task has no execution timeout".into())
            })?);
        let started = Instant::now();
        loop {
            let observation = self.observation(spec, &container, provider_build, None)?;
            if observation.state.is_terminal() {
                return Ok(observation);
            }
            if started.elapsed() >= execution_timeout {
                let id = container_id(&container)?;
                let _ = self
                    .docker
                    .stop_container(&id, Some(StopContainerOptions { t: 1 }))
                    .await;
                container = self
                    .docker
                    .inspect_container(&id, None)
                    .await
                    .map_err(docker_error)?;
                let mut timed_out = self.observation(spec, &container, provider_build, None)?;
                timed_out.state = RuntimeUnitState::Failed;
                timed_out.observed_at_ms = now_ms();
                timed_out.finished_at_ms = Some(timed_out.observed_at_ms);
                timed_out.health = None;
                timed_out.outputs.clear();
                timed_out.failure = Some(RuntimeFailure {
                    code: "execution_timeout".into(),
                    message: "Task exceeded its execution timeout".into(),
                    retryable: false,
                });
                timed_out
                    .validate_against(spec)
                    .map_err(RuntimeError::Protocol)?;
                return Ok(timed_out);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            container = self
                .docker
                .inspect_container(&container_id(&container)?, None)
                .await
                .map_err(docker_error)?;
        }
    }

    pub(super) async fn wait_for_service(
        &self,
        spec: &RuntimeUnitSpec,
        mut container: ContainerInspectResponse,
        provider_build: &str,
    ) -> RuntimeResult<RuntimeObservation> {
        let Some(policy) = &spec.health else {
            return self.observation(spec, &container, provider_build, None);
        };
        if !container_is_running(&container) {
            return self.observation(spec, &container, provider_build, None);
        }
        if policy.start_period_ms > 0 {
            tokio::time::sleep(Duration::from_millis(policy.start_period_ms)).await;
        }
        let mut successes = 0_u32;
        let mut failures = 0_u32;
        loop {
            let health = self.probe_health(spec, &container).await?;
            match health.state {
                RuntimeHealthState::Healthy => {
                    successes = successes.saturating_add(1);
                    failures = 0;
                }
                RuntimeHealthState::Unhealthy => {
                    failures = failures.saturating_add(1);
                    successes = 0;
                }
                RuntimeHealthState::Unknown | RuntimeHealthState::Starting => {
                    successes = 0;
                }
            }
            if successes >= policy.success_threshold || failures >= policy.failure_threshold {
                return self.observation(spec, &container, provider_build, Some(health));
            }
            tokio::time::sleep(Duration::from_millis(policy.interval_ms)).await;
            container = self
                .docker
                .inspect_container(&container_id(&container)?, None)
                .await
                .map_err(docker_error)?;
            if !container_is_running(&container) {
                return self.observation(spec, &container, provider_build, None);
            }
        }
    }

    pub(super) async fn probe_health(
        &self,
        spec: &RuntimeUnitSpec,
        container: &ContainerInspectResponse,
    ) -> RuntimeResult<RuntimeHealthObservation> {
        let policy = spec.health.as_ref().ok_or_else(|| {
            RuntimeError::Protocol("health probe requested for a unit without health policy".into())
        })?;
        let (state, message) = match &policy.probe {
            HealthProbe::Http {
                port,
                path,
                expected_statuses,
            } => {
                let port = host_port(spec, container, port)?;
                let url = format!("http://127.0.0.1:{port}{path}");
                match self
                    .health_client
                    .get(url)
                    .timeout(Duration::from_millis(policy.timeout_ms))
                    .send()
                    .await
                {
                    Ok(response) if expected_statuses.contains(&response.status().as_u16()) => {
                        (RuntimeHealthState::Healthy, None)
                    }
                    Ok(response) => (
                        RuntimeHealthState::Unhealthy,
                        Some(format!("HTTP probe returned status {}", response.status())),
                    ),
                    Err(error) => (
                        RuntimeHealthState::Unhealthy,
                        Some(sanitize_probe_message(&error.to_string())),
                    ),
                }
            }
            HealthProbe::Tcp { port } => {
                let port = host_port(spec, container, port)?;
                match tokio::time::timeout(
                    Duration::from_millis(policy.timeout_ms),
                    TcpStream::connect(("127.0.0.1", port)),
                )
                .await
                {
                    Ok(Ok(stream)) => {
                        drop(stream);
                        (RuntimeHealthState::Healthy, None)
                    }
                    Ok(Err(error)) => (
                        RuntimeHealthState::Unhealthy,
                        Some(sanitize_probe_message(&error.to_string())),
                    ),
                    Err(_) => (
                        RuntimeHealthState::Unhealthy,
                        Some("TCP probe timed out".into()),
                    ),
                }
            }
            HealthProbe::Command { .. } => command_health(container),
        };
        Ok(RuntimeHealthObservation {
            state,
            checked_at_ms: now_ms(),
            message,
        })
    }
}

pub(super) fn host_port(
    spec: &RuntimeUnitSpec,
    container: &ContainerInspectResponse,
    name: &str,
) -> RuntimeResult<u16> {
    let port = spec
        .network
        .ports
        .iter()
        .find(|port| port.name == name)
        .ok_or_else(|| RuntimeError::Protocol(format!("health port {name:?} is missing")))?;
    let protocol = match port.protocol {
        TransportProtocol::Tcp => "tcp",
        TransportProtocol::Udp => "udp",
    };
    let key = format!("{}/{protocol}", port.container_port);
    let binding = container
        .network_settings
        .as_ref()
        .and_then(|settings| settings.ports.as_ref())
        .and_then(|ports| ports.get(&key))
        .and_then(Option::as_ref)
        .and_then(|bindings| bindings.first())
        .ok_or_else(|| {
            RuntimeError::ProviderUnavailable(format!(
                "Docker did not publish health port {name:?}"
            ))
        })?;
    binding
        .host_port
        .as_deref()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            RuntimeError::Protocol(format!("Docker health port {name:?} binding is invalid"))
        })
}

fn command_health(container: &ContainerInspectResponse) -> (RuntimeHealthState, Option<String>) {
    let health = container
        .state
        .as_ref()
        .and_then(|state| state.health.as_ref());
    let status = health.and_then(|health| health.status);
    let message = health
        .and_then(|health| health.log.as_ref())
        .and_then(|logs| logs.last())
        .and_then(|log| log.output.as_deref())
        .map(sanitize_probe_message);
    match status {
        Some(HealthStatusEnum::HEALTHY) => (RuntimeHealthState::Healthy, message),
        Some(HealthStatusEnum::UNHEALTHY) => (RuntimeHealthState::Unhealthy, message),
        Some(HealthStatusEnum::STARTING) => (RuntimeHealthState::Starting, message),
        Some(HealthStatusEnum::NONE | HealthStatusEnum::EMPTY) | None => {
            (RuntimeHealthState::Unknown, message)
        }
    }
}

fn sanitize_probe_message(message: &str) -> String {
    let value = message.replace(['\0', '\r', '\n'], " ");
    let value = value.trim();
    if value.is_empty() {
        "health probe failed".into()
    } else {
        value.chars().take(4096).collect()
    }
}
