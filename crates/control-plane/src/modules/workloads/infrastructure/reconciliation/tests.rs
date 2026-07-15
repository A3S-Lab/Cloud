use super::*;
use crate::modules::fleet::domain::repositories::RuntimeObservationRecord;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError,
    ResourceName, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources,
    ServiceTemplate, Workload, WorkloadRevision,
};
use a3s_cloud_contracts::{NodeCommandFailure, NodeCommandResult};
use a3s_runtime::contract::{
    RuntimeHealthObservation, RuntimeHealthState, RuntimeObservation, RuntimeUnitClass,
};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use std::collections::{BTreeMap, HashMap};
use tokio::sync::RwLock;

struct FakeTargets {
    targets: RwLock<Vec<ActiveRuntimeTarget>>,
}

#[async_trait]
impl IWorkloadRuntimeTargetRepository for FakeTargets {
    async fn list_active_runtime_targets(
        &self,
        limit: usize,
    ) -> Result<Vec<ActiveRuntimeTarget>, RepositoryError> {
        Ok(self
            .targets
            .read()
            .await
            .iter()
            .take(limit)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
struct FakeControl {
    state: RwLock<FakeControlState>,
}

#[derive(Default)]
struct FakeControlState {
    sequence: u64,
    commands: BTreeMap<NodeCommandId, NodeCommand>,
    acknowledgements: HashMap<NodeCommandId, NodeCommandAck>,
    latest: Option<RuntimeObservationRecord>,
}

impl FakeControl {
    async fn commands(&self) -> Vec<NodeCommand> {
        let mut commands = self
            .state
            .read()
            .await
            .commands
            .values()
            .cloned()
            .collect::<Vec<_>>();
        commands.sort_by_key(|command| command.sequence);
        commands
    }

    async fn acknowledge(&self, command: &NodeCommand, outcome: NodeCommandOutcome) {
        self.state.write().await.acknowledgements.insert(
            command.id,
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.id.as_uuid(),
                lease_id: Uuid::now_v7(),
                node_id: command.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest().expect("command payload digest"),
                completed_at: Utc::now(),
                outcome,
            },
        );
    }

    async fn set_latest(&self, record: RuntimeObservationRecord) {
        self.state.write().await.latest = Some(record);
    }
}

#[async_trait]
impl IWorkloadRuntimeControl for FakeControl {
    async fn enqueue_command(
        &self,
        draft: NodeCommandDraft,
    ) -> Result<IdempotentWrite<NodeCommand>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(command) = state.commands.get(&draft.proposed_command_id) {
            return Ok(IdempotentWrite {
                value: command.clone(),
                replayed: true,
            });
        }
        state.sequence += 1;
        let command =
            NodeCommand::issue(draft, state.sequence).map_err(RepositoryError::Conflict)?;
        state.commands.insert(command.id, command.clone());
        Ok(IdempotentWrite {
            value: command,
            replayed: false,
        })
    }

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommand>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .commands
            .get(&command_id)
            .filter(|command| command.node_id == node_id)
            .cloned())
    }

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .acknowledgements
            .get(&command_id)
            .filter(|acknowledgement| acknowledgement.node_id == node_id.as_uuid())
            .cloned())
    }

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .latest
            .as_ref()
            .filter(|record| {
                record.node_id == node_id
                    && record.observation.unit_id == unit_id
                    && record.observation.generation == generation
            })
            .cloned())
    }
}

