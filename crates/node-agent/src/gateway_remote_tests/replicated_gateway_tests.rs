use super::*;
use crate::{CommandExecutor, FileCommandJournal};
use a3s_cloud_contracts::{
    GatewayAckState, NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope,
    NodeCommandMetadata, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
    RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation,
    RuntimeRemoval,
};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use std::path::PathBuf;

struct UnusedRuntime;

fn unused_runtime() -> RuntimeError {
    RuntimeError::Protocol("replicated Gateway fixture does not use Runtime".into())
}

#[async_trait]
impl RuntimeClient for UnusedRuntime {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Err(unused_runtime())
    }

    async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        Err(unused_runtime())
    }

    async fn inspect(&self, _unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        Err(unused_runtime())
    }

    async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        Err(unused_runtime())
    }

    async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        Err(unused_runtime())
    }

    async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        Err(unused_runtime())
    }

    async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        Err(unused_runtime())
    }
}

struct ReplicatedGatewayMember {
    node_id: uuid::Uuid,
    traffic_port: u16,
    management_port: u16,
    managed_state_file: PathBuf,
    config_path: PathBuf,
    state_directory: PathBuf,
    certificate_root: PathBuf,
    signer: Arc<FixtureGatewayCertificateSigner>,
    ca_bundle_pem: String,
    snapshot: GatewaySnapshot,
    command: NodeCommandEnvelope,
    process: Option<GatewayProcess>,
}

