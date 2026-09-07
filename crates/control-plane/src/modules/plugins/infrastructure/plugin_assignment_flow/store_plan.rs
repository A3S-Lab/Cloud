use super::types::{StorePlanInput, StorePlanOutput, StoredPlan};
use super::PluginAssignmentFlowRuntime;
use crate::modules::plugins::domain::entities::{NewPluginPlanProjection, PluginPlanProjection};
use crate::modules::shared_kernel::domain::{
    PluginPlanProjectionId, RepositoryError, Sha256Digest,
};
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandResult};
use a3s_flow::FlowError;
use chrono::Utc;
use uuid::Uuid;

/// Deterministic namespace for plan projection IDs derived from the assignment Operation.
const STORE_PLAN_PROJECTION_NS: Uuid = Uuid::from_bytes([
    0xa3, 0x50, 0x1u8, 0x73, 0x74, 0x6f, 0x72, 0x65, 0x2d, 0x70, 0x6c, 0x61, 0x6e, 0x00, 0x00,
    0x01,
]);

pub(super) fn store_plan_projection_id(operation_id: Uuid) -> PluginPlanProjectionId {
    PluginPlanProjectionId::from_uuid(Uuid::new_v5(
        &STORE_PLAN_PROJECTION_NS,
        operation_id.as_bytes(),
    ))
}

pub(super) async fn store_plan(
    runtime: &PluginAssignmentFlowRuntime,
    input: StorePlanInput,
) -> a3s_flow::Result<StorePlanOutput> {
    let planned = *input.planned;
    let locked = planned.authorized.resolved.locked.as_ref();
    let assignment = runtime
        .assignments
        .find(locked.organization_id, locked.assignment_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "plugin assignment plan store reload failed: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment disappeared before plan store".into())
        })?;
    if assignment.organization_id != locked.organization_id
        || assignment.id != locked.assignment_id
        || assignment.current_operation_id != Some(locked.operation_id)
        || assignment.assignment_generation != locked.assignment_generation
        || assignment.registry_id != locked.registry_id
        || assignment.target_host_id != locked.target_host_id
    {
        return Ok(StorePlanOutput::Terminal {
            reason: "plugin assignment drifted before plan store".into(),
        });
    }

    let acknowledgement = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, planned.command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host plan acknowledgement for store: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime(
                "Plugin Host plan acknowledgement disappeared before plan store".into(),
            )
        })?;
    let NodeCommandOutcome::Succeeded { result } = acknowledgement.outcome else {
        return Ok(StorePlanOutput::Terminal {
            reason: "Plugin Host plan acknowledgement is no longer successful".into(),
        });
    };
    let (request_id, assignment_generation, envelope) = match *result {
        NodeCommandResult::PluginHostPlanned { plan, .. } => {
            (plan.request_id, plan.assignment_generation, plan.plan)
        }
        NodeCommandResult::PluginHostEnablementPlanned {
            enablement_plan, ..
        } => {
            let plan = enablement_plan.plan.ok_or_else(|| {
                FlowError::Runtime(
                    "Plugin Host enablement-plan acknowledgement omitted the operation plan".into(),
                )
            })?;
            (
                enablement_plan.request_id,
                enablement_plan.assignment_generation,
                plan,
            )
        }
        _ => {
            return Ok(StorePlanOutput::Terminal {
                reason: "Plugin Host plan acknowledgement no longer carries a plan result".into(),
            });
        }
    };
    let plan_digest = envelope.plan_digest.clone();
    if plan_digest != planned.plan_digest
        || request_id != planned.request_id
        || assignment_generation != locked.assignment_generation
        || envelope.plan.operation_id != planned.use_operation_id
    {
        return Ok(StorePlanOutput::Terminal {
            reason: "Plugin Host plan result drifted before plan store".into(),
        });
    }

    let plan_digest_value = Sha256Digest::parse(plan_digest.clone())
        .map_err(|error| FlowError::Runtime(error))?;
    let projection_id = store_plan_projection_id(locked.operation_id.as_uuid());
    if let Ok(Some(existing)) = runtime
        .projections
        .find_by_plan_digest(locked.organization_id, &plan_digest_value)
        .await
    {
        if existing.assignment_id == locked.assignment_id
            && existing.operation_id == locked.operation_id
            && existing.assignment_generation == locked.assignment_generation
        {
            return Ok(StorePlanOutput::Ready {
                stored: Box::new(StoredPlan {
                    planned: Box::new(planned),
                    projection_id: existing.id,
                    plan_digest: existing.plan_digest.as_str().to_owned(),
                    awaits_confirmation: existing.awaits_confirmation(),
                    stored_at: existing.created_at,
                }),
            });
        }
        return Ok(StorePlanOutput::Terminal {
            reason: "plugin plan projection already exists for this plan digest".into(),
        });
    }

    let now = Utc::now();
    let projection = PluginPlanProjection::from_validated_envelope(NewPluginPlanProjection {
        organization_id: locked.organization_id,
        id: projection_id,
        assignment_id: locked.assignment_id,
        operation_id: locked.operation_id,
        assignment_generation: locked.assignment_generation,
        envelope,
        created_at: now,
    })
    .map_err(FlowError::Runtime)?;
    let projection = match runtime.projections.create(projection).await {
        Ok(value) => value,
        Err(RepositoryError::Conflict(_)) => {
            match runtime
                .projections
                .find_by_plan_digest(locked.organization_id, &plan_digest_value)
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!(
                        "could not reload plugin plan projection after conflict: {error}"
                    ))
                })? {
                Some(existing)
                    if existing.assignment_id == locked.assignment_id
                        && existing.operation_id == locked.operation_id
                        && existing.assignment_generation == locked.assignment_generation =>
                {
                    existing
                }
                _ => {
                    return Ok(StorePlanOutput::Terminal {
                        reason: "plugin plan projection already exists for this plan digest"
                            .into(),
                    });
                }
            }
        }
        Err(error) => {
            return Err(FlowError::Runtime(format!(
                "could not store plugin plan projection: {error}"
            )));
        }
    };

    Ok(StorePlanOutput::Ready {
        stored: Box::new(StoredPlan {
            planned: Box::new(planned),
            projection_id: projection.id,
            plan_digest: projection.plan_digest.as_str().to_owned(),
            awaits_confirmation: projection.awaits_confirmation(),
            stored_at: projection.created_at,
        }),
    })
}
