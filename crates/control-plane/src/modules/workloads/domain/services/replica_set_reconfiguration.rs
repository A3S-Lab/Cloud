use crate::modules::workloads::domain::entities::{
    ManagedOwnerReference, Workload, WorkloadControl, WorkloadReplica, WorkloadReplicaLifecycle,
    WorkloadReplicaMember, WorkloadRevision,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaSetReconfiguration {
    pub control: WorkloadControl,
    pub replicas: Vec<WorkloadReplica>,
    pub members_to_create: Vec<WorkloadReplicaMember>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicaSetReconfigurationError {
    Conflict(String),
    Invariant(String),
}

impl ReplicaSetReconfigurationError {
    pub fn into_message(self) -> String {
        match self {
            Self::Conflict(message) | Self::Invariant(message) => message,
        }
    }
}

impl Display for ReplicaSetReconfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict(message) => write!(formatter, "replica-set conflict: {message}"),
            Self::Invariant(message) => write!(formatter, "replica-set invariant: {message}"),
        }
    }
}

impl Error for ReplicaSetReconfigurationError {}

#[allow(clippy::too_many_arguments)]
pub fn plan_replica_set_reconfiguration(
    workload: &Workload,
    mut control: WorkloadControl,
    revision: &WorkloadRevision,
    mut replicas: Vec<WorkloadReplica>,
    expected_control_version: u64,
    expected_policy_generation: u64,
    desired_replicas: u32,
    requested_owner: Option<&ManagedOwnerReference>,
    requested_at: DateTime<Utc>,
) -> Result<ReplicaSetReconfiguration, ReplicaSetReconfigurationError> {
    validate_current_set(workload, &control, revision, &mut replicas)?;
    if control.aggregate_version != expected_control_version {
        return Err(ReplicaSetReconfigurationError::Conflict(format!(
            "workload control changed from expected version {expected_control_version} to {}",
            control.aggregate_version
        )));
    }

    let previous_desired = control.spec.placement_policy.desired_replicas();
    control
        .reconfigure_replica_set(
            expected_policy_generation,
            desired_replicas,
            requested_owner,
            requested_at,
        )
        .map_err(ReplicaSetReconfigurationError::Conflict)?;

    let mut members_to_create = Vec::new();
    if desired_replicas < previous_desired {
        for ordinal in desired_replicas..previous_desired {
            let replica = replica_for_ordinal_mut(&mut replicas, ordinal).ok_or_else(|| {
                ReplicaSetReconfigurationError::Invariant(format!(
                    "desired Workload replica ordinal {ordinal} is missing"
                ))
            })?;
            replica
                .request_retirement(requested_at)
                .map_err(ReplicaSetReconfigurationError::Conflict)?;
        }
    } else {
        for ordinal in previous_desired..desired_replicas {
            match replica_for_ordinal_mut(&mut replicas, ordinal) {
                Some(replica) if replica.lifecycle == WorkloadReplicaLifecycle::Retired => {
                    replica
                        .reactivate(revision, requested_at)
                        .map_err(ReplicaSetReconfigurationError::Conflict)?;
                }
                Some(replica) if replica.lifecycle == WorkloadReplicaLifecycle::Retiring => {
                    return Err(ReplicaSetReconfigurationError::Conflict(format!(
                        "Workload replica ordinal {} is still retiring",
                        replica.ordinal
                    )));
                }
                Some(_) => {
                    return Err(ReplicaSetReconfigurationError::Invariant(format!(
                        "Workload replica ordinal {ordinal} is already desired outside the current policy"
                    )))
                }
                None => {
                    let replica = WorkloadReplica::for_ordinal(workload, revision, ordinal)
                        .map_err(ReplicaSetReconfigurationError::Invariant)?;
                    let member = WorkloadReplicaMember::for_replica(workload, &replica)
                        .map_err(ReplicaSetReconfigurationError::Invariant)?;
                    members_to_create.push(member);
                    replicas.push(replica);
                }
            }
        }
    }

    replicas.sort_by_key(|replica| (replica.ordinal, replica.id));
    members_to_create.sort_by_key(|member| (member.replica_id, member.ordinal, member.id));
    validate_desired_prefix(&replicas, desired_replicas)?;
    Ok(ReplicaSetReconfiguration {
        control,
        replicas,
        members_to_create,
    })
}

fn validate_current_set(
    workload: &Workload,
    control: &WorkloadControl,
    revision: &WorkloadRevision,
    replicas: &mut [WorkloadReplica],
) -> Result<(), ReplicaSetReconfigurationError> {
    control
        .validate_against(workload)
        .map_err(ReplicaSetReconfigurationError::Invariant)?;
    replicas.sort_by_key(|replica| (replica.ordinal, replica.id));
    if replicas.is_empty() {
        return Err(ReplicaSetReconfigurationError::Invariant(
            "Workload replica set is empty".into(),
        ));
    }

    let mut ids = BTreeSet::new();
    let mut ordinals = BTreeSet::new();
    for replica in replicas.iter() {
        replica
            .validate()
            .map_err(ReplicaSetReconfigurationError::Invariant)?;
        if replica.organization_id != workload.organization_id
            || replica.project_id != workload.project_id
            || replica.environment_id != workload.environment_id
            || replica.workload_id != workload.id
            || !ids.insert(replica.id)
            || !ordinals.insert(replica.ordinal)
        {
            return Err(ReplicaSetReconfigurationError::Invariant(
                "Workload replica set has inconsistent or duplicate identities".into(),
            ));
        }
    }

    let canonical = replicas
        .iter()
        .find(|replica| replica.ordinal == 0)
        .ok_or_else(|| {
            ReplicaSetReconfigurationError::Invariant(
                "Workload replica set is missing ordinal zero".into(),
            )
        })?;
    if revision.workload_id != workload.id
        || canonical.revision_id != revision.id
        || canonical.revision_generation != revision.generation
    {
        return Err(ReplicaSetReconfigurationError::Invariant(
            "Workload replica set has no canonical current revision".into(),
        ));
    }
    validate_desired_prefix(replicas, control.spec.placement_policy.desired_replicas())
}

fn validate_desired_prefix(
    replicas: &[WorkloadReplica],
    desired_replicas: u32,
) -> Result<(), ReplicaSetReconfigurationError> {
    for ordinal in 0..desired_replicas {
        if replicas
            .iter()
            .find(|replica| replica.ordinal == ordinal)
            .is_none_or(|replica| replica.lifecycle != WorkloadReplicaLifecycle::Desired)
        {
            return Err(ReplicaSetReconfigurationError::Invariant(format!(
                "Workload desired replica ordinal {ordinal} is missing"
            )));
        }
    }
    if replicas.iter().any(|replica| {
        replica.ordinal >= desired_replicas
            && replica.lifecycle == WorkloadReplicaLifecycle::Desired
    }) {
        return Err(ReplicaSetReconfigurationError::Invariant(
            "Workload desired replicas are not an ordinal prefix".into(),
        ));
    }
    Ok(())
}

fn replica_for_ordinal_mut(
    replicas: &mut [WorkloadReplica],
    ordinal: u32,
) -> Option<&mut WorkloadReplica> {
    replicas
        .iter_mut()
        .find(|replica| replica.ordinal == ordinal)
}
