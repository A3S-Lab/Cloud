use super::types::{
    AuthorizedTrust, AuthorizeTrustInput, AuthorizeTrustOutput, ResolvedHost,
};
use super::PluginAssignmentFlowRuntime;
use crate::modules::artifacts::application::{INodeArtifactStore, NodeArtifactDescriptor};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::plugins::domain::services::{
    PluginPolicyStoreError, PluginTrustRootStoreError,
};
use crate::modules::shared_kernel::domain::{NodeCommandId, Sha256Digest};
use a3s_cloud_contracts::{
    artifact_uri, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodePluginHostAuthorizeTrustRequest, PLUGIN_POLICY_ACL_MEDIA_TYPE, PLUGIN_TRUST_ROOT_MEDIA_TYPE,
};
use a3s_flow::FlowError;
use a3s_runtime::contract::ArtifactRef;
use chrono::{Duration, Utc};
use uuid::Uuid;

/// Deterministic namespace for authorize-trust command IDs derived from the
/// assignment Operation. Capabilities inspect reuses the raw Operation UUID.
const AUTHORIZE_TRUST_COMMAND_NS: Uuid = Uuid::from_bytes([
    0xa3, 0x50, 0x1u8, 0x67, 0x74, 0x72, 0x75, 0x73, 0x74, 0x2d, 0x61, 0x75, 0x74, 0x68, 0x00, 0x01,
]);

pub(super) fn authorize_trust_command_id(operation_id: Uuid) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &AUTHORIZE_TRUST_COMMAND_NS,
        operation_id.as_bytes(),
    ))
}

pub(super) async fn authorize_trust(
    runtime: &PluginAssignmentFlowRuntime,
    input: AuthorizeTrustInput,
) -> a3s_flow::Result<AuthorizeTrustOutput> {
    let resolved = *input.resolved;
    let locked = resolved.locked.as_ref();
    let assignment = runtime
        .assignments
        .find(locked.organization_id, locked.assignment_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "plugin assignment trust authorization reload failed: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment disappeared before trust authorization".into())
        })?;
    if assignment.organization_id != locked.organization_id
        || assignment.id != locked.assignment_id
        || assignment.current_operation_id != Some(locked.operation_id)
        || assignment.assignment_generation != locked.assignment_generation
        || assignment.registry_id != locked.registry_id
        || assignment.target_host_id != locked.target_host_id
    {
        return Ok(AuthorizeTrustOutput::Terminal {
            reason: "plugin assignment drifted before trust authorization".into(),
        });
    }

    let registry = runtime
        .registries
        .find(locked.organization_id, locked.registry_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!("could not load enrolled plugin registry: {error}"))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("enrolled plugin registry was not found for trust authorization".into())
        })?;
    if registry.organization_id != locked.organization_id || registry.id != locked.registry_id {
        return Ok(AuthorizeTrustOutput::Terminal {
            reason: "plugin registry identity drifted before trust authorization".into(),
        });
    }

    let deadline_at = resolved
        .resolved_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment trust authorization deadline overflowed".into())
        })?;
    let now = Utc::now().max(resolved.resolved_at);
    if now >= deadline_at {
        return Ok(AuthorizeTrustOutput::Terminal {
            reason: "plugin assignment trust authorization timed out".into(),
        });
    }

    let trust_root_bytes = runtime
        .trust_roots
        .get(&registry.trust_root)
        .await
        .map_err(map_trust_root_error)?;
    let policy_bytes = runtime
        .policies
        .get(&assignment.policy_digest)
        .await
        .map_err(map_policy_error)?;

    let trust_root_artifact = admit_bytes(
        runtime.artifacts.as_ref(),
        &trust_root_bytes,
        PLUGIN_TRUST_ROOT_MEDIA_TYPE,
    )
    .await?;
    let policy_artifact = admit_bytes(
        runtime.artifacts.as_ref(),
        &policy_bytes,
        PLUGIN_POLICY_ACL_MEDIA_TYPE,
    )
    .await?;
    if Sha256Digest::parse(trust_root_artifact.digest.clone()).ok().as_ref()
        != Some(registry.trust_root.digest())
        || Sha256Digest::parse(policy_artifact.digest.clone()).ok().as_ref()
            != Some(&assignment.policy_digest)
    {
        return Err(FlowError::Runtime(
            "admitted trust or policy Artifact digest drifted from enrolled evidence".into(),
        ));
    }

    let command_id = authorize_trust_command_id(locked.operation_id.as_uuid());
    let request = NodePluginHostAuthorizeTrustRequest::new(
        locked.assignment_generation,
        trust_root_artifact.clone(),
        policy_artifact.clone(),
    )
    .map_err(FlowError::Runtime)?;

    let command = match runtime
        .node_control
        .find_command(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host authorize-trust command: {error}"
            ))
        })? {
        Some(existing) => existing,
        None => {
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "plugin assignment authorize-trust command TTL overflowed".into(),
                    )
                })?
                .min(deadline_at);
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: locked.target_host_id,
                    aggregate_id: locked.assignment_id.as_uuid(),
                    payload: NodeCommandPayload::PluginHostAuthorizeTrust {
                        request: Box::new(request.clone()),
                    },
                    issued_at: now,
                    not_after,
                    correlation_id: locked.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!(
                        "could not enqueue Plugin Host authorize-trust: {error}"
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
            "Plugin Host authorize-trust command identity drifted on enqueue".into(),
        ));
    }

    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host authorize-trust acknowledgement: {error}"
            ))
        })?
    else {
        if Utc::now() >= command.not_after {
            return Ok(AuthorizeTrustOutput::Terminal {
                reason: "Plugin Host authorize-trust expired before acknowledgement".into(),
            });
        }
        return Ok(pending(
            "waiting for Plugin Host authorize-trust acknowledgement".into(),
            Utc::now().max(now),
            deadline_at.min(command.not_after),
            runtime.config.observation_poll,
        )?);
    };

    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            let NodeCommandResult::PluginHostTrustAuthorized { authorized } = *result else {
                return Ok(AuthorizeTrustOutput::Terminal {
                    reason: "Plugin Host authorize-trust returned an unexpected result".into(),
                });
            };
            authorized
                .validate_for(&request)
                .map_err(|error| FlowError::Runtime(error))?;
            Ok(AuthorizeTrustOutput::Ready {
                authorized: Box::new(AuthorizedTrust {
                    resolved: Box::new(ResolvedHost {
                        locked: resolved.locked,
                        command_id: resolved.command_id,
                        host_id: resolved.host_id,
                        manager_version: resolved.manager_version,
                        manager_build_id: resolved.manager_build_id,
                        capabilities_digest: resolved.capabilities_digest,
                        resolved_at: resolved.resolved_at,
                    }),
                    command_id,
                    trust_root_digest: authorized.trust_root_digest,
                    policy_digest: authorized.policy_digest,
                    authorized_at: authorized.authorized_at,
                }),
            })
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(AuthorizeTrustOutput::Terminal {
                reason: format!(
                    "Plugin Host authorize-trust {}: {}",
                    failure.code, failure.message
                ),
            })
        }
    }
}

