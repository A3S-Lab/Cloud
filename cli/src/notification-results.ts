import type { Notification, NotificationMutationResult, NotificationPage } from '@a3s/cloud-client';
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
