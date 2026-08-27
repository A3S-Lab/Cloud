import {
  type CloudApi,
  DEFAULT_NOTIFICATION_LIMIT,
  encodeNotificationAlertPolicyQuery,
  encodeNotificationQuery,
  encodeOutboundNotificationSubscriptionQuery,
  MAX_NOTIFICATION_ALERT_POLICY_ACL_BYTES,
  MAX_NOTIFICATION_LIMIT,
  MAX_OUTBOUND_NOTIFICATION_SUBSCRIPTION_ACL_BYTES,
  type NotificationAlertPolicyQuery,
  type NotificationQuery,
  type OutboundNotificationSubscriptionQuery,
} from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  requireArity,
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization } from './context';
import { inputValidationUsageError, usageError } from './errors';
import {
  notificationMutationResult,
  notificationAlertPoliciesResult,
  notificationAlertPolicyMutationResult,
  notificationAlertPolicyResult,
  notificationResult,
  notificationsResult,
  outboundNotificationSubscriptionMutationResult,
  outboundNotificationSubscriptionResult,
  outboundNotificationSubscriptionsResult,
} from './notification-results';
import type { CommandResult } from './results';

const NOTIFICATION_LIST_COMMAND = 'notifications list';

export async function executeNotificationCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: { readFile?: (path: string) => Promise<Uint8Array> } = {}
): Promise<CommandResult | undefined> {
  if (
    !command.startsWith('notifications ') &&
    !command.startsWith('notification-alert-policies ') &&
    !command.startsWith('notification-subscriptions ')
  ) {
    return undefined;
  }
  const organizationId = requireOrganization(context);
  switch (command) {
    case 'notifications list': {
      requireArity(arguments_.positionals, 2, 'notifications list');
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (arguments_.expectedVersion !== undefined || arguments_.stream !== undefined) {
        throw usageError('--expected-version and --stream are not valid for notifications list');
      }
      const query: NotificationQuery = {
        unreadOnly: arguments_.unreadOnly,
        cursor: arguments_.cursor,
        limit: boundedLimit(arguments_.limit, 'notification'),
      };
      try {
        encodeNotificationQuery(query);
      } catch (error) {
        throw inputValidationUsageError(error);
      }
      return notificationsResult(await cloudApi().listNotifications(organizationId, query));
    }
    case 'notifications get': {
      requireReadCommand(arguments_, 'notifications get <notification-id>');
      if (arguments_.unreadOnly) {
        throw usageError('--unread-only is valid only for notifications list');
      }
      return notificationResult(
        await cloudApi().getNotification(
          organizationId,
          positionalUuid(arguments_.positionals, 2, 'notification ID')
        )
      );
    }
    case 'notification-alert-policies list': {
      requireArity(arguments_.positionals, 2, 'notification-alert-policies list');
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (arguments_.expectedVersion !== undefined || arguments_.stream !== undefined) {
        throw usageError(
          '--expected-version and --stream are not valid for notification-alert-policies list'
        );
      }
      const query: NotificationAlertPolicyQuery = {
        cursor: arguments_.cursor,
        limit: boundedLimit(arguments_.limit, 'notification alert policy'),
      };
      try {
        encodeNotificationAlertPolicyQuery(query);
      } catch (error) {
        throw inputValidationUsageError(error);
      }
      return notificationAlertPoliciesResult(
        await cloudApi().listNotificationAlertPolicies(organizationId, query)
      );
    }
    case 'notification-alert-policies get':
      requireReadCommand(arguments_, 'notification-alert-policies get <policy-id>');
      return notificationAlertPolicyResult(
        await cloudApi().getNotificationAlertPolicy(
          organizationId,
          positionalUuid(arguments_.positionals, 2, 'notification alert policy ID')
        )
      );
    case 'notification-alert-policies create': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'notification-alert-policies create');
      const definitionAcl = await readAclDocument(
        mutation.file,
        {
          label: 'notification alert policy ACL',
          maximumBytes: MAX_NOTIFICATION_ALERT_POLICY_ACL_BYTES,
        },
        dependencies.readFile
      );
      return notificationAlertPolicyMutationResult(
        await cloudApi().createNotificationAlertPolicy(organizationId, definitionAcl, mutation.idempotencyKey)
      );
    }
    case 'notification-alert-policies revoke': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'notification-alert-policies revoke <policy-id>',
        'notification alert policy'
      );
      return notificationAlertPolicyMutationResult(
        await cloudApi().revokeNotificationAlertPolicy(
          organizationId,
          positionalUuid(arguments_.positionals, 2, 'notification alert policy ID'),
          mutation.expectedVersion,
          mutation.idempotencyKey
        )
      );
    }
    case 'notification-subscriptions list': {
      requireArity(arguments_.positionals, 2, 'notification-subscriptions list');
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (arguments_.expectedVersion !== undefined || arguments_.stream !== undefined) {
        throw usageError('--expected-version and --stream are not valid for notification-subscriptions list');
      }
      const query: OutboundNotificationSubscriptionQuery = {
        cursor: arguments_.cursor,
        limit: boundedLimit(arguments_.limit, 'outbound notification subscription'),
      };
      try {
        encodeOutboundNotificationSubscriptionQuery(query);
      } catch (error) {
        throw inputValidationUsageError(error);
      }
      return outboundNotificationSubscriptionsResult(
        await cloudApi().listOutboundNotificationSubscriptions(organizationId, query)
      );
    }
    case 'notification-subscriptions get':
      requireReadCommand(arguments_, 'notification-subscriptions get <subscription-id>');
      return outboundNotificationSubscriptionResult(
        await cloudApi().getOutboundNotificationSubscription(
          organizationId,
          positionalUuid(arguments_.positionals, 2, 'outbound notification subscription ID')
        )
      );
    case 'notification-subscriptions create': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'notification-subscriptions create');
      const definitionAcl = await readAclDocument(
        mutation.file,
        {
          label: 'outbound notification subscription ACL',
          maximumBytes: MAX_OUTBOUND_NOTIFICATION_SUBSCRIPTION_ACL_BYTES,
        },
        dependencies.readFile
      );
      return outboundNotificationSubscriptionMutationResult(
        await cloudApi().createOutboundNotificationSubscription(
          organizationId,
          definitionAcl,
          mutation.idempotencyKey
        )
      );
    }
    case 'notification-subscriptions revoke': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'notification-subscriptions revoke <subscription-id>',
        'outbound notification subscription'
      );
      return outboundNotificationSubscriptionMutationResult(
        await cloudApi().revokeOutboundNotificationSubscription(
          organizationId,
          positionalUuid(arguments_.positionals, 2, 'outbound notification subscription ID'),
          mutation.expectedVersion,
          mutation.idempotencyKey
        )
      );
    }
    case 'notifications read': {
      if (arguments_.unreadOnly) {
        throw usageError('--unread-only is valid only for notifications list');
      }
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'notifications read <notification-id>',
        'notification'
      );
      return notificationMutationResult(
        await cloudApi().markNotificationRead(
          organizationId,
          positionalUuid(arguments_.positionals, 2, 'notification ID'),
          mutation.expectedVersion,
          mutation.idempotencyKey
        )
      );
    }
    default:
      throw usageError(`unknown notification command: ${command}`);
  }
}

export function rejectMisplacedNotificationOptions(command: string, arguments_: ParsedArguments): void {
  if (arguments_.unreadOnly && command !== NOTIFICATION_LIST_COMMAND) {
    throw usageError('--unread-only is valid only for notifications list');
  }
}

function boundedLimit(value: string | undefined, label: string): number {
  if (value === undefined) {
    return DEFAULT_NOTIFICATION_LIMIT;
  }
  if (!/^[0-9]+$/u.test(value)) {
    throw usageError(`${label} limit must be an integer`);
  }
  const limit = Number(value);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_NOTIFICATION_LIMIT) {
    throw usageError(`${label} limit must be between 1 and ${MAX_NOTIFICATION_LIMIT}`);
  }
  return limit;
}
