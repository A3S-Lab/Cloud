mod alert_policy;
mod alert_policy_repository;
mod notification;
mod notification_repository;
mod outbound_adapter;
mod outbound_delivery;
mod outbound_receipt;
mod outbound_repository;
mod outbound_smtp_attempt;
mod outbound_smtp_repository;
mod outbound_smtp_service;
mod outbound_subscription;

pub use alert_policy::{
    NotificationAlertPolicy, NotificationAlertPolicyCursor, NotificationAlertPolicyDefinition,
    NotificationAlertPolicyPage, NotificationAlertPolicySpec, NotificationAlertPolicyTarget,
    NotificationAlertSource, NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES,
    NOTIFICATION_ALERT_POLICY_SCHEMA, NOTIFICATION_ALERT_POLICY_SCHEMA_V2,
};
pub use alert_policy_repository::{
    CreateNotificationAlertPolicyWrite, INotificationAlertPolicyRepository,
    NotificationAlertPolicyEvent, RevokeNotificationAlertPolicyWrite,
};
pub use notification::{
    Notification, NotificationCursor, NotificationPage, NotificationScope, NotificationSeverity,
};
pub use notification_repository::{INotificationRepository, MarkNotificationReadWrite};
pub use outbound_adapter::{IOutboundNotificationRequestAdapter, OutboundNotificationRequestError};
pub use outbound_delivery::{
    outbound_notification_attempt_id, OutboundNotificationChannel,
    OutboundNotificationConnectorTarget, OutboundNotificationDelivery, OutboundNotificationTarget,
    MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS, OUTBOUND_NOTIFICATION_EVENT_KEY,
    OUTBOUND_NOTIFICATION_SCHEMA, OUTBOUND_NOTIFICATION_SCHEMA_V2, OUTBOUND_NOTIFICATION_SCHEMA_V3,
};
pub use outbound_receipt::{
    OutboundNotificationTerminalOutcome, OutboundNotificationTerminalReceipt,
};
pub use outbound_repository::{
    CreateOutboundNotificationSubscriptionWrite, IOutboundNotificationDeliveryRepository,
    IOutboundNotificationRepository, OutboundNotificationDeliveryAdmission,
    OutboundNotificationSubscriptionEvent, RevokeOutboundNotificationSubscriptionWrite,
};
pub use outbound_smtp_attempt::{
    outbound_notification_smtp_attempt_id, OutboundNotificationSmtpAttemptOutcome,
    OutboundNotificationSmtpAttemptRecord, OutboundNotificationSmtpAttemptState,
    MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS,
    MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_OUTCOME_SECONDS,
};
pub use outbound_smtp_repository::{
    IOutboundNotificationSmtpAttemptRepository, OutboundNotificationSmtpAttemptAdmission,
    OutboundNotificationSmtpAttemptSettlement, OutboundNotificationSmtpDispatchStart,
};
pub use outbound_smtp_service::{
    IOutboundNotificationSmtpDeliveryService, IPreparedOutboundNotificationSmtpDelivery,
    OutboundNotificationSmtpPreparationError, OutboundNotificationSmtpProviderOutcome,
};
pub use outbound_subscription::{
    OutboundNotificationSubscription, OutboundNotificationSubscriptionCursor,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionPage,
    OutboundNotificationSubscriptionSpec, MAXIMUM_OUTBOUND_NOTIFICATION_SUPPRESSION_DAYS,
    MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V4,
};