impl ReplicatedGatewayMember {
    #[allow(clippy::too_many_arguments)]
    async fn start(
        binary: &str,
        root: &Path,
        ordinal: usize,
        traffic_port: u16,
        management_port: u16,
        upstream: SocketAddr,
        workload_id: uuid::Uuid,
        workload_revision_id: uuid::Uuid,
    ) -> TestResult<Self> {
        let member_directory = root.join(format!("member-{ordinal}"));
        tokio::fs::create_dir_all(&member_directory).await?;
        let node_id = uuid::Uuid::now_v7();
        let managed_state_file = member_directory.join("managed-snapshot.json");
        let config_path = member_directory.join("gateway.acl");
        tokio::fs::write(
            &config_path,
            management_gateway_acl(management_port, node_id, &managed_state_file),
        )
        .await?;
        let mut process = GatewayProcess::start(binary, &config_path)?;
        let base_url = format!("http://127.0.0.1:{management_port}/api/gateway");
        wait_for_gateway(&base_url, &mut process.child).await?;

        let certificate_root = member_directory.join("managed-certificates");
        let certificate_id = uuid::Uuid::now_v7();
        let certificate_directory = certificate_root.join(certificate_id.to_string());
        let certificate_request = GatewayCertificateRequest::new(
            certificate_id,
            vec![TLS_HOSTNAME.to_owned()],
            certificate_directory
                .join("certificate.pem")
                .to_string_lossy(),
            certificate_directory
                .join("private-key.pem")
                .to_string_lossy(),
        )?;
        let signer = Arc::new(FixtureGatewayCertificateSigner::new(
            node_id,
            vec![TLS_HOSTNAME.to_owned()],
        ));
        let ca_bundle_pem = signer.certificate_pem.clone();
        let issued_at = Utc::now();
        let snapshot = GatewaySnapshot::new_with_certificate(
            node_id,
            1,
            None,
            issued_at,
            issued_at + chrono::Duration::minutes(10),
            tls_gateway_acl(
                traffic_port,
                management_port,
                upstream,
                &certificate_request,
                node_id,
                &managed_state_file,
                &ManagedTargetFixture::new(
                    workload_revision_id,
                    format!("workload:{workload_id}:revision:{workload_revision_id}"),
                    1,
                ),
            ),
            Some(certificate_request),
        )?;
        let command = NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: uuid::Uuid::now_v7(),
                lease_id: uuid::Uuid::now_v7(),
                node_id,
                sequence: 1,
                aggregate_id: node_id,
                issued_at,
                not_after: issued_at + chrono::Duration::minutes(3),
                correlation_id: uuid::Uuid::now_v7(),
            },
            NodeCommandPayload::GatewaySnapshotInstall {
                snapshot: Box::new(snapshot.clone()),
            },
        )?;
        let state_directory = member_directory.join("node-state");
        tokio::fs::create_dir(&state_directory).await?;
        Ok(Self {
            node_id,
            traffic_port,
            management_port,
            managed_state_file,
            config_path,
            state_directory,
            certificate_root,
            signer,
            ca_bundle_pem,
            snapshot,
            command,
            process: Some(process),
        })
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/api/gateway", self.management_port)
    }

    fn installer(&self) -> TestResult<Arc<DurableGatewaySnapshotInstaller>> {
        let provisioner = Arc::new(NodeGatewayCertificateProvisioner::new(
            self.certificate_root.clone(),
            self.node_id,
            self.signer.clone(),
            Arc::new(SystemGatewayCertificateClock),
        )?);
        Ok(Arc::new(
            DurableGatewaySnapshotInstaller::new_with_certificates(
                self.node_id,
                gateway_control(&self.base_url())?,
                provisioner,
            ),
        ))
    }

    async fn execute(&self, command: NodeCommandEnvelope) -> TestResult<NodeCommandAck> {
        let executor = CommandExecutor::new(
            FileCommandJournal::new(self.state_directory.clone(), self.node_id)?,
            Arc::new(UnusedRuntime),
            self.installer()?,
        );
        let acknowledgement = executor.execute(command.clone()).await?;
        acknowledgement.validate_against(&command)?;
        validate_gateway_acknowledgement(&acknowledgement, &self.snapshot)?;
        Ok(acknowledgement)
    }

    fn process_mut(&mut self) -> TestResult<&mut GatewayProcess> {
        self.process
            .as_mut()
            .ok_or_else(|| "replicated Gateway member process is not running".into())
    }

    async fn https(&mut self) -> TestResult<String> {
        let client = managed_tls_client(&self.ca_bundle_pem, self.traffic_port)?;
        let url = format!("https://{TLS_HOSTNAME}:{}/fixture", self.traffic_port);
        let response = wait_for_https(&client, &url, &mut self.process_mut()?.child).await?;
        Ok(response.text().await?)
    }

    async fn restart(&mut self, binary: &str) -> TestResult {
        self.process.take();
        let mut process = GatewayProcess::start(binary, &self.config_path)?;
        wait_for_gateway(&self.base_url(), &mut process.child).await?;
        self.process = Some(process);
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires a dedicated remote Gateway runner"]
async fn installed_a3s_gateways_converge_independently_and_recover_member_loss() -> TestResult {
    let binary = required_gateway_binary()?;
    let directory = tempfile::tempdir()?;
    let upstream = LoopbackHttpUpstream::start("replicated-gateway-target").await?;
    let workload_id = uuid::Uuid::now_v7();
    let workload_revision_id = uuid::Uuid::now_v7();
    let ports = unused_replica_ports();
    let mut primary = ReplicatedGatewayMember::start(
        &binary,
        directory.path(),
        0,
        ports[0],
        ports[1],
        upstream.address,
        workload_id,
        workload_revision_id,
    )
    .await?;
    let mut secondary = ReplicatedGatewayMember::start(
        &binary,
        directory.path(),
        1,
        ports[2],
        ports[3],
        upstream.address,
        workload_id,
        workload_revision_id,
    )
    .await?;
    if primary.node_id == secondary.node_id
        || primary.command.command_id == secondary.command.command_id
        || primary.snapshot.snapshot_digest == secondary.snapshot.snapshot_digest
    {
        return Err("replicated Gateway members reused physical snapshot identity".into());
    }

    let primary_ack = primary.execute(primary.command.clone()).await?;
    let secondary_ack = secondary.execute(secondary.command.clone()).await?;
    if primary.signer.calls.load(Ordering::SeqCst) != 1
        || secondary.signer.calls.load(Ordering::SeqCst) != 1
    {
        return Err("replicated Gateway members did not issue one independent certificate".into());
    }
    if primary.https().await? != "replicated-gateway-target"
        || secondary.https().await? != "replicated-gateway-target"
    {
        return Err("replicated Gateway member returned an unexpected target".into());
    }
    let cross_member_client = managed_tls_client(&primary.ca_bundle_pem, secondary.traffic_port)?;
    let secondary_url = format!("https://{TLS_HOSTNAME}:{}/fixture", secondary.traffic_port);
    if cross_member_client.get(secondary_url).send().await.is_ok() {
        return Err("replicated Gateway members reused certificate authority identity".into());
    }

    secondary.process.take();
    if gateway_control(&secondary.base_url())?
        .readiness(&secondary.snapshot)
        .await
        .is_ok()
    {
        return Err("lost Gateway member remained reachable through management readiness".into());
    }
    if primary.https().await? != "replicated-gateway-target" {
        return Err("surviving Gateway member stopped serving after peer loss".into());
    }

    secondary.restart(&binary).await?;
    let recovered = gateway_control(&secondary.base_url())?
        .readiness(&secondary.snapshot)
        .await?;
    if recovered.state != ManagedSnapshotState::Applied || !recovered.ready {
        return Err("restarted Gateway member did not recover its exact physical snapshot".into());
    }
    if secondary.https().await? != "replicated-gateway-target" {
        return Err("restarted Gateway member recovered a stale target".into());
    }

    let mut replayed_primary_command = primary.command.clone();
    replayed_primary_command.lease_id = uuid::Uuid::now_v7();
    let replayed_primary = primary.execute(replayed_primary_command).await?;
    let mut replayed_secondary_command = secondary.command.clone();
    replayed_secondary_command.lease_id = uuid::Uuid::now_v7();
    let replayed_secondary = secondary.execute(replayed_secondary_command).await?;
    if replayed_primary.outcome != primary_ack.outcome
        || replayed_primary.completed_at != primary_ack.completed_at
        || replayed_secondary.outcome != secondary_ack.outcome
        || replayed_secondary.completed_at != secondary_ack.completed_at
        || primary.signer.calls.load(Ordering::SeqCst) != 1
        || secondary.signer.calls.load(Ordering::SeqCst) != 1
    {
        return Err("replicated Gateway replay changed a durable member outcome".into());
    }

    for (member, acknowledgement) in [
        (&primary, replayed_primary),
        (&secondary, replayed_secondary),
    ] {
        let journal = FileCommandJournal::new(member.state_directory.clone(), member.node_id)?;
        if journal.pending_acknowledgements().await? != vec![acknowledgement.clone()] {
            return Err("replicated Gateway member lost its pending Cloud acknowledgement".into());
        }
        let sequence = journal
            .mark_acknowledged(NodeCommandAckReceipt {
                schema: NodeCommandAckReceipt::SCHEMA.into(),
                command_id: acknowledgement.command_id,
                node_id: acknowledgement.node_id,
                replayed: false,
            })
            .await?;
        if sequence != 1 || journal.after_sequence().await? != 1 {
            return Err(
                "replicated Gateway member did not advance its exact durable cursor".into(),
            );
        }
    }
    if !tokio::fs::try_exists(&primary.managed_state_file).await?
        || !tokio::fs::try_exists(&secondary.managed_state_file).await?
    {
        return Err("replicated Gateway member omitted its native durable journal".into());
    }
    Ok(())
}

fn validate_gateway_acknowledgement(
    acknowledgement: &NodeCommandAck,
    snapshot: &GatewaySnapshot,
) -> TestResult {
    let gateway_acknowledgement = match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } => acknowledgement,
            _ => return Err("replicated Gateway command returned a Runtime result".into()),
        },
        _ => return Err("replicated Gateway command did not succeed".into()),
    };
    gateway_acknowledgement.validate_for(
        acknowledgement.command_id,
        acknowledgement.node_id,
        snapshot,
    )?;
    if gateway_acknowledgement.state != GatewayAckState::Applied || !gateway_acknowledgement.ready {
        return Err("replicated Gateway command omitted exact applied readiness".into());
    }
    Ok(())
}

fn unused_replica_ports() -> [u16; 4] {
    let listeners = (0..4)
        .map(|_| TcpListener::bind("127.0.0.1:0").expect("bind replicated Gateway port"))
        .collect::<Vec<_>>();
    let ports = std::array::from_fn(|index| {
        listeners[index]
            .local_addr()
            .expect("replicated Gateway address")
            .port()
    });
    drop(listeners);
    ports
}
