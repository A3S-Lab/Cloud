use crate::modules::plugins::domain::entities::PluginAssignment;
use a3s_use_core::{
    PluginDesiredState, PluginHostPackageState, PluginObservedState, PluginOperationAction,
};

/// One Use-owned plan selected from assignment desired state vs host observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SelectedPlan {
    Package {
        action: PluginOperationAction,
    },
    Enablement {
        enabled: bool,
        expected_package_generation: u64,
    },
    AlreadyConverged,
}

pub(super) fn select_plan(
    assignment: &PluginAssignment,
    state: &PluginHostPackageState,
) -> Result<SelectedPlan, String> {
    match assignment.desired_state {
        PluginDesiredState::Absent => {
            if state.observed == PluginObservedState::Removed {
                Ok(SelectedPlan::AlreadyConverged)
            } else {
                Ok(SelectedPlan::Package {
                    action: PluginOperationAction::Uninstall,
                })
            }
        }
        PluginDesiredState::Enabled | PluginDesiredState::InstalledDisabled => {
            if package_matches_assignment(assignment, state) {
                if state.desired == assignment.desired_state
                    && enablement_observation_ready(assignment.desired_state, state.observed)
                {
                    return Ok(SelectedPlan::AlreadyConverged);
                }
                if state.desired != assignment.desired_state {
                    let expected_package_generation = state.package_generation.ok_or_else(|| {
                        String::from(
                            "Plugin Host package generation is required for enablement planning",
                        )
                    })?;
                    if expected_package_generation == 0 {
                        return Err(
                            "Plugin Host package generation must be positive for enablement"
                                .into(),
                        );
                    }
                    return Ok(SelectedPlan::Enablement {
                        enabled: assignment.desired_state == PluginDesiredState::Enabled,
                        expected_package_generation,
                    });
                }
                return Ok(SelectedPlan::Package {
                    action: PluginOperationAction::Upgrade,
                });
            }
            let action = if matches!(state.observed, PluginObservedState::Removed)
                || state.version.is_none()
            {
                PluginOperationAction::Install
            } else {
                PluginOperationAction::Upgrade
            };
            Ok(SelectedPlan::Package { action })
        }
    }
}

fn package_matches_assignment(
    assignment: &PluginAssignment,
    state: &PluginHostPackageState,
) -> bool {
    !matches!(state.observed, PluginObservedState::Removed)
        && state.version.as_deref() == Some(assignment.selection.version.as_str())
        && state.package_digest.as_deref()
            == Some(assignment.selection.package_digest.as_str())
        && state.manifest_digest.as_deref()
            == Some(assignment.selection.manifest_digest.as_str())
}

