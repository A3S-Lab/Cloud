use super::fixture::{require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_runtime::contract::{
    HealthProbe, NetworkMode, RuntimeHealthCheck, RuntimeHealthState, RuntimeNetworkSpec,
    RuntimePort, RuntimeUnitSpec, RuntimeUnitState, TransportProtocol,
};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use std::time::{Duration, Instant};

impl DockerConformanceFixture {
    pub(crate) async fn run_health(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        self.verify_http_probe(client).await?;
        self.verify_tcp_probe(client).await?;
        self.verify_command_transition(client).await?;
        self.verify_probe_timeout(client).await?;
        self.verify_start_period(client).await?;
        self.verify_unhealthy_exit(client).await
    }

    async fn verify_http_probe(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let service = health_service(
            &self.namespace,
            "health-http",
            "mkdir -p /www && printf 'healthy\\n' >/www/index.html && exec httpd -f -p 8080 -h /www",
            HealthProbe::Http {
                port: "health".into(),
                path: "/".into(),
                expected_statuses: vec![200],
            },
            2,
            5,
            100,
        );
        let observation = client
            .apply(&specs::apply("health-http-apply", service.clone()))
            .await?;
        require_healthy(&observation, "HTTP")?;
        client
            .remove(&specs::action("health-http-remove", &service))
            .await?;
        Ok(())
    }

    async fn verify_tcp_probe(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let service = health_service(
            &self.namespace,
            "health-tcp",
            "exec httpd -f -p 8080",
            HealthProbe::Tcp {
                port: "health".into(),
            },
            2,
            5,
            100,
        );
        let observation = client
            .apply(&specs::apply("health-tcp-apply", service.clone()))
            .await?;
        require_healthy(&observation, "TCP")?;
        client
            .remove(&specs::action("health-tcp-remove", &service))
            .await?;
        Ok(())
    }

    async fn verify_command_transition(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let service = health_service(
            &self.namespace,
            "health-command-transition",
            "exec sleep 300",
            HealthProbe::Command {
                command: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "n=$(cat /tmp/health-count 2>/dev/null || echo 0); n=$((n+1)); echo $n >/tmp/health-count; test $n -ge 3".into(),
                ],
            },
            2,
            20,
            100,
        );
        let observation = client
            .apply(&specs::apply(
                "health-command-transition-apply",
                service.clone(),
            ))
            .await?;
        require_healthy(&observation, "command")?;
        let inspection = self
            .docker_call(
                "inspect command health transition",
                self.docker
                    .inspect_container(resource_id(&observation)?, None),
            )
            .await?;
        let results = inspection
            .state
            .as_ref()
            .and_then(|state| state.health.as_ref())
            .and_then(|health| health.log.as_ref())
            .ok_or_else(|| {
                RuntimeError::Protocol("Docker command health has no result history".into())
            })?;
        require(
            results
                .iter()
                .any(|result| result.exit_code.is_some_and(|code| code != 0))
                && results.iter().any(|result| result.exit_code == Some(0)),
            "Docker command health did not record unhealthy-to-healthy transition",
        )?;
        client
            .remove(&specs::action("health-command-transition-remove", &service))
            .await?;
        Ok(())
    }

    async fn verify_probe_timeout(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let service = health_service(
            &self.namespace,
            "health-timeout",
            "mkdir -p /www/cgi-bin && printf '#!/bin/sh\\nsleep 2\\nprintf \\\'Content-Type: text/plain\\\\r\\\\n\\\\r\\\\nslow\\\\n\\\'\\n' >/www/cgi-bin/slow && chmod +x /www/cgi-bin/slow && exec httpd -f -p 8080 -h /www",
            HealthProbe::Http {
                port: "health".into(),
                path: "/cgi-bin/slow".into(),
                expected_statuses: vec![200],
            },
            1,
            2,
            100,
        );
        let observation = client
            .apply(&specs::apply("health-timeout-apply", service.clone()))
            .await?;
        require(
            observation.state == RuntimeUnitState::Running
                && observation
                    .health
                    .as_ref()
                    .is_some_and(|health| health.state == RuntimeHealthState::Unhealthy),
            "timed-out HTTP probe did not reach unhealthy threshold",
        )?;
        client
            .remove(&specs::action("health-timeout-remove", &service))
            .await?;
        Ok(())
    }

    async fn verify_start_period(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let service = health_service(
            &self.namespace,
            "health-start-period",
            "sleep 0.2; touch /tmp/ready; exec sleep 300",
            HealthProbe::Command {
                command: vec!["/bin/sh".into(), "-c".into(), "test -f /tmp/ready".into()],
            },
            1,
            2,
            500,
        );
        let started = Instant::now();
        let observation = client
            .apply(&specs::apply("health-start-period-apply", service.clone()))
            .await?;
        require_healthy(&observation, "start-period command")?;
        require(
            started.elapsed() >= Duration::from_millis(450),
            "Docker health start period was not honored before probing",
        )?;
        client
            .remove(&specs::action("health-start-period-remove", &service))
            .await?;
        Ok(())
    }

    async fn verify_unhealthy_exit(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let mut service = health_service(
            &self.namespace,
            "health-unhealthy-exit",
            "sleep 0.2; exit 17",
            HealthProbe::Command {
                command: vec!["/bin/sh".into(), "-c".into(), "exit 1".into()],
            },
            1,
            20,
            100,
        );
        service.restart = a3s_runtime::contract::RestartPolicy::Never;
        let observation = client
            .apply(&specs::apply(
                "health-unhealthy-exit-apply",
                service.clone(),
            ))
            .await?;
        require(
            observation.state == RuntimeUnitState::Failed,
            "Service exit during unhealthy probing did not become failed",
        )?;
        client
            .remove(&specs::action("health-unhealthy-exit-remove", &service))
            .await?;
        Ok(())
    }
}

fn health_service(
    namespace: &str,
    suffix: &str,
    script: &str,
    probe: HealthProbe,
    success_threshold: u32,
    failure_threshold: u32,
    start_period_ms: u64,
) -> RuntimeUnitSpec {
    let mut service = specs::service_spec(specs::unit_id(namespace, suffix), script);
    let needs_port = matches!(&probe, HealthProbe::Http { .. } | HealthProbe::Tcp { .. });
    if needs_port {
        service.network = RuntimeNetworkSpec {
            mode: NetworkMode::Service,
            ports: vec![RuntimePort {
                name: "health".into(),
                container_port: 8080,
                protocol: TransportProtocol::Tcp,
            }],
        };
    }
    service.health = Some(RuntimeHealthCheck {
        probe,
        interval_ms: 100,
        timeout_ms: 100,
        start_period_ms,
        success_threshold,
        failure_threshold,
    });
    service
}

fn require_healthy(
    observation: &a3s_runtime::contract::RuntimeObservation,
    probe: &str,
) -> RuntimeResult<()> {
    require(
        observation.state == RuntimeUnitState::Running
            && observation
                .health
                .as_ref()
                .is_some_and(|health| health.state == RuntimeHealthState::Healthy),
        format!("Docker {probe} health probe did not converge healthy"),
    )
}
