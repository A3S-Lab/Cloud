pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CreateOutboundNotificationSubscription, CreateOutboundNotificationSubscriptionHandler,
    GetNotification, GetNotificationHandler, GetOutboundNotificationSubscription,
    GetOutboundNotificationSubscriptionHandler, IOutboundNotificationDispatcher, ListNotifications,
    ListNotificationsHandler, ListOutboundNotificationSubscriptions,
    ListOutboundNotificationSubscriptionsHandler, MarkNotificationRead,
    MarkNotificationReadHandler, MarkNotificationReadResult, OutboundNotificationDispatchResult,
    OutboundNotificationDispatcher, OutboundNotificationSubscriptionMutationResult,
    RevokeOutboundNotificationSubscription, RevokeOutboundNotificationSubscriptionHandler,
    DEFAULT_NOTIFICATION_LIMIT, MAXIMUM_NOTIFICATION_LIMIT,
};
pub use domain::{
    outbound_notification_attempt_id, CreateOutboundNotificationSubscriptionWrite,
    INotificationRepository, IOutboundNotificationDeliveryRepository,
    IOutboundNotificationRepository, IOutboundNotificationRequestAdapter,
    MarkNotificationReadWrite, Notification, NotificationCursor, NotificationPage,
    NotificationScope, NotificationSeverity, OutboundNotificationChannel,
    OutboundNotificationConnectorTarget, OutboundNotificationDelivery,
    OutboundNotificationDeliveryAdmission, OutboundNotificationRequestError,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionCursor,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionEvent,
    OutboundNotificationSubscriptionPage, OutboundNotificationSubscriptionSpec,
    OutboundNotificationTerminalOutcome, OutboundNotificationTerminalReceipt,
    RevokeOutboundNotificationSubscriptionWrite, MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS, OUTBOUND_NOTIFICATION_EVENT_KEY,
    OUTBOUND_NOTIFICATION_SCHEMA, OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA,
};
pub use infrastructure::{
    A3sEventOutboundNotificationConsumer, InMemoryNotificationRepository,
    OutboxNotificationProjector, PostgresNotificationRepository, SignedWebhookNotificationAdapter,
    SlackCompatibleNotificationAdapter,
};
pub use presentation::NotificationsModule;
