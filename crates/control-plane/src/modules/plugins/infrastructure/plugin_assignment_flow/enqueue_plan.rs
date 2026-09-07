use super::plan_selection::{select_plan, SelectedPlan};
use super::types::{
    AlreadyConvergedAssignment, AuthorizedTrust, EnqueuePlanInput, EnqueuePlanOutput,
    PlannedAssignment,
};
use super::PluginAssignmentFlowRuntime;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::services::PluginRegistryCatalogError;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeCommandResult};
use a3s_flow::FlowError;
use a3s_use_core::{
    PluginDesiredState, PluginHostEnablementPlanRequest, PluginHostEnablementPlanStatus,
    PluginHostObservationRequest, PluginHostObservationStatus, PluginHostPackageState,
    PluginHostPlanRequest, PluginObservedState, PluginOperationAction, PluginPackageId,
    VerifiedPluginCatalogRecord, PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA,
    PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA, PLUGIN_HOST_PLAN_REQUEST_SCHEMA,
};
use a3s_use_extension::PluginCatalogHost;
use chrono::{Duration, Utc};
use uuid::Uuid;

/// Deterministic namespace for package-plan command IDs.
const ENQUEUE_PLAN_COMMAND_NS: Uuid = Uuid::from_bytes([
    0xa3, 0x50, 0x1u8, 0x70, 0x6c, 0x61, 0x6e, 0x2d, 0x65, 0x6e, 0x71, 0x00, 0x00, 0x00, 0x00, 0x01,
]);

/// Deterministic namespace for enablement-plan command IDs.
const ENQUEUE_ENABLEMENT_PLAN_COMMAND_NS: Uuid = Uuid::from_bytes([
    0xa3, 0x50, 0x1u8, 0x65, 0x6e, 0x61, 0x62, 0x6c, 0x65, 0x2d, 0x70, 0x6c, 0x61, 0x6e, 0x00,
    0x01,
]);

/// Deterministic namespace for pre-plan observation command IDs.
const PRE_PLAN_OBSERVE_COMMAND_NS: Uuid = Uuid::from_bytes([
    0xa3, 0x50, 0x1u8, 0x70, 0x72, 0x65, 0x2d, 0x70, 0x6c, 0x61, 0x6e, 0x2d, 0x6f, 0x62, 0x73,
    0x01,
]);

pub(super) fn enqueue_plan_command_id(operation_id: Uuid) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &ENQUEUE_PLAN_COMMAND_NS,
        operation_id.as_bytes(),
    ))
}

pub(super) fn enqueue_enablement_plan_command_id(operation_id: Uuid) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &ENQUEUE_ENABLEMENT_PLAN_COMMAND_NS,
        operation_id.as_bytes(),
    ))
}

pub(super) fn pre_plan_observe_command_id(operation_id: Uuid) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &PRE_PLAN_OBSERVE_COMMAND_NS,
        operation_id.as_bytes(),
    ))
}

pub(super) async fn enqueue_plan(
    runtime: &PluginAssignmentFlowRuntime,
    input: EnqueuePlanInput,
) -> a3s_flow::Result<EnqueuePlanOutput> {
    let authorized = *input.authorized;
    let locked = authorized.resolved.locked.as_ref();
    let assignment = runtime
        .assignments
        .find(locked.organization_id, locked.assignment_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "plugin assignment plan enqueue reload failed: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment disappeared before plan enqueue".into())
        })?;
    if assignment.organization_id != locked.organization_id
        || assignment.id != locked.assignment_id
        || assignment.current_operation_id != Some(locked.operation_id)
        || assignment.assignment_generation != locked.assignment_generation
        || assignment.registry_id != locked.registry_id
        || assignment.target_host_id != locked.target_host_id
    {
        return Ok(EnqueuePlanOutput::Terminal {
            reason: "plugin assignment drifted before plan enqueue".into(),
        });
    }

    let deadline_at = authorized
        .authorized_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment plan enqueue deadline overflowed".into())
        })?;
    let now = Utc::now().max(authorized.authorized_at);
    if now >= deadline_at {
        return Ok(EnqueuePlanOutput::Terminal {
            reason: "plugin assignment plan enqueue timed out".into(),
        });
    }

    let observed_state = match ensure_pre_plan_observation(
        runtime,
        &assignment,
        &authorized,
        now,
        deadline_at,
    )
    .await?
    {
        PrePlanObservation::Pending(output) => return Ok(output),
        PrePlanObservation::Ready(state) => state,
    };

    let selected = select_plan(&assignment, &observed_state).map_err(FlowError::Runtime)?;

    match selected {
        SelectedPlan::AlreadyConverged => Ok(EnqueuePlanOutput::AlreadyConverged {
            converged: Box::new(AlreadyConvergedAssignment {
                authorized: Box::new(authorized),
                observed_desired: desired_state_label(observed_state.desired).into(),
                observed_state: observed_state_label(observed_state.observed).into(),
                capability_generation: observed_state.capability_generation,
                package_generation: observed_state.package_generation,
                converged_at: Utc::now(),
            }),
        }),
        SelectedPlan::Package { action } => {
            enqueue_package_plan(runtime, &assignment, &authorized, action, now, deadline_at).await
        }
        SelectedPlan::Enablement {
            enabled,
            expected_package_generation,
        } => {
            enqueue_enablement_plan(
                runtime,
                &assignment,
                &authorized,
                enabled,
                expected_package_generation,
                now,
                deadline_at,
            )
            .await
        }
    }
}

