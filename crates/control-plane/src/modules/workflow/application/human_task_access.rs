use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::PrincipalId;
use crate::modules::workflow::domain::{AssignmentPolicyRef, HumanTaskRecord};

/// Projects the only assignment policy currently understood by the public Cloud surface.
///
/// Workflow owns task assignment semantics. Identity's shared evaluator proves project access;
/// this boundary refuses unknown policy revisions instead of silently treating them as the
/// built-in organization-member policy. The request-bound Form interaction is visible only to
/// the current claimant.
pub(crate) fn public_record(
    mut record: HumanTaskRecord,
    actor_principal_id: Option<PrincipalId>,
) -> ApplicationResult<HumanTaskRecord> {
    ensure_supported_assignment_policy(&record)?;
    if record.task.claimed_by != actor_principal_id {
        record.interaction_request = None;
    }
    Ok(record)
}

pub(crate) fn ensure_supported_assignment_policy(
    record: &HumanTaskRecord,
) -> ApplicationResult<()> {
    let supported = AssignmentPolicyRef::workflow_organization_member_exclusive()
        .map_err(ApplicationError::Internal)?;
    if record.task.assignment_policy != supported {
        return Err(ApplicationError::Unavailable(
            "HumanTask assignment policy is not supported by this public API".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::test_support::{pending_task, timestamp};
    use crate::modules::workflow::{HumanTaskInteractionSpec, HumanTaskRecord};
    use uuid::Uuid;

    fn record(claimed: bool) -> (HumanTaskRecord, PrincipalId) {
        let (mut task, principal_id) = pending_task();
        task.assignment_policy =
            AssignmentPolicyRef::workflow_organization_member_exclusive().expect("built-in policy");
        let mut record = HumanTaskRecord::create(
            task,
            HumanTaskInteractionSpec::approval("Review", None, None).expect("interaction"),
            1,
            Uuid::now_v7(),
        )
        .expect("record");
        if claimed {
            record.activate(1, timestamp(8, 1)).expect("activation");
            record
                .claim(2, principal_id, timestamp(8, 2))
                .expect("claim");
        }
        (record, principal_id)
    }

    #[test]
    fn only_the_current_claimant_receives_the_form_interaction_request() {
        let (record, claimant) = record(true);
        assert!(public_record(record.clone(), Some(claimant))
            .expect("claimant view")
            .interaction_request
            .is_some());
        assert!(public_record(record.clone(), Some(PrincipalId::new()))
            .expect("other member view")
            .interaction_request
            .is_none());
        assert!(public_record(record, None)
            .expect("summary view")
            .interaction_request
            .is_none());
    }

    #[test]
    fn unknown_assignment_policies_fail_closed() {
        let (task, _) = pending_task();
        let record = HumanTaskRecord::create(
            task,
            HumanTaskInteractionSpec::approval("Review", None, None).expect("interaction"),
            1,
            Uuid::now_v7(),
        )
        .expect("record");
        assert!(matches!(
            public_record(record, None),
            Err(ApplicationError::Unavailable(_))
        ));
    }
}
