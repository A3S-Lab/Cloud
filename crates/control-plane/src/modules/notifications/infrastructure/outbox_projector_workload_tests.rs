use super::*;
use crate::modules::shared_kernel::domain::{DeploymentId, OperationId, WorkloadRevisionId};
use crate::modules::workloads::domain::events::{
    WorkloadDeploymentAvailabilityImpact, WorkloadDeploymentFailurePhase,
    WorkloadDeploymentHealthChanged, WorkloadDeploymentHealthStatus,
};

#[allow(clippy::too_many_arguments)]
fn workload_deployment_health_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    event_key: &str,
    status: WorkloadDeploymentHealthStatus,
    failure_phase: Option<WorkloadDeploymentFailurePhase>,
    availability_impact: Option<WorkloadDeploymentAvailabilityImpact>,
    aggregate_version: u64,
    occurred_at: DateTime<Utc>,
) -> OutboxMessage {
    let operation_id = OperationId::new();
    OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: workload_id.as_uuid(),
        aggregate_version,
        occurred_at,
        correlation_id: operation_id.as_uuid(),
        causation_id: None,
        payload: serde_json::to_value(WorkloadDeploymentHealthChanged {
            organization_id,
            project_id,
            environment_id,
            workload_id,
            workload_name: "checkout-api".into(),
            deployment_id: DeploymentId::new(),
            revision_id: WorkloadRevisionId::new(),
            revision_generation: aggregate_version,
            operation_id,
            node_id: Some(NodeId::new()),
            status,
            failure_phase,
            availability_impact,
        })
        .expect("Workload deployment health payload"),
        delivery_attempts: 1,
    }
}

#[tokio::test]
async fn workload_failures_and_recovery_are_logical_workload_projections() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let workload_id = WorkloadId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::WorkloadDeploymentHealthV1,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(organization_id, membership_id, recipient, created_at),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));

    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.failed",
            WorkloadDeploymentHealthStatus::Failed,
            Some(WorkloadDeploymentFailurePhase::Queued),
            Some(WorkloadDeploymentAvailabilityImpact::Unavailable),
            1,
            created_at - chrono::Duration::seconds(1),
        ))
        .await
        .expect("pre-policy failure is silent");
    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.healthy",
            WorkloadDeploymentHealthStatus::Healthy,
            None,
            None,
            2,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("initial health is silent");

    let retained_failure = workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        workload_id,
        "workload.deployment.failed",
        WorkloadDeploymentHealthStatus::Failed,
        Some(WorkloadDeploymentFailurePhase::Verifying),
        Some(WorkloadDeploymentAvailabilityImpact::PreviousRevisionRetained),
        3,
        created_at + chrono::Duration::seconds(2),
    );
    projector
        .project(&retained_failure)
        .await
        .expect("retained failure projects warning");
    projector
        .project(&retained_failure)
        .await
        .expect("retained failure replay is idempotent");

    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            WorkloadId::new(),
            "workload.deployment.healthy",
            WorkloadDeploymentHealthStatus::Healthy,
            None,
            None,
            4,
            created_at + chrono::Duration::seconds(3),
        ))
        .await
        .expect("another Workload cannot recover this failure");
    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.healthy",
            WorkloadDeploymentHealthStatus::Healthy,
            None,
            None,
            4,
            created_at + chrono::Duration::seconds(4),
        ))
        .await
        .expect("same Workload recovers after failure");
    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.healthy",
            WorkloadDeploymentHealthStatus::Healthy,
            None,
            None,
            5,
            created_at + chrono::Duration::seconds(5),
        ))
        .await
        .expect("routine health after recovery is silent");
    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.failed",
            WorkloadDeploymentHealthStatus::Failed,
            Some(WorkloadDeploymentFailurePhase::Scheduled),
            Some(WorkloadDeploymentAvailabilityImpact::Unavailable),
            6,
            created_at + chrono::Duration::seconds(6),
        ))
        .await
        .expect("unavailable failure projects critical alert");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("Workload notifications");
    assert_eq!(projected.len(), 3);
    assert_eq!(projected[0].severity, NotificationSeverity::Critical);
    assert_eq!(projected[0].title, "Workload deployment unavailable");
    assert_eq!(projected[0].source_aggregate_version, 6);
    assert_eq!(projected[1].severity, NotificationSeverity::Information);
    assert_eq!(projected[1].title, "Workload deployment recovered");
    assert_eq!(projected[1].source_aggregate_version, 4);
    assert_eq!(projected[2].severity, NotificationSeverity::Warning);
    assert_eq!(projected[2].title, "Workload deployment failed");
    assert_eq!(projected[2].source_aggregate_version, 3);
    assert!(projected
        .iter()
        .all(|notification| !notification.body.contains("provider-private")));
}

