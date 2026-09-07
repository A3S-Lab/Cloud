use super::types::{ResolveHostInput, ResolveHostOutput, ResolvedHost};
use super::PluginAssignmentFlowRuntime;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::domain::{NodeCommandId, RepositoryError};
use a3s_cloud_contracts::{
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodePluginHostCapabilitiesRequest,
};
use a3s_flow::FlowError;
use a3s_use_core::{
    PluginHostCapabilities, PluginSurfaceKind, PLUGIN_CATALOG_SCHEMA_V3,
    PLUGIN_HOST_CAPABILITIES_SCHEMA_V6, PLUGIN_HOST_PROTOCOL_LEVEL_V6,
    PLUGIN_OPERATION_PLAN_SCHEMA_V4,
};
use chrono::{Duration, Utc};

pub(super) async fn resolve_host(
    runtime: &PluginAssignmentFlowRuntime,
    input: ResolveHostInput,
) -> a3s_flow::Result<ResolveHostOutput> {
    let locked = *input.locked;
    let assignment = runtime
        .assignments
        .find(locked.organization_id, locked.assignment_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!("plugin assignment host resolution reload failed: {error}"))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment disappeared before host resolution".into())
        })?;
    if assignment.organization_id != locked.organization_id
        || assignment.id != locked.assignment_id
        || assignment.current_operation_id != Some(locked.operation_id)
        || assignment.assignment_generation != locked.assignment_generation
        || assignment.target_host_id != locked.target_host_id
    {
        return Ok(ResolveHostOutput::Terminal {
            reason: "plugin assignment drifted before host resolution".into(),
        });
    }

    let node = match runtime
        .nodes
        .find(locked.organization_id, locked.target_host_id)
        .await
    {
        Ok(node) => node,
        Err(RepositoryError::NotFound) => {
            return Ok(ResolveHostOutput::Terminal {
                reason: "plugin assignment target host was not found".into(),
            });
        }
        Err(error) => {
            return Err(FlowError::Runtime(format!(
                "could not load target Plugin Host: {error}"
            )));
        }
    };
    if node.organization_id != locked.organization_id {
        return Ok(ResolveHostOutput::Terminal {
            reason: "plugin assignment target host belongs to another organization".into(),
        });
    }
    let deadline_at = locked
        .locked_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| FlowError::Runtime("plugin assignment host resolution deadline overflowed".into()))?;
    let now = Utc::now().max(locked.locked_at);
    if now >= deadline_at {
        return Ok(ResolveHostOutput::Terminal {
            reason: "plugin assignment host resolution timed out before the node was ready".into(),
        });
    }
    if node.state != NodeState::Ready {
        return Ok(pending(
            format!(
                "waiting for target Plugin Host to become ready (state={})",
                node.state.as_str()
            ),
            now,
            deadline_at,
            runtime.config.observation_poll,
        )?);
    }

    let command_id = NodeCommandId::from_uuid(locked.operation_id.as_uuid());
    // Reuse an existing inspect command across Flow polls. Re-issuing with a
    // fresh issued_at/not_after is rejected as command-id reuse with different input.
    let command = match runtime
        .node_control
        .find_command(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host capabilities inspect command: {error}"
            ))
        })? {
        Some(existing) => existing,
        None => {
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime("plugin assignment capabilities command TTL overflowed".into())
                })?
                .min(deadline_at);
            let payload = NodeCommandPayload::PluginHostCapabilitiesInspect {
                request: NodePluginHostCapabilitiesRequest::new(locked.assignment_generation)
                    .map_err(|error| FlowError::Runtime(error))?,
            };
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: locked.target_host_id,
                    aggregate_id: locked.assignment_id.as_uuid(),
                    payload,
                    issued_at: now,
                    not_after,
                    correlation_id: locked.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!(
                        "could not enqueue Plugin Host capabilities inspect: {error}"
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
            "Plugin Host capabilities inspect command identity drifted on enqueue".into(),
        ));
    }

    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host capabilities acknowledgement: {error}"
            ))
        })?
    else {
        if Utc::now() >= command.not_after {
            return Ok(ResolveHostOutput::Terminal {
                reason: "Plugin Host capabilities inspect expired before acknowledgement".into(),
            });
        }
        return Ok(pending(
            "waiting for Plugin Host capabilities inspect acknowledgement".into(),
            Utc::now().max(now),
            deadline_at.min(command.not_after),
            runtime.config.observation_poll,
        )?);
    };

    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            let NodeCommandResult::PluginHostCapabilitiesInspected { capabilities } = *result
            else {
                return Ok(ResolveHostOutput::Terminal {
                    reason: "Plugin Host capabilities inspect returned an unexpected result".into(),
                });
            };
            validate_capabilities(&assignment, &capabilities)?;
            let capabilities_digest = capabilities
                .descriptor_digest()
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            Ok(ResolveHostOutput::Ready {
                resolved: Box::new(ResolvedHost {
                    locked: Box::new(locked),
                    command_id,
                    host_id: capabilities.host_id,
                    manager_version: capabilities.manager_version,
                    manager_build_id: capabilities.manager_build_id,
                    capabilities_digest,
                    resolved_at: Utc::now(),
                }),
            })
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(ResolveHostOutput::Terminal {
                reason: format!(
                    "Plugin Host capabilities inspect failed: {} ({})",
                    failure.message, failure.code
                ),
            })
        }
    }
}