fn enablement_observation_ready(
    desired: PluginDesiredState,
    observed: PluginObservedState,
) -> bool {
    match (desired, observed) {
        (PluginDesiredState::Enabled, PluginObservedState::Ready) => true,
        (PluginDesiredState::InstalledDisabled, PluginObservedState::Installed)
        | (PluginDesiredState::InstalledDisabled, PluginObservedState::Ready) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::plugins::domain::entities::{NewPluginAssignment, PluginAssignment};
    use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OperationId, OrganizationId, PluginAssignmentId, PluginRegistryId,
        PrincipalId, ProjectId, Sha256Digest,
    };
    use a3s_use_core::{
        PluginManagedScope, PluginPackageId, PluginSurfaceKind, PluginSurfaceRef, PlanScopeKind,
        PLUGIN_MANAGED_SCOPE_SCHEMA_V2,
    };
    use chrono::Utc;
    use uuid::Uuid;

    fn assignment(desired: PluginDesiredState) -> PluginAssignment {
        let organization_id = OrganizationId::from_uuid(Uuid::now_v7());
        let selection = PluginCatalogSelection {
            package_id: PluginPackageId::parse("a3s/selftest").expect("package"),
            catalog_record_digest: digest('a'),
            version: "1.2.3".into(),
            package_digest: digest('b'),
            manifest_digest: digest('c'),
            selected_surfaces: vec![PluginSurfaceRef {
                kind: PluginSurfaceKind::Skill,
                id: "selftest".into(),
            }],
        };
        PluginAssignment::create(NewPluginAssignment {
            organization_id,
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            id: PluginAssignmentId::new(),
            registry_id: PluginRegistryId::new(),
            target_host_id: NodeId::new(),
            workspace_scope: PluginManagedScope {
                schema: PLUGIN_MANAGED_SCOPE_SCHEMA_V2.into(),
                host_id: "host:node-01".into(),
                scope_kind: PlanScopeKind::Workspace,
                scope_id: "workspace:research".into(),
                authority_id: "cloud:organization-01".into(),
                fence_generation: 7,
                fence_digest: digest('d').as_str().into(),
            },
            selection,
            policy_digest: digest('e'),
            desired_state: desired,
            actor_id: PrincipalId::new(),
            request_id: Uuid::now_v7(),
            operation_id: OperationId::new(),
            created_at: Utc::now(),
        })
        .expect("assignment")
    }

    fn digest(fill: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", fill.to_string().repeat(64))).expect("digest")
    }

    fn state(
        desired: PluginDesiredState,
        observed: PluginObservedState,
        present: bool,
    ) -> PluginHostPackageState {
        let assignment = assignment(PluginDesiredState::Enabled);
        PluginHostPackageState {
            version: present.then(|| assignment.selection.version.clone()),
            package_generation: present.then_some(13),
            package_digest: present.then(|| assignment.selection.package_digest.as_str().into()),
            manifest_digest: present.then(|| assignment.selection.manifest_digest.as_str().into()),
            receipt_digest: present.then(|| digest('f').as_str().into()),
            capability_generation: 14,
            capability_revision: digest('1').as_str().into(),
            desired,
            observed,
            selected_surfaces: assignment.selection.selected_surfaces.clone(),
        }
    }

    #[test]
    fn selects_install_when_package_absent_and_desired_enabled() {
        let selected = select_plan(
            &assignment(PluginDesiredState::Enabled),
            &state(
                PluginDesiredState::Absent,
                PluginObservedState::Removed,
                false,
            ),
        )
        .expect("selection");
        assert_eq!(
            selected,
            SelectedPlan::Package {
                action: PluginOperationAction::Install
            }
        );
    }

    #[test]
    fn selects_enablement_when_only_desired_enablement_drifts() {
        let selected = select_plan(
            &assignment(PluginDesiredState::InstalledDisabled),
            &state(
                PluginDesiredState::Enabled,
                PluginObservedState::Ready,
                true,
            ),
        )
        .expect("selection");
        assert_eq!(
            selected,
            SelectedPlan::Enablement {
                enabled: false,
                expected_package_generation: 13,
            }
        );
    }

    #[test]
    fn selects_already_converged_when_observation_matches() {
        let selected = select_plan(
            &assignment(PluginDesiredState::Enabled),
            &state(
                PluginDesiredState::Enabled,
                PluginObservedState::Ready,
                true,
            ),
        )
        .expect("selection");
        assert_eq!(selected, SelectedPlan::AlreadyConverged);
    }

    #[test]
    fn selects_uninstall_when_desired_absent_and_package_present() {
        let selected = select_plan(
            &assignment(PluginDesiredState::Absent),
            &state(
                PluginDesiredState::Enabled,
                PluginObservedState::Ready,
                true,
            ),
        )
        .expect("selection");
        assert_eq!(
            selected,
            SelectedPlan::Package {
                action: PluginOperationAction::Uninstall
            }
        );
    }

    #[test]
    fn selects_upgrade_when_observed_package_digests_drift() {
        let assignment = assignment(PluginDesiredState::Enabled);
        let mut drifted = state(
            PluginDesiredState::Enabled,
            PluginObservedState::Ready,
            true,
        );
        drifted.version = Some("0.9.0".into());
        drifted.package_digest = Some(digest('9').as_str().into());
        drifted.manifest_digest = Some(digest('8').as_str().into());
        let selected = select_plan(&assignment, &drifted).expect("selection");
        assert_eq!(
            selected,
            SelectedPlan::Package {
                action: PluginOperationAction::Upgrade
            }
        );
    }

    #[test]
    fn selects_already_converged_when_desired_absent_and_package_removed() {
        let selected = select_plan(
            &assignment(PluginDesiredState::Absent),
            &state(
                PluginDesiredState::Absent,
                PluginObservedState::Removed,
                false,
            ),
        )
        .expect("selection");
        assert_eq!(selected, SelectedPlan::AlreadyConverged);
    }
}
