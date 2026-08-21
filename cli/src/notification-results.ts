import type {
  Notification,
  NotificationAlertPolicy,
  NotificationAlertPolicyMutationResult,
  NotificationAlertPolicyPage,
  NotificationMutationResult,
  NotificationPage,
  OutboundNotificationSubscription,
  OutboundNotificationSubscriptionMutationResult,
  OutboundNotificationSubscriptionPage,
} from '@a3s/cloud-client';
import { renderTable, sanitizeCell, type TableColumn } from './output';
import type { CommandResult } from './results';

const NOTIFICATION_COLUMNS: readonly TableColumn<Notification>[] = [
  { header: 'OCCURRED AT', value: (row) => row.occurredAt },
  { header: 'SEVERITY', value: (row) => row.severity },
  { header: 'TITLE', value: (row) => row.title },
  { header: 'SOURCE', value: (row) => row.sourceEventKey },
  { header: 'READ AT', value: (row) => row.readAt ?? '' },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'ID', value: (row) => row.id },
];

export function notificationsResult(page: NotificationPage): CommandResult {
  const table = renderTable(page.notifications, NOTIFICATION_COLUMNS);
  return {
    json: page,
    table: page.nextCursor ? `${table}Next cursor: ${sanitizeCell(page.nextCursor)}\n` : table,
  };
}

export function notificationResult(notification: Notification): CommandResult {
  return { json: notification, table: renderTable([notification], NOTIFICATION_COLUMNS) };
}

export function notificationMutationResult(result: NotificationMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.notification, replayed: result.replayed }],
      [...NOTIFICATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

const ALERT_POLICY_COLUMNS: readonly TableColumn<NotificationAlertPolicy>[] = [
  { header: 'CREATED AT', value: (row) => row.createdAt },
  { header: 'SOURCE', value: (row) => row.source },
  { header: 'PROJECT ID', value: (row) => row.projectId },
  { header: 'ENVIRONMENT ID', value: (row) => row.environmentId },
  { header: 'RECOVERY', value: (row) => row.notifyOnRecovery },
  { header: 'STATE', value: (row) => row.state },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'POLICY ID', value: (row) => row.policyId },
];

export function notificationAlertPoliciesResult(page: NotificationAlertPolicyPage): CommandResult {
  const table = renderTable(page.policies, ALERT_POLICY_COLUMNS);
  return {
    json: page,
    table: page.nextCursor ? `${table}Next cursor: ${sanitizeCell(page.nextCursor)}\n` : table,
  };
}

export function notificationAlertPolicyResult(policy: NotificationAlertPolicy): CommandResult {
  return { json: policy, table: renderTable([policy], ALERT_POLICY_COLUMNS) };
}

export function notificationAlertPolicyMutationResult(
  result: NotificationAlertPolicyMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.policy, replayed: result.replayed }],
      [...ALERT_POLICY_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

const OUTBOUND_SUBSCRIPTION_COLUMNS: readonly TableColumn<OutboundNotificationSubscription>[] = [
  { header: 'CREATED AT', value: (row) => row.createdAt },
  { header: 'CHANNEL', value: (row) => row.channel },
  { header: 'MIN SEVERITY', value: (row) => row.minimumSeverity },
  { header: 'ATTEMPTS', value: (row) => row.maximumProviderAttempts },
  { header: 'SUPPRESS BEFORE', value: (row) => row.suppressBefore ?? '' },
  { header: 'STATE', value: (row) => row.state },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'SUBSCRIPTION ID', value: (row) => row.subscriptionId },
  { header: 'CONNECTOR REVISION', value: (row) => row.connectorRevisionId },
];

export function outboundNotificationSubscriptionsResult(
  page: OutboundNotificationSubscriptionPage
): CommandResult {
  const table = renderTable(page.subscriptions, OUTBOUND_SUBSCRIPTION_COLUMNS);
  return {
    json: page,
    table: page.nextCursor ? `${table}Next cursor: ${sanitizeCell(page.nextCursor)}\n` : table,
  };
}

export function outboundNotificationSubscriptionResult(
  subscription: OutboundNotificationSubscription
): CommandResult {
  return {
    json: subscription,
    table: renderTable([subscription], OUTBOUND_SUBSCRIPTION_COLUMNS),
  };
}

export function outboundNotificationSubscriptionMutationResult(
  result: OutboundNotificationSubscriptionMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.subscription, replayed: result.replayed }],
      [...OUTBOUND_SUBSCRIPTION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}