fn validate_capabilities(
    assignment: &crate::modules::plugins::domain::entities::PluginAssignment,
    capabilities: &PluginHostCapabilities,
) -> a3s_flow::Result<()> {
    capabilities
        .validate()
        .map_err(|error| FlowError::Runtime(error.to_string()))?;
    if capabilities.schema != PLUGIN_HOST_CAPABILITIES_SCHEMA_V6
        || capabilities.protocol_level != PLUGIN_HOST_PROTOCOL_LEVEL_V6
        || !capabilities.exclusive_managed_scope_mutation
        || !capabilities.supports_plan_schema(PLUGIN_OPERATION_PLAN_SCHEMA_V4)
        || !capabilities
            .catalog_schemas
            .iter()
            .any(|schema| schema == PLUGIN_CATALOG_SCHEMA_V3)
    {
        return Err(FlowError::Runtime(
            "Plugin Host capabilities do not advertise the frozen U0 protocol inventory".into(),
        ));
    }
    if capabilities.host_id != assignment.workspace_scope.host_id {
        return Err(FlowError::Runtime(
            "Plugin Host capabilities host identity does not match the assignment managed scope"
                .into(),
        ));
    }
    for surface in &assignment.selection.selected_surfaces {
        if !capabilities.surface_kinds.contains(&surface.kind) {
            return Err(FlowError::Runtime(format!(
                "Plugin Host does not advertise selected surface kind {:?}",
                surface_kind_label(surface.kind)
            )));
        }
    }
    Ok(())
}

fn surface_kind_label(kind: PluginSurfaceKind) -> &'static str {
    match kind {
        PluginSurfaceKind::Flow => "flow",
        PluginSurfaceKind::Mcp => "mcp",
        PluginSurfaceKind::Okf => "okf",
        PluginSurfaceKind::Skill => "skill",
        PluginSurfaceKind::Tool => "tool",
        PluginSurfaceKind::Ui => "ui",
    }
}

fn pending(
    reason: String,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
    poll: Duration,
) -> a3s_flow::Result<ResolveHostOutput> {
    let next_poll_at = now
        .checked_add_signed(poll)
        .ok_or_else(|| FlowError::Runtime("plugin assignment host resolution poll overflowed".into()))?
        .min(deadline_at);
    Ok(ResolveHostOutput::Pending {
        reason,
        next_poll_at,
        deadline_at,
    })
}
