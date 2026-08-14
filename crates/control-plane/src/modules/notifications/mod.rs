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
    INotificationRepository, MarkNotificationReadWrite, Notification, NotificationCursor,
    NotificationPage, NotificationScope, NotificationSeverity,
};
pub use infrastructure::{
    InMemoryNotificationRepository, OutboxNotificationProjector, PostgresNotificationRepository,
};
pub use presentation::NotificationsModule;
