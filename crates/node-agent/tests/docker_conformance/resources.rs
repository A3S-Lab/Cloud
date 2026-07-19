use super::fixture::{require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_runtime::contract::RuntimeUnitState;
use a3s_runtime::{RuntimeClient, RuntimeResult};
use std::time::{Duration, Instant};

const CPU_MILLIS: u64 = 333;
const MEMORY_BYTES: u64 = 48 * 1024 * 1024;
const PIDS: u32 = 17;

impl DockerConformanceFixture {
    pub(crate) async fn run_resources(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        self.verify_resource_configuration(client).await?;
        self.verify_workload_visible_controls(client).await?;
        self.verify_execution_timeout(client).await
    }

    async fn verify_resource_configuration(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let mut service = specs::service_spec(
            specs::unit_id(&self.namespace, "resources-config"),
            "exec sleep 300",
        );
        service.resources.cpu_millis = CPU_MILLIS;
        service.resources.memory_bytes = MEMORY_BYTES;
        service.resources.pids = PIDS;
        let observation = client
            .apply(&specs::apply("resources-config-apply", service.clone()))
            .await?;
        let inspection = self
            .docker_call(
                "inspect resource configuration",
                self.docker
                    .inspect_container(resource_id(&observation)?, None),
            )
            .await?;
        let host = inspection.host_config.ok_or_else(|| {
            a3s_runtime::RuntimeError::Protocol(
                "Docker resource inspection omitted HostConfig".into(),
            )
        })?;
        require(
            host.nano_cpus == Some((CPU_MILLIS * 1_000_000) as i64),
            "Docker CPU configuration does not match Runtime cpu_millis",
        )?;
        require(
            host.memory == Some(MEMORY_BYTES as i64)
                && host.memory_swap == Some(MEMORY_BYTES as i64),
            "Docker memory and swap configuration do not match Runtime memory_bytes",
        )?;
        require(
            host.pids_limit == Some(i64::from(PIDS)),
            "Docker PIDs configuration does not match Runtime pids",
        )?;
        client
            .remove(&specs::action("resources-config-remove", &service))
            .await?;
        Ok(())
    }

    async fn verify_workload_visible_controls(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let script = format!(
            "set -- $(cat /sys/fs/cgroup/cpu.max); test \"$1\" != max; quota=$1; period=$2; low=$((period * 32 / 100)); high=$((period * 34 / 100)); test \"$quota\" -ge \"$low\"; test \"$quota\" -le \"$high\"; test \"$(cat /sys/fs/cgroup/memory.max)\" = {MEMORY_BYTES}; test \"$(cat /sys/fs/cgroup/pids.max)\" = {PIDS}"
        );
        let mut task = specs::task_spec(
            specs::unit_id(&self.namespace, "resources-workload"),
            &script,
        );
        task.resources.cpu_millis = CPU_MILLIS;
        task.resources.memory_bytes = MEMORY_BYTES;
        task.resources.pids = PIDS;
        let observation = client
            .apply(&specs::apply("resources-workload-apply", task.clone()))
            .await?;
        require(
            observation.converges(&task),
            "workload could not observe enforced CPU, memory, and PIDs cgroup controls",
        )?;
        client
            .remove(&specs::action("resources-workload-remove", &task))
            .await?;
        Ok(())
    }

    async fn verify_execution_timeout(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let mut task = specs::task_spec(
            specs::unit_id(&self.namespace, "resources-timeout"),
            "exec sleep 5",
        );
        task.resources.execution_timeout_ms = Some(150);
        let started = Instant::now();
        let observation = client
            .apply(&specs::apply("resources-timeout-apply", task.clone()))
            .await?;
        let elapsed = started.elapsed();
        require(
            observation.state == RuntimeUnitState::Failed
                && observation
                    .failure
                    .as_ref()
                    .is_some_and(|failure| failure.code == "execution_timeout"),
            "Docker Task execution timeout did not return the bounded failure",
        )?;
        require(
            elapsed >= Duration::from_millis(100) && elapsed < Duration::from_secs(5),
            format!("Docker Task timeout completed outside its bound: {elapsed:?}"),
        )?;
        client
            .remove(&specs::action("resources-timeout-remove", &task))
            .await?;
        Ok(())
    }
}