enum PrePlanObservation {
    Ready(PluginHostPackageState),
    Pending(EnqueuePlanOutput),
}

async fn ensure_pre_plan_observation(
    runtime: &PluginAssignmentFlowRuntime,
    assignment: &PluginAssignment,
    authorized: &AuthorizedTrust,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
) -> a3s_flow::Result<PrePlanObservation> {
    let locked = authorized.resolved.locked.as_ref();
    let command_id = pre_plan_observe_command_id(locked.operation_id.as_uuid());
    let command = match runtime
        .node_control
        .find_command(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host pre-plan observation command: {error}"
            ))
        })? {
        Some(existing) => existing,
        None => {
            let request = PluginHostObservationRequest {
                schema: PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA.into(),
                request_id: format!(
                    "request:pre-plan-observe:{}",
                    locked.operation_id.as_uuid()
                ),
                assignment_generation: locked.assignment_generation,
                capabilities_digest: authorized.resolved.capabilities_digest.clone(),
                scope: assignment.workspace_scope.clone(),
                package_id: PluginPackageId::parse(assignment.package_id().as_str())
                    .map_err(|error| FlowError::Runtime(error.to_string()))?,
            };
            request
                .validate()
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "plugin assignment pre-plan observation command TTL overflowed".into(),
                    )
                })?
                .min(deadline_at);
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: locked.target_host_id,
                    aggregate_id: locked.assignment_id.as_uuid(),
                    payload: NodeCommandPayload::PluginHostObserve {
                        request: Box::new(request),
                    },
                    issued_at: now,
                    not_after,
                    correlation_id: locked.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!(
                        "could not enqueue Plugin Host pre-plan observation: {error}"
                    ))
                })?
                .value
        }
    };
    if command.id != command_id
        || command.node_id != locked.target_host_id
        || command.correlation_id != locked.operation_id.as_uuid()
        || command.generation() != locked.assignment_generation
    {
        return Err(FlowError::Runtime(
            "Plugin Host pre-plan observation command identity drifted on enqueue".into(),
        ));
    }

    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host pre-plan observation acknowledgement: {error}"
            ))
        })?
    else {
        if Utc::now() >= command.not_after {
            return Ok(PrePlanObservation::Pending(EnqueuePlanOutput::Terminal {
                reason: "Plugin Host pre-plan observation expired before acknowledgement".into(),
            }));
        }
        return Ok(PrePlanObservation::Pending(pending(
            "waiting for Plugin Host pre-plan observation acknowledgement".into(),
            Utc::now().max(now),
            deadline_at.min(command.not_after),
            runtime.config.observation_poll,
        )?));
    };

    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            let NodeCommandResult::PluginHostObserved { observation, .. } = *result else {
                return Ok(PrePlanObservation::Pending(EnqueuePlanOutput::Terminal {
                    reason: "Plugin Host pre-plan observation returned an unexpected result".into(),
                }));
            };
            match observation.status {
                PluginHostObservationStatus::Available { state } => {
                    state
                        .validate()
                        .map_err(|error| FlowError::Runtime(error.to_string()))?;
                    Ok(PrePlanObservation::Ready(state))
                }
                PluginHostObservationStatus::Unavailable { reason } => {
                    Ok(PrePlanObservation::Pending(pending(
                        format!(
                            "waiting for Plugin Host pre-plan observation availability ({reason:?})"
                        ),
                        Utc::now().max(now),
                        deadline_at,
                        runtime.config.observation_poll,
                    )?))
                }
            }
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(PrePlanObservation::Pending(EnqueuePlanOutput::Terminal {
                reason: format!(
                    "Plugin Host pre-plan observation {}: {}",
                    failure.code, failure.message
                ),
            }))
        }
    }
}

