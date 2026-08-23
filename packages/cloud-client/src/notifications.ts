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

export type NotificationAlertSource =
  | 'edge.domain-claim-status.v1'
  | 'edge.gateway-certificate-renewal-status.v1'
  | 'workload.deployment-health.v1'
  | 'edge.gateway-certificate-expiry-status.v1'
  | 'fleet.node-availability-status.v1';
export type NotificationAlertPolicyState = 'active' | 'revoked';

export interface NotificationAlertPolicyEnvironmentTarget {
  kind: 'environment';
  projectId: string;
  environmentId: string;
}

export interface NotificationAlertPolicyNodeTarget {
  kind: 'node';
  nodeId: string;
}

export type NotificationAlertPolicyTarget =
  | NotificationAlertPolicyEnvironmentTarget
  | NotificationAlertPolicyNodeTarget;

export interface NotificationAlertPolicy {
  organizationId: string;
  policyId: string;
  source: NotificationAlertSource;
  target: NotificationAlertPolicyTarget;
  /** @deprecated Use target. Null for Node policies. */
  projectId: string | null;
  /** @deprecated Use target. Null for Node policies. */
  environmentId: string | null;
  notifyOnRecovery: boolean;
  definitionSchema: 'cloud.notification.alert-policy.v1' | 'cloud.notification.alert-policy.v2';
  definitionAcl: string;
  definitionDigest: string;
  state: NotificationAlertPolicyState;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  revokedAt: string | null;
}

export interface NotificationAlertPolicyPage {
  policies: NotificationAlertPolicy[];
  nextCursor: string | null;
}

export interface NotificationAlertPolicyMutationResult {
  policy: NotificationAlertPolicy;
  replayed: boolean;
}

export interface NotificationAlertPolicyQuery {
  cursor?: string;
  limit?: number;
}

type PersonalPageQuery = {
  cursor?: string;
  limit?: number;
};

export type OutboundNotificationChannel = 'signed_webhook' | 'slack_compatible' | 'smtp';
export type OutboundNotificationSubscriptionState = 'active' | 'revoked';

export interface OutboundNotificationConnectorTarget {
  kind: 'connector';
  projectId: string;
  environmentId: string;
  profileId: string;
  revisionId: string;
}

export interface OutboundNotificationRecipientContactTarget {
  kind: 'recipient_contact';
  recipientContactId: string;
}

export type OutboundNotificationTarget =
  | OutboundNotificationConnectorTarget
  | OutboundNotificationRecipientContactTarget;

export interface OutboundNotificationSubscription {
  organizationId: string;
  subscriptionId: string;
  channel: OutboundNotificationChannel;
  minimumSeverity: NotificationSeverity;
  target: OutboundNotificationTarget;
  /** @deprecated Use target. Null for SMTP subscriptions. */
  connectorProjectId: string | null;
  /** @deprecated Use target. Null for SMTP subscriptions. */
  connectorEnvironmentId: string | null;
  /** @deprecated Use target. Null for SMTP subscriptions. */
  connectorProfileId: string | null;
  /** @deprecated Use target. Null for SMTP subscriptions. */
  connectorRevisionId: string | null;
  maximumProviderAttempts: number;
  suppressBefore: string | null;
  definitionSchema:
    | 'cloud.notification.outbound-subscription.v1'
    | 'cloud.notification.outbound-subscription.v2'
    | 'cloud.notification.outbound-subscription.v3'
    | 'cloud.notification.outbound-subscription.v4';
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
export const MAX_NOTIFICATION_ALERT_POLICY_ACL_BYTES = 16 * 1024;
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

export function encodeNotificationAlertPolicyQuery(
  query: NotificationAlertPolicyQuery = {}
): URLSearchParams {
  return encodePersonalPageQuery(new URLSearchParams(), query, 'notification alert policy');
}

export function validateNotificationId(value: string): void {
  validateNonNilUuid(value, 'notification ID');
}

export function validateExpectedNotificationVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('expected notification version must be a positive safe integer');
  }
}

export function validateNotificationAlertPolicyId(value: string): void {
  validateNonNilUuid(value, 'notification alert policy ID');
}

export function validateExpectedNotificationAlertPolicyVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('expected notification alert policy version must be a positive safe integer');
  }
}

export function validateNotificationAlertPolicyAcl(acl: string): void {
  const byteLength = new TextEncoder().encode(acl).byteLength;
  if (byteLength < 1 || byteLength > MAX_NOTIFICATION_ALERT_POLICY_ACL_BYTES) {
    throw new RangeError(
      `notification alert policy ACL must contain between 1 and ${MAX_NOTIFICATION_ALERT_POLICY_ACL_BYTES} UTF-8 bytes`
    );
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
  query: PersonalPageQuery,
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
