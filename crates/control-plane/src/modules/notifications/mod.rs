pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CreateNotificationAlertPolicy, CreateNotificationAlertPolicyHandler,
    CreateOutboundNotificationSubscription, CreateOutboundNotificationSubscriptionHandler,
    GetNotification, GetNotificationAlertPolicy, GetNotificationAlertPolicyHandler,
    GetNotificationHandler, GetOutboundNotificationSubscription,
    GetOutboundNotificationSubscriptionHandler, IOutboundNotificationDispatcher,
    ListNotificationAlertPolicies, ListNotificationAlertPoliciesHandler, ListNotifications,
    ListNotificationsHandler, ListOutboundNotificationSubscriptions,
    ListOutboundNotificationSubscriptionsHandler, MarkNotificationRead,
    MarkNotificationReadHandler, MarkNotificationReadResult, NotificationAlertPolicyMutationResult,
    OutboundNotificationDispatchResult, OutboundNotificationDispatcher,
    OutboundNotificationSubscriptionMutationResult, RevokeNotificationAlertPolicy,
    RevokeNotificationAlertPolicyHandler, RevokeOutboundNotificationSubscription,
    RevokeOutboundNotificationSubscriptionHandler, DEFAULT_NOTIFICATION_LIMIT,
    MAXIMUM_NOTIFICATION_LIMIT,
};
pub use domain::{
    outbound_notification_attempt_id, CreateNotificationAlertPolicyWrite,
    CreateOutboundNotificationSubscriptionWrite, INotificationAlertPolicyRepository,
    INotificationRepository, IOutboundNotificationDeliveryRepository,
    IOutboundNotificationRepository, IOutboundNotificationRequestAdapter,
    MarkNotificationReadWrite, Notification, NotificationAlertPolicy,
    NotificationAlertPolicyCursor, NotificationAlertPolicyDefinition, NotificationAlertPolicyEvent,
    NotificationAlertPolicyPage, NotificationAlertPolicySpec, NotificationAlertSource,
    NotificationCursor, NotificationPage, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationConnectorTarget, OutboundNotificationDelivery,
    OutboundNotificationDeliveryAdmission, OutboundNotificationRequestError,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionCursor,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionEvent,
    OutboundNotificationSubscriptionPage, OutboundNotificationSubscriptionSpec,
    OutboundNotificationTerminalOutcome, OutboundNotificationTerminalReceipt,
    RevokeNotificationAlertPolicyWrite, RevokeOutboundNotificationSubscriptionWrite,
    MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
    MAXIMUM_OUTBOUND_NOTIFICATION_SUPPRESSION_DAYS,
    MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS, NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES,
    NOTIFICATION_ALERT_POLICY_SCHEMA, OUTBOUND_NOTIFICATION_EVENT_KEY,
    OUTBOUND_NOTIFICATION_SCHEMA, OUTBOUND_NOTIFICATION_SCHEMA_V2,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3,
};
pub use infrastructure::{
    A3sEventOutboundNotificationConsumer, InMemoryNotificationRepository,
    OutboxNotificationProjector, PostgresNotificationRepository, SignedWebhookNotificationAdapter,
    SlackCompatibleNotificationAdapter,
};
pub use presentation::NotificationsModule;
