pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    GetNotification, GetNotificationHandler, IOutboundNotificationDispatcher, ListNotifications,
    ListNotificationsHandler, MarkNotificationRead, MarkNotificationReadHandler,
    MarkNotificationReadResult, OutboundNotificationDispatchResult, OutboundNotificationDispatcher,
    DEFAULT_NOTIFICATION_LIMIT, MAXIMUM_NOTIFICATION_LIMIT,
    MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION,
};
pub use domain::{
    INotificationRepository, IOutboundNotificationRequestAdapter, MarkNotificationReadWrite,
    Notification, NotificationCursor, NotificationPage, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationConnectorTarget, OutboundNotificationDelivery,
    OutboundNotificationRequestError, OUTBOUND_NOTIFICATION_EVENT_KEY,
    OUTBOUND_NOTIFICATION_SCHEMA,
};
pub use infrastructure::{
    A3sEventOutboundNotificationConsumer, InMemoryNotificationRepository,
    OutboxNotificationProjector, PostgresNotificationRepository, SignedWebhookNotificationAdapter,
    SlackCompatibleNotificationAdapter,
};
pub use presentation::NotificationsModule;
