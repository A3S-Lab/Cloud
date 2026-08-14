import {
  DEFAULT_NOTIFICATION_LIMIT,
  MAX_NOTIFICATION_LIMIT,
  type CloudApi,
  type NotificationQuery,
  encodeNotificationQuery,
} from '@a3s/cloud-client';
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
import { usageError } from './errors';
import { notificationMutationResult, notificationResult, notificationsResult } from './notification-results';
import type { CommandResult } from './results';

const NOTIFICATION_LIST_COMMAND = 'notifications list';

export async function executeNotificationCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  if (!command.startsWith('notifications ')) {
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
        limit: notificationLimit(arguments_.limit),
      };
      try {
        encodeNotificationQuery(query);
      } catch (error) {
        if (error instanceof TypeError || error instanceof RangeError) {
          throw usageError(error.message);
        }
        throw error;
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

function notificationLimit(value: string | undefined): number {
  if (value === undefined) {
    return DEFAULT_NOTIFICATION_LIMIT;
  }
  if (!/^[0-9]+$/u.test(value)) {
    throw usageError('notification limit must be an integer');
  }
  const limit = Number(value);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_NOTIFICATION_LIMIT) {
    throw usageError(`notification limit must be between 1 and ${MAX_NOTIFICATION_LIMIT}`);
  }
  return limit;
}