#[tokio::test]
async fn workload_recovery_requires_opt_in_and_active_policy() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let workload_id = WorkloadId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    let policy = create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::WorkloadDeploymentHealthV1,
        project_id,
        environment_id,
        false,
        created_at,
    )
    .await;
    let projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(organization_id, membership_id, recipient, created_at),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));

    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.failed",
            WorkloadDeploymentHealthStatus::Failed,
            Some(WorkloadDeploymentFailurePhase::Applying),
            Some(WorkloadDeploymentAvailabilityImpact::PreviousRevisionRetained),
            1,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("failure projects despite recovery opt-out");
    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.healthy",
            WorkloadDeploymentHealthStatus::Healthy,
            None,
            None,
            2,
            created_at + chrono::Duration::seconds(2),
        ))
        .await
        .expect("recovery opt-out is silent");
    revoke_alert_policy(
        notifications.as_ref(),
        &policy,
        created_at + chrono::Duration::seconds(3),
    )
    .await;
    projector
        .project(&workload_deployment_health_message(
            organization_id,
            project_id,
            environment_id,
            workload_id,
            "workload.deployment.failed",
            WorkloadDeploymentHealthStatus::Failed,
            Some(WorkloadDeploymentFailurePhase::Resolving),
            Some(WorkloadDeploymentAvailabilityImpact::Unavailable),
            3,
            created_at + chrono::Duration::seconds(4),
        ))
        .await
        .expect("revoked policy remains silent");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("Workload notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].source_event_key, "workload.deployment.failed");
    assert_eq!(projected[0].severity, NotificationSeverity::Warning);
}

#[tokio::test]
async fn workload_alerts_recheck_resource_grants() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::WorkloadDeploymentHealthV1,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let failure = workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        WorkloadId::new(),
        "workload.deployment.failed",
        WorkloadDeploymentHealthStatus::Failed,
        Some(WorkloadDeploymentFailurePhase::Queued),
        Some(WorkloadDeploymentAvailabilityImpact::Unavailable),
        1,
        created_at + chrono::Duration::seconds(1),
    );
    let membership = || {
        membership_lookup_with_role(
            organization_id,
            membership_id,
            recipient,
            MembershipRole::Restricted,
            true,
            created_at,
        )
    };

    let unauthorized = OutboxNotificationProjector::new(notifications.clone(), membership())
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    unauthorized
        .project(&failure)
        .await
        .expect("missing grant is ignored");
    assert!(notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("notifications")
        .is_empty());

    let grant = ResourceGrant::create(
        ResourceGrantId::new(),
        organization_id,
        membership_id,
        ResourceGrantScope::Environment {
            project_id,
            environment_id,
        },
        created_at,
    );
    let authorized = OutboxNotificationProjector::new(notifications.clone(), membership())
        .with_alert_policies(notifications.clone(), resource_grants(vec![grant]));
    authorized
        .project(&failure)
        .await
        .expect("matching grant projects alert");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].severity, NotificationSeverity::Critical);
}

