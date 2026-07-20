use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, OperationId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::modules::workloads::{
    DeploymentStatus, IWorkloadRepository, PostgresWorkloadRepository,
};
use a3s_flow::WaitStatus;
use a3s_orm::PostgresExecutor;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const CRASH_PROBE_TEST: &str = "activation_before_retirement_crash_probe";
const CRASH_PROBE_PARENT_ENV: &str = "A3S_CLOUD_ACTIVATION_RETIREMENT_CRASH_PROBE";
const CRASH_PROBE_POSTGRES_ENV: &str = "A3S_CLOUD_ACTIVATION_RETIREMENT_CRASH_POSTGRES_URL";
const CRASH_PROBE_OPERATION_ENV: &str = "A3S_CLOUD_ACTIVATION_RETIREMENT_CRASH_OPERATION_ID";

pub async fn kill_after_activation_before_retirement(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    deployment_id: DeploymentId,
    operation_id: OperationId,
    target_revision_id: WorkloadRevisionId,
) -> TestResult {
    let lock_client = executor.pool().get().await?;
    lock_client
        .batch_execute("begin; lock table node_commands in access exclusive mode")
        .await?;

    let crash_result = run_parent_crash_probe(
        executor,
        postgres_url,
        organization_id,
        workload_id,
        deployment_id,
        operation_id,
        target_revision_id,
    )
    .await;
    let release_result = lock_client.batch_execute("rollback").await;
    match (crash_result, release_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(crash_error), Ok(())) => Err(crash_error),
        (Ok(()), Err(release_error)) => Err(release_error.into()),
        (Err(crash_error), Err(release_error)) => Err(format!(
            "activation crash gate failed: {crash_error}; node-command lock release also failed: {release_error}"
        )
        .into()),
    }
}

async fn run_parent_crash_probe(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    deployment_id: DeploymentId,
    operation_id: OperationId,
    target_revision_id: WorkloadRevisionId,
) -> TestResult {
    let workloads = PostgresWorkloadRepository::new(executor.clone());
    let before = workloads
        .find_deployment(organization_id, deployment_id)
        .await?;
    if before.status != DeploymentStatus::Applying || before.retirement_command_id.is_some() {
        return Err(
            "activation crash gate did not begin from an applying deployment without retirement"
                .into(),
        );
    }

    let mut crash_probe =
        CrashProbeProcess::start(&std::env::current_exe()?, postgres_url, operation_id)?;
    for _ in 0..600 {
        if let Some(status) = crash_probe.try_wait()? {
            return Err(format!(
                "activation crash probe exited before the durable boundary with {status}"
            )
            .into());
        }
        let deployment = workloads
            .find_deployment(organization_id, deployment_id)
            .await?;
        if deployment.status == DeploymentStatus::Retiring {
            if deployment.retirement_command_id.is_some() {
                return Err(
                    "activation crash probe dispatched retirement before process death".into(),
                );
            }
            let workload = workloads
                .find_workload(organization_id, workload_id)
                .await?;
            if workload.active_revision_id != Some(target_revision_id) {
                return Err(
                    "activation crash probe did not durably select the target revision".into(),
                );
            }
            let crash_status = crash_probe.kill_and_wait()?;
            require_sigkill(crash_status)?;
            return Ok(());
        }
        if deployment.status != DeploymentStatus::Applying
            && deployment.status != DeploymentStatus::Verifying
        {
            return Err(format!(
                "activation crash probe reached unexpected deployment state {}",
                deployment.status.as_str()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("activation crash probe did not reach the retiring boundary".into())
}

pub async fn run_activation_crash_probe() -> TestResult {
    if required_probe_environment(CRASH_PROBE_PARENT_ENV)? != "1" {
        return Err("activation crash probe requires its private marker".into());
    }
    let postgres_url = required_probe_environment(CRASH_PROBE_POSTGRES_ENV)?;
    let operation_id = OperationId::from_uuid(uuid::Uuid::parse_str(&required_probe_environment(
        CRASH_PROBE_OPERATION_ENV,
    )?)?);
    let executor = PostgresExecutor::connect_no_tls(&postgres_url, 2)?;
    let workloads = std::sync::Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let nodes = std::sync::Arc::new(
        a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository::new(executor),
    );
    let flow =
        crate::secret_rotation_restart_support::restart_flow(&postgres_url, workloads, nodes)
            .await?;
    let run_id = operation_id.to_string();
    let snapshot = flow.engine().snapshot(&run_id).await?;
    let waiting = snapshot
        .waits
        .values()
        .filter(|wait| wait.status == WaitStatus::Waiting)
        .collect::<Vec<_>>();
    if waiting.len() != 1 {
        return Err(format!(
            "activation crash probe expected one pending Flow wait, found {}",
            waiting.len()
        )
        .into());
    }
    flow.engine()
        .resume_wait(&run_id, &waiting[0].wait_id)
        .await?;
    Err("activation crash probe returned before process death".into())
}

struct CrashProbeProcess {
    child: Option<Child>,
}

impl CrashProbeProcess {
    fn start(
        test_binary: &std::path::Path,
        postgres_url: &str,
        operation_id: OperationId,
    ) -> std::io::Result<Self> {
        let child = Command::new(test_binary)
            .arg(CRASH_PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CRASH_PROBE_PARENT_ENV, "1")
            .env(CRASH_PROBE_POSTGRES_ENV, postgres_url)
            .env(CRASH_PROBE_OPERATION_ENV, operation_id.to_string())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("activation crash probe process disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("activation crash probe process disappeared"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        child.kill()?;
        child.wait()
    }
}

impl Drop for CrashProbeProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn require_sigkill(status: ExitStatus) -> TestResult {
    if status.success() {
        return Err("activation crash probe exited successfully".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(
                format!("activation crash probe exited with {status} instead of SIGKILL").into(),
            );
        }
    }
    Ok(())
}

fn required_probe_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("activation crash probe omitted {name}")))
}