async fn enqueue_package_plan(
    runtime: &PluginAssignmentFlowRuntime,
    assignment: &PluginAssignment,
    authorized: &AuthorizedTrust,
    action: PluginOperationAction,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
) -> a3s_flow::Result<EnqueuePlanOutput> {
    let locked = authorized.resolved.locked.as_ref();
    let command_id = enqueue_plan_command_id(locked.operation_id.as_uuid());
    let command = match runtime
        .node_control
        .find_command(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host plan command: {error}"
            ))
        })? {
        Some(existing) => existing,
        None => {
            let request =
                build_package_plan_request(runtime, assignment, authorized, action).await?;
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime("plugin assignment plan command TTL overflowed".into())
                })?
                .min(deadline_at);
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: locked.target_host_id,
                    aggregate_id: locked.assignment_id.as_uuid(),
                    payload: NodeCommandPayload::PluginHostPlan {
                        request: Box::new(request),
                    },
                    issued_at: now,
                    not_after,
                    correlation_id: locked.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!("could not enqueue Plugin Host plan: {error}"))
                })?
                .value
        }
    };
    if command.id != command_id
        || command.node_id != locked.target_host_id
        || command.correlation_id != locked.operation_id.as_uuid()
        || command.generation() != locked.assignment_generation
    {
        return Err(FlowError::Runtime(
            "Plugin Host plan command identity drifted on enqueue".into(),
        ));
    }
    let NodeCommandPayload::PluginHostPlan { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "Plugin Host plan command payload kind drifted".into(),
        ));
    };

    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host plan acknowledgement: {error}"
            ))
        })?
    else {
        if Utc::now() >= command.not_after {
            return Ok(EnqueuePlanOutput::Terminal {
                reason: "Plugin Host plan expired before acknowledgement".into(),
            });
        }
        return pending(
            "waiting for Plugin Host plan acknowledgement".into(),
            Utc::now().max(now),
            deadline_at.min(command.not_after),
            runtime.config.observation_poll,
        );
    };

    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            let NodeCommandResult::PluginHostPlanned {
                capabilities,
                plan,
            } = *result
            else {
                return Ok(EnqueuePlanOutput::Terminal {
                    reason: "Plugin Host plan returned an unexpected result".into(),
                });
            };
            plan.validate_for(request, &capabilities)
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            let plan_digest = plan.plan.plan_digest.clone();
            Ok(EnqueuePlanOutput::Ready {
                planned: Box::new(PlannedAssignment {
                    authorized: Box::new(authorized.clone()),
                    command_id,
                    request_id: plan.request_id,
                    plan_digest,
                    use_operation_id: plan.plan.plan.operation_id,
                    plan_kind: "package".into(),
                    action: format!("{:?}", plan.plan.plan.action).to_ascii_lowercase(),
                    planned_at: Utc::now(),
                }),
            })
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(EnqueuePlanOutput::Terminal {
                reason: format!("Plugin Host plan {}: {}", failure.code, failure.message),
            })
        }
    }
}