#[tokio::test]
async fn workload_alerts_ignore_unregistered_schema_versions() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::WorkloadDeploymentHealthV1,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(organization_id, membership_id, recipient, created_at),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    let mut message = workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        WorkloadId::new(),
        "workload.deployment.failed",
        WorkloadDeploymentHealthStatus::Failed,
        Some(WorkloadDeploymentFailurePhase::Queued),
        Some(WorkloadDeploymentAvailabilityImpact::Unavailable),
        1,
        created_at + chrono::Duration::seconds(1),
    );
    message.schema_version = 2;

    projector
        .project(&message)
        .await
        .expect("unregistered schema version is ignored");
    assert!(notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("notifications")
        .is_empty());
}

#[test]
fn malformed_workload_deployment_health_payloads_fail_closed() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let workload_id = WorkloadId::new();
    let occurred_at = Utc::now();
    let message = workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        workload_id,
        "workload.deployment.failed",
        WorkloadDeploymentHealthStatus::Failed,
        Some(WorkloadDeploymentFailurePhase::Verifying),
        Some(WorkloadDeploymentAvailabilityImpact::PreviousRevisionRetained),
        2,
        occurred_at,
    );
    assert!(decode_workload_deployment_health(&message).is_ok());

    let mut unexpected = message.clone();
    unexpected.payload["providerPrivateFailure"] = serde_json::json!("secret");
    assert!(decode_workload_deployment_health(&unexpected).is_err());

    let mut wrong_subject = message.clone();
    wrong_subject.aggregate_id = Uuid::now_v7();
    assert!(decode_workload_deployment_health(&wrong_subject).is_err());

    let mut nil_subject = message.clone();
    nil_subject.aggregate_id = Uuid::nil();
    nil_subject.payload["workloadId"] = serde_json::json!(Uuid::nil());
    assert!(decode_workload_deployment_health(&nil_subject).is_err());

    let mut nil_organization = message.clone();
    nil_organization.organization_id = Uuid::nil();
    nil_organization.payload["organizationId"] = serde_json::json!(Uuid::nil());
    assert!(decode_workload_deployment_health(&nil_organization).is_err());

    let mut nil_event = message.clone();
    nil_event.event_id = Uuid::nil();
    assert!(decode_workload_deployment_health(&nil_event).is_err());

    let mut nil_causation = message.clone();
    nil_causation.causation_id = Some(Uuid::nil());
    assert!(decode_workload_deployment_health(&nil_causation).is_err());

    let mut wrong_generation = message.clone();
    wrong_generation.payload["revisionGeneration"] = serde_json::json!(3);
    assert!(decode_workload_deployment_health(&wrong_generation).is_err());

    let mut wrong_correlation = message.clone();
    wrong_correlation.correlation_id = Uuid::now_v7();
    assert!(decode_workload_deployment_health(&wrong_correlation).is_err());

    let mut noncanonical_name = message.clone();
    noncanonical_name.payload["workloadName"] = serde_json::json!("  checkout-api  ");
    assert!(decode_workload_deployment_health(&noncanonical_name).is_err());

    let mut wrong_status = message.clone();
    wrong_status.payload["status"] = serde_json::json!("healthy");
    wrong_status.payload["failurePhase"] = serde_json::Value::Null;
    wrong_status.payload["availabilityImpact"] = serde_json::Value::Null;
    assert!(decode_workload_deployment_health(&wrong_status).is_err());

    let mut missing_failure_detail = message.clone();
    missing_failure_detail.payload["failurePhase"] = serde_json::Value::Null;
    assert!(decode_workload_deployment_health(&missing_failure_detail).is_err());

    let mut nil_deployment = message.clone();
    nil_deployment.payload["deploymentId"] = serde_json::json!(Uuid::nil());
    assert!(decode_workload_deployment_health(&nil_deployment).is_err());

    let mut healthy_with_failure = workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        workload_id,
        "workload.deployment.healthy",
        WorkloadDeploymentHealthStatus::Healthy,
        None,
        None,
        3,
        occurred_at,
    );
    assert!(decode_workload_deployment_health(&healthy_with_failure).is_ok());
    healthy_with_failure.payload["availabilityImpact"] =
        serde_json::json!("previous_revision_retained");
    assert!(decode_workload_deployment_health(&healthy_with_failure).is_err());
}
