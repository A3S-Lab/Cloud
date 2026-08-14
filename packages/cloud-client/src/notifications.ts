import { validateNonNilUuid } from './validation';

export type NotificationSeverity = 'information' | 'warning' | 'critical';

export type NotificationScope =
  | { kind: 'organization' }
  | { kind: 'project'; projectId: string }
  | { kind: 'environment'; projectId: string; environmentId: string }
  | { kind: 'node'; nodeId: string };

export interface Notification {
  id: string;
  organizationId: string;
  sourceEventId: string;
  sourceEventKey: string;
  sourceAggregateId: string;
  severity: NotificationSeverity;
  title: string;
  body: string;
  scope: NotificationScope;
  occurredAt: string;
  deliveredAt: string;
  aggregateVersion: number;
  readAt: string | null;
}

export interface NotificationPage {
  notifications: Notification[];
  nextCursor: string | null;
}

export interface NotificationMutationResult {
  notification: Notification;
  replayed: boolean;
}

export interface NotificationQuery {
  unreadOnly?: boolean;
  cursor?: string;
  limit?: number;
}

export const DEFAULT_NOTIFICATION_LIMIT = 50;
export const MAX_NOTIFICATION_LIMIT = 200;

export function encodeNotificationQuery(query: NotificationQuery = {}): URLSearchParams {
  const parameters = new URLSearchParams();
  if (query.unreadOnly !== undefined) {
    parameters.set('unreadOnly', String(query.unreadOnly));
  }
  if (query.cursor !== undefined) {
    if (query.cursor.length === 0 || query.cursor.length > 128 || /[\0\r\n]/.test(query.cursor)) {
      throw new TypeError('notification cursor is invalid');
    }
    parameters.set('cursor', query.cursor);
  }
  const limit = query.limit ?? DEFAULT_NOTIFICATION_LIMIT;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_NOTIFICATION_LIMIT) {
    throw new RangeError(`notification limit must be between 1 and ${MAX_NOTIFICATION_LIMIT}`);
  }
  parameters.set('limit', String(limit));
  return parameters;
}

export function validateNotificationId(value: string): void {
  validateNonNilUuid(value, 'notification ID');
}

export function validateExpectedNotificationVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('expected notification version must be a positive safe integer');
  }
}
