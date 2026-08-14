pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    GetNotification, GetNotificationHandler, ListNotifications, ListNotificationsHandler,
    MarkNotificationRead, MarkNotificationReadHandler, MarkNotificationReadResult,
    DEFAULT_NOTIFICATION_LIMIT, MAXIMUM_NOTIFICATION_LIMIT,
};
pub use domain::{
    INotificationRepository, IOutboundNotificationAdapter, MarkNotificationReadWrite, Notification,
    NotificationCursor, NotificationPage, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationDelivery, OutboundNotificationDeliveryError,
    OutboundNotificationDeliveryReceipt,
};
pub use infrastructure::{
    InMemoryNotificationRepository, OutboxNotificationProjector, PostgresNotificationRepository,
    SignedWebhookNotificationAdapter, SlackCompatibleNotificationAdapter,
};
pub use presentation::NotificationsModule;