#[tokio::test]
async fn missing_provider_evidence_reapplies_the_same_revision_once_and_stop_removes_the_target(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let target = active_target(now - ChronoDuration::minutes(1))?;
    let spec = project_runtime_spec(&target.revision)?;
    let node_id = target.deployment.node_id.ok_or("target node")?;
    let targets = Arc::new(FakeTargets {
        targets: RwLock::new(vec![target.clone()]),
    });
    let control = Arc::new(FakeControl::default());
    control
        .set_latest(observation_record(
            node_id,
            Uuid::now_v7(),
            now - ChronoDuration::minutes(1),
            running_observation(&spec)?,
        ))
        .await;
    let reconciler = WorkloadRuntimeReconciler::new(
        targets.clone(),
        control.clone(),
        Duration::from_secs(10),
        Duration::from_secs(60),
        Duration::from_secs(30),
        100,
    )?;

    let first = reconciler.run_once(now).await?;
    assert_eq!(first.inspect_commands, 1);
    assert_eq!(first.recovery_commands, 0);
    let commands = control.commands().await;
    let inspect = commands.first().ok_or("inspect command")?.clone();
    assert!(matches!(
        inspect.payload,
        NodeCommandPayload::RuntimeInspect { .. }
    ));

    control
        .acknowledge(
            &inspect,
            NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeInspected {
                    inspection: RuntimeInspection::NotFound {
                        unit_id: spec.unit_id.clone(),
                        last_generation: Some(spec.generation),
                    },
                }),
            },
        )
        .await;
    let second = reconciler
        .run_once(now + ChronoDuration::seconds(1))
        .await?;
    assert_eq!(second.recovery_commands, 1);
    let commands = control.commands().await;
    assert_eq!(commands.len(), 2);
    let recovery = commands
        .iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeApply { .. }))
        .ok_or("recovery command")?
        .clone();
    let NodeCommandPayload::RuntimeApply { request } = &recovery.payload else {
        return Err("recovery command payload".into());
    };
    assert_eq!(request.spec.generation, target.revision.generation);
    assert_eq!(request.spec.artifact.digest, spec.artifact.digest);
    assert_eq!(request.spec.digest()?, spec.digest()?);

    let replay = reconciler
        .run_once(now + ChronoDuration::seconds(2))
        .await?;
    assert_eq!(replay.recovery_commands, 0);
    assert_eq!(replay.pending_commands, 1);
    assert_eq!(control.commands().await.len(), 2);

    let recovered = running_observation(&spec)?;
    control
        .acknowledge(
            &recovery,
            NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::RuntimeApplied {
                    observation: Box::new(recovered.clone()),
                }),
            },
        )
        .await;
    control
        .set_latest(observation_record(
            node_id,
            recovery.id.as_uuid(),
            now + ChronoDuration::seconds(3),
            recovered,
        ))
        .await;
    let converged = reconciler
        .run_once(now + ChronoDuration::seconds(4))
        .await?;
    assert_eq!(converged.converged, 1);
    assert_eq!(control.commands().await.len(), 2);

    targets.targets.write().await.clear();
    let stopped = reconciler
        .run_once(now + ChronoDuration::minutes(1))
        .await?;
    assert_eq!(stopped.targets, 0);
    assert_eq!(control.commands().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn retryable_inspect_failure_uses_a_new_deterministic_command(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let target = active_target(now - ChronoDuration::minutes(1))?;
    let spec = project_runtime_spec(&target.revision)?;
    let node_id = target.deployment.node_id.ok_or("target node")?;
    let targets = Arc::new(FakeTargets {
        targets: RwLock::new(vec![target]),
    });
    let control = Arc::new(FakeControl::default());
    control
        .set_latest(observation_record(
            node_id,
            Uuid::now_v7(),
            now - ChronoDuration::minutes(1),
            running_observation(&spec)?,
        ))
        .await;
    let reconciler = WorkloadRuntimeReconciler::new(
        targets,
        control.clone(),
        Duration::from_secs(10),
        Duration::from_secs(60),
        Duration::from_secs(30),
        100,
    )?;
    reconciler.run_once(now).await?;
    let first = control.commands().await.remove(0);
    control
        .acknowledge(
            &first,
            NodeCommandOutcome::Failed {
                failure: NodeCommandFailure {
                    code: "provider_unavailable".into(),
                    message: "provider temporarily unavailable".into(),
                    retryable: true,
                },
            },
        )
        .await;

    let report = reconciler
        .run_once(now + ChronoDuration::seconds(1))
        .await?;
    assert_eq!(report.inspect_commands, 1);
    let commands = control.commands().await;
    assert_eq!(commands.len(), 2);
    assert_ne!(commands[0].id, commands[1].id);
    assert_eq!(commands[1].sequence, commands[0].sequence + 1);
    Ok(())
}

fn active_target(at: DateTime<Utc>) -> Result<ActiveRuntimeTarget, String> {
    let organization_id = OrganizationId::new();
    let mut workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("reconciliation fixture")?,
        at,
    );
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        service_template(),
        at,
    )?;
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        at,
    );
    let node_id = NodeId::new();
    let command_id = NodeCommandId::new();
    deployment.resolve(at)?;
    deployment.schedule(node_id, at)?;
    deployment.dispatch(command_id, at)?;
    deployment.verify(at)?;
    deployment.activate(at)?;
    workload.activate(revision.id, at)?;
    Ok(ActiveRuntimeTarget {
        workload,
        revision,
        deployment,
    })
}

fn service_template() -> ServiceTemplate {
    let digest = format!("sha256:{}", "a".repeat(64));
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/cloud/reconcile@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/fixture".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
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
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 1_000,
        },
    }
}

fn running_observation(spec: &RuntimeUnitSpec) -> Result<RuntimeObservation, String> {
    let now_ms = u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| "test clock predates Unix epoch")?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec.digest()?,
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("provider/reconciliation-fixture".into()),
        provider_build: Some("test-runtime/1".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: RuntimeHealthState::Healthy,
            checked_at_ms: now_ms,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: None,
        provider_attestation: None,
        failure: None,
    };
    observation.validate_against(spec)?;
    Ok(observation)
}

fn observation_record(
    node_id: NodeId,
    report_id: Uuid,
    received_at: DateTime<Utc>,
    observation: RuntimeObservation,
) -> RuntimeObservationRecord {
    RuntimeObservationRecord {
        report_id,
        node_id,
        command_id: Some(NodeCommandId::from_uuid(report_id)),
        observed_at: received_at,
        received_at,
        observation,
    }
}