async fn enqueue_enablement_plan(
    runtime: &PluginAssignmentFlowRuntime,
    assignment: &PluginAssignment,
    authorized: &AuthorizedTrust,
    enabled: bool,
    expected_package_generation: u64,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
) -> a3s_flow::Result<EnqueuePlanOutput> {
    let locked = authorized.resolved.locked.as_ref();
    let command_id = enqueue_enablement_plan_command_id(locked.operation_id.as_uuid());
    let command = match runtime
        .node_control
        .find_command(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host enablement-plan command: {error}"
            ))
        })? {
        Some(existing) => existing,
        None => {
            let request = PluginHostEnablementPlanRequest {
                schema: PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA.into(),
                request_id: format!(
                    "request:enablement-plan:{}",
                    locked.operation_id.as_uuid()
                ),
                assignment_generation: locked.assignment_generation,
                capabilities_digest: authorized.resolved.capabilities_digest.clone(),
                scope: assignment.workspace_scope.clone(),
                package_id: PluginPackageId::parse(assignment.package_id().as_str())
                    .map_err(|error| FlowError::Runtime(error.to_string()))?,
                expected_package_generation,
                enabled,
            };
            request
                .validate()
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "plugin assignment enablement-plan command TTL overflowed".into(),
                    )
                })?
                .min(deadline_at);
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: locked.target_host_id,
                    aggregate_id: locked.assignment_id.as_uuid(),
                    payload: NodeCommandPayload::PluginHostPlanEnablement {
                        request: Box::new(request),
                    },
                    issued_at: now,
                    not_after,
                    correlation_id: locked.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!(
                        "could not enqueue Plugin Host enablement-plan: {error}"
                    ))
                })?
                .value
        }
    };
    if command.id != command_id
        || command.node_id != locked.target_host_id
        || command.correlation_id != locked.operation_id.as_uuid()
        || command.generation() != locked.assignment_generation
    {
        return Err(FlowError::Runtime(
            "Plugin Host enablement-plan command identity drifted on enqueue".into(),
        ));
    }
    let NodeCommandPayload::PluginHostPlanEnablement { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "Plugin Host enablement-plan command payload kind drifted".into(),
        ));
    };

    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host enablement-plan acknowledgement: {error}"
            ))
        })?
    else {
        if Utc::now() >= command.not_after {
            return Ok(EnqueuePlanOutput::Terminal {
                reason: "Plugin Host enablement-plan expired before acknowledgement".into(),
            });
        }
        return pending(
            "waiting for Plugin Host enablement-plan acknowledgement".into(),
            Utc::now().max(now),
            deadline_at.min(command.not_after),
            runtime.config.observation_poll,
        );
    };

    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            let NodeCommandResult::PluginHostEnablementPlanned {
                capabilities,
                enablement_plan,
            } = *result
            else {
                return Ok(EnqueuePlanOutput::Terminal {
                    reason: "Plugin Host enablement-plan returned an unexpected result".into(),
                });
            };
            enablement_plan
                .validate_for(request, &capabilities)
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            match enablement_plan.status {
                PluginHostEnablementPlanStatus::NoChange => {
                    Ok(EnqueuePlanOutput::AlreadyConverged {
                        converged: Box::new(AlreadyConvergedAssignment {
                            authorized: Box::new(authorized.clone()),
                            observed_desired: desired_state_label(enablement_plan.state.desired)
                                .into(),
                            observed_state: observed_state_label(enablement_plan.state.observed)
                                .into(),
                            capability_generation: enablement_plan.state.capability_generation,
                            package_generation: enablement_plan.state.package_generation,
                            converged_at: Utc::now(),
                        }),
                    })
                }
                PluginHostEnablementPlanStatus::Planned => {
                    let plan = enablement_plan.plan.ok_or_else(|| {
                        FlowError::Runtime(
                            "Plugin Host enablement-plan Planned result omitted the operation plan"
                                .into(),
                        )
                    })?;
                    let plan_digest = plan.plan_digest.clone();
                    let action = format!("{:?}", plan.plan.action).to_ascii_lowercase();
                    let use_operation_id = plan.plan.operation_id.clone();
                    Ok(EnqueuePlanOutput::Ready {
                        planned: Box::new(PlannedAssignment {
                            authorized: Box::new(authorized.clone()),
                            command_id,
                            request_id: enablement_plan.request_id,
                            plan_digest,
                            use_operation_id,
                            plan_kind: "enablement".into(),
                            action,
                            planned_at: Utc::now(),
                        }),
                    })
                }
            }
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(EnqueuePlanOutput::Terminal {
                reason: format!(
                    "Plugin Host enablement-plan {}: {}",
                    failure.code, failure.message
                ),
            })
        }
    }
}

async fn build_package_plan_request(
    runtime: &PluginAssignmentFlowRuntime,
    assignment: &PluginAssignment,
    authorized: &AuthorizedTrust,
    action: PluginOperationAction,
) -> a3s_flow::Result<PluginHostPlanRequest> {
    let locked = authorized.resolved.locked.as_ref();
    let action = match action {
        PluginOperationAction::Enable | PluginOperationAction::Disable => {
            return Err(FlowError::Runtime(
                "enable/disable must use Plugin Host enablement-plan, not package-plan".into(),
            ));
        }
        other => other,
    };
    let candidate = match action {
        PluginOperationAction::Uninstall => None,
        PluginOperationAction::Install | PluginOperationAction::Upgrade => {
            Some(load_install_candidate(runtime, assignment, authorized).await?)
        }
        PluginOperationAction::Enable | PluginOperationAction::Disable => unreachable!(),
    };
    let mut selected_surfaces = match action {
        PluginOperationAction::Uninstall => Vec::new(),
        _ => assignment.selection.selected_surfaces.clone(),
    };
    selected_surfaces.sort();
    let request = PluginHostPlanRequest {
        schema: PLUGIN_HOST_PLAN_REQUEST_SCHEMA.into(),
        request_id: format!("request:plan:{}", locked.operation_id.as_uuid()),
        assignment_generation: locked.assignment_generation,
        capabilities_digest: authorized.resolved.capabilities_digest.clone(),
        scope: assignment.workspace_scope.clone(),
        action,
        package_id: PluginPackageId::parse(assignment.package_id().as_str())
            .map_err(|error| FlowError::Runtime(error.to_string()))?,
        candidate,
        package_lock: None,
        selected_surfaces,
    };
    request
        .validate()
        .map_err(|error| FlowError::Runtime(error.to_string()))?;
    Ok(request)
}

