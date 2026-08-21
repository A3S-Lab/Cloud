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

export type OutboundNotificationChannel = 'signed_webhook' | 'slack_compatible';
export type OutboundNotificationSubscriptionState = 'active' | 'revoked';

export interface OutboundNotificationSubscription {
  organizationId: string;
  subscriptionId: string;
  channel: OutboundNotificationChannel;
  minimumSeverity: NotificationSeverity;
  connectorProjectId: string;
  connectorEnvironmentId: string;
  connectorProfileId: string;
  connectorRevisionId: string;
  maximumProviderAttempts: number;
  suppressBefore: string | null;
  definitionSchema:
    | 'cloud.notification.outbound-subscription.v1'
    | 'cloud.notification.outbound-subscription.v2'
    | 'cloud.notification.outbound-subscription.v3';
  definitionAcl: string;
  definitionDigest: string;
  state: OutboundNotificationSubscriptionState;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  revokedAt: string | null;
}

export interface OutboundNotificationSubscriptionPage {
  subscriptions: OutboundNotificationSubscription[];
  nextCursor: string | null;
}

export interface OutboundNotificationSubscriptionMutationResult {
  subscription: OutboundNotificationSubscription;
  replayed: boolean;
}

export interface OutboundNotificationSubscriptionQuery {
  cursor?: string;
  limit?: number;
}

export const DEFAULT_NOTIFICATION_LIMIT = 50;
export const MAX_NOTIFICATION_LIMIT = 200;
export const MAX_OUTBOUND_NOTIFICATION_SUBSCRIPTION_ACL_BYTES = 16 * 1024;

export function encodeNotificationQuery(query: NotificationQuery = {}): URLSearchParams {
  const parameters = new URLSearchParams();
  if (query.unreadOnly !== undefined) {
    parameters.set('unreadOnly', String(query.unreadOnly));
  }
  return encodePersonalPageQuery(parameters, query, 'notification');
}

export function encodeOutboundNotificationSubscriptionQuery(
  query: OutboundNotificationSubscriptionQuery = {}
): URLSearchParams {
  return encodePersonalPageQuery(new URLSearchParams(), query, 'outbound notification subscription');
}

export function validateNotificationId(value: string): void {
  validateNonNilUuid(value, 'notification ID');
}

export function validateExpectedNotificationVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('expected notification version must be a positive safe integer');
  }
}

export function validateOutboundNotificationSubscriptionId(value: string): void {
  validateNonNilUuid(value, 'outbound notification subscription ID');
}

export function validateExpectedOutboundNotificationSubscriptionVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError(
      'expected outbound notification subscription version must be a positive safe integer'
    );
  }
}

export function validateOutboundNotificationSubscriptionAcl(acl: string): void {
  const byteLength = new TextEncoder().encode(acl).byteLength;
  if (byteLength < 1 || byteLength > MAX_OUTBOUND_NOTIFICATION_SUBSCRIPTION_ACL_BYTES) {
    throw new RangeError(
      `outbound notification subscription ACL must contain between 1 and ${MAX_OUTBOUND_NOTIFICATION_SUBSCRIPTION_ACL_BYTES} UTF-8 bytes`
    );
  }
}

function encodePersonalPageQuery(
  parameters: URLSearchParams,
  query: OutboundNotificationSubscriptionQuery,
  label: string
): URLSearchParams {
  if (query.cursor !== undefined) {
    if (query.cursor.length === 0 || query.cursor.length > 128 || /[\0\r\n]/.test(query.cursor)) {
      throw new TypeError(`${label} cursor is invalid`);
    }
    parameters.set('cursor', query.cursor);
  }
  const limit = query.limit ?? DEFAULT_NOTIFICATION_LIMIT;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_NOTIFICATION_LIMIT) {
    throw new RangeError(`${label} limit must be between 1 and ${MAX_NOTIFICATION_LIMIT}`);
  }
  parameters.set('limit', String(limit));
  return parameters;
}
