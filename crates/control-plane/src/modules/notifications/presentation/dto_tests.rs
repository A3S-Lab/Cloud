use super::*;
use crate::modules::notifications::OutboundNotificationSubscriptionDefinition;
use crate::modules::shared_kernel::domain::{
    NotificationSubscriptionId, OrganizationId, PrincipalId, RecipientContactId,
};
use serde_json::json;

#[test]
fn smtp_subscription_response_exposes_only_the_closed_contact_target() {
    let organization_id = OrganizationId::new();
    let principal_id = PrincipalId::new();
    let contact_id = RecipientContactId::new();
    let definition = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
        contact_id,
        NotificationSeverity::Warning,
        3,
        None,
    )
    .expect("SMTP definition");
    let subscription = OutboundNotificationSubscription::create(
        organization_id,
        NotificationSubscriptionId::new(),
        principal_id,
        definition,
        principal_id,
        Utc::now(),
    )
    .expect("SMTP subscription");

    let response =
        serde_json::to_value(OutboundNotificationSubscriptionResponse::from(subscription))
            .expect("serialized response");

    assert_eq!(
        response["target"],
        json!({
            "kind": "recipient_contact",
            "recipientContactId": contact_id,
        })
    );
    assert_eq!(response["channel"], "smtp");
    for obsolete in [
        "connectorProjectId",
        "connectorEnvironmentId",
        "connectorProfileId",
        "connectorRevisionId",
        "recipientContactId",
    ] {
        assert!(response.get(obsolete).is_none());
    }
}