async fn load_install_candidate(
    runtime: &PluginAssignmentFlowRuntime,
    assignment: &PluginAssignment,
    authorized: &AuthorizedTrust,
) -> a3s_flow::Result<VerifiedPluginCatalogRecord> {
    let locked = authorized.resolved.locked.as_ref();
    let registry = runtime
        .registries
        .find(locked.organization_id, locked.registry_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!("could not load enrolled plugin registry: {error}"))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("enrolled plugin registry was not found for plan enqueue".into())
        })?;
    let host = catalog_host_from_resolved(authorized)?;
    let inspection = runtime
        .catalog
        .inspect_cached(
            &registry,
            &host,
            assignment.package_id().as_str(),
            Some(assignment.selection.version.as_str()),
            None,
        )
        .await
        .map_err(map_catalog_error)?;
    let candidate = inspection.plugin;
    candidate
        .validate()
        .map_err(|error| FlowError::Runtime(error.to_string()))?;
    let catalog_record_digest = candidate
        .record
        .descriptor_digest()
        .map_err(|error| FlowError::Runtime(error.to_string()))?;
    if catalog_record_digest != assignment.selection.catalog_record_digest.as_str()
        || candidate.record.package_id != assignment.package_id().as_str()
        || candidate.record.version != assignment.selection.version
        || candidate.record.package.sha256.as_deref()
            != Some(assignment.selection.package_digest.as_str())
        || candidate.record.package.manifest_sha256.as_deref()
            != Some(assignment.selection.manifest_digest.as_str())
    {
        return Err(FlowError::Runtime(
            "verified catalog candidate drifted from the locked assignment selection".into(),
        ));
    }
    candidate
        .record
        .resolve_surfaces(&assignment.selection.selected_surfaces)
        .map_err(|error| FlowError::Runtime(error.to_string()))?;
    Ok(candidate)
}

fn catalog_host_from_resolved(
    authorized: &AuthorizedTrust,
) -> a3s_flow::Result<PluginCatalogHost> {
    let target = authorized
        .resolved
        .manager_build_id
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            FlowError::Runtime(
                "Plugin Host manager build id does not encode a catalog target".into(),
            )
        })?;
    PluginCatalogHost::new(target, authorized.resolved.manager_version.clone())
        .map_err(|error| FlowError::Runtime(error.to_string()))
}

fn pending(
    reason: String,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
    poll: Duration,
) -> a3s_flow::Result<EnqueuePlanOutput> {
    let next_poll_at = now
        .checked_add_signed(poll)
        .ok_or_else(|| FlowError::Runtime("plan enqueue poll overflowed".into()))?
        .min(deadline_at);
    Ok(EnqueuePlanOutput::Pending {
        reason,
        next_poll_at,
        deadline_at,
    })
}

fn map_catalog_error(error: PluginRegistryCatalogError) -> FlowError {
    match error {
        PluginRegistryCatalogError::PackageNotFound => FlowError::Runtime(
            "selected plugin package was not present in the verified catalog cache".into(),
        ),
        PluginRegistryCatalogError::PackageIncompatible => FlowError::Runtime(
            "selected plugin package is incompatible with the resolved Plugin Host".into(),
        ),
        other => FlowError::Runtime(format!("could not inspect plugin catalog for plan: {other}")),
    }
}

fn desired_state_label(desired: PluginDesiredState) -> &'static str {
    match desired {
        PluginDesiredState::Enabled => "enabled",
        PluginDesiredState::InstalledDisabled => "installed-disabled",
        PluginDesiredState::Absent => "absent",
    }
}

fn observed_state_label(observed: PluginObservedState) -> &'static str {
    match observed {
        PluginObservedState::Installed => "installed",
        PluginObservedState::Reconciling => "reconciling",
        PluginObservedState::Ready => "ready",
        PluginObservedState::Degraded => "degraded",
        PluginObservedState::Broken => "broken",
        PluginObservedState::Incompatible => "incompatible",
        PluginObservedState::Draining => "draining",
        PluginObservedState::Removed => "removed",
    }
}