async fn admit_bytes(
    artifacts: &dyn INodeArtifactStore,
    bytes: &[u8],
    media_type: &str,
) -> a3s_flow::Result<ArtifactRef> {
    let digest = Sha256Digest::from_bytes(bytes);
    let artifact = ArtifactRef {
        uri: artifact_uri(digest.as_str()).map_err(FlowError::Runtime)?,
        digest: digest.as_str().to_owned(),
        media_type: media_type.into(),
    };
    let descriptor = NodeArtifactDescriptor::new(artifact.clone(), bytes.len() as u64)
        .map_err(FlowError::Runtime)?;
    let reader: crate::modules::artifacts::application::NodeArtifactReader =
        Box::pin(std::io::Cursor::new(bytes.to_vec()));
    let write = artifacts.put(&descriptor, reader).await.map_err(|error| {
        FlowError::Runtime(format!("could not admit Plugin Host Artifact: {error}"))
    })?;
    if write.descriptor.artifact != artifact {
        return Err(FlowError::Runtime(
            "node Artifact store changed the admitted Plugin Host Artifact identity".into(),
        ));
    }
    Ok(artifact)
}

fn pending(
    reason: String,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
    poll: Duration,
) -> a3s_flow::Result<AuthorizeTrustOutput> {
    let next_poll_at = now
        .checked_add_signed(poll)
        .ok_or_else(|| FlowError::Runtime("authorize-trust poll overflowed".into()))?
        .min(deadline_at);
    Ok(AuthorizeTrustOutput::Pending {
        reason,
        next_poll_at,
        deadline_at,
    })
}

fn map_trust_root_error(error: PluginTrustRootStoreError) -> FlowError {
    match error {
        PluginTrustRootStoreError::NotFound => FlowError::Runtime(
            "enrolled plugin trust-root object was not found for authorize-trust".into(),
        ),
        other => FlowError::Runtime(format!("could not load plugin trust-root object: {other}")),
    }
}

fn map_policy_error(error: PluginPolicyStoreError) -> FlowError {
    match error {
        PluginPolicyStoreError::NotFound => FlowError::Runtime(
            "selected plugin policy ACL was not found for authorize-trust".into(),
        ),
        other => FlowError::Runtime(format!("could not load plugin policy ACL: {other}")),
    }
}
