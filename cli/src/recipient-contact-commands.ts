import {
  type CloudApi,
  CloudApiError,
  MAX_RECIPIENT_CONTACT_ADDRESS_BYTES,
  MAX_RECIPIENT_CONTACT_PROOF_BYTES,
  validateRecipientContactAddress,
  validateRecipientContactProof,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization } from './context';
import { usageError } from './errors';
import {
  recipientContactMutationResult,
  recipientContactResult,
  recipientContactsResult,
} from './recipient-contact-results';
import type { CommandResult } from './results';
import { type ReadStdin, readBoundedUtf8Stdin } from './standard-input';

const RECIPIENT_CONTACT_REQUEST_COMMAND = 'recipient-contacts request';
const RECIPIENT_CONTACT_VERIFY_COMMAND = 'recipient-contacts verify';

export interface RecipientContactCommandDependencies {
  readStdin?: ReadStdin;
}

export function rejectMisplacedRecipientContactOptions(command: string, arguments_: ParsedArguments): void {
  if (arguments_.addressStdin && command !== RECIPIENT_CONTACT_REQUEST_COMMAND) {
    throw usageError('--address-stdin is valid only for recipient contact verification requests');
  }
  if (arguments_.proofStdin && command !== RECIPIENT_CONTACT_VERIFY_COMMAND) {
    throw usageError('--proof-stdin is valid only for recipient contact verification completion');
  }
}

export async function executeRecipientContactCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: RecipientContactCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'recipient-contacts list':
      requireListCommand(arguments_);
      return recipientContactsResult(await cloudApi().listRecipientContacts(requireOrganization(context)));
    case 'recipient-contacts get':
      requireReadCommand(arguments_, 'recipient-contacts get <recipient-contact-id>');
      return recipientContactResult(
        await cloudApi().getRecipientContact(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'recipient contact ID')
        )
      );
    case RECIPIENT_CONTACT_REQUEST_COMMAND: {
      requireArity(positionals, 2, 'recipient-contacts request');
      rejectLogOptions(arguments_);
      rejectFileOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (!arguments_.addressStdin) {
        throw usageError('--address-stdin is required for recipient contact verification requests');
      }
      const idempotencyKey = requireIdempotencyKey(arguments_);
      const address = await readRecipientContactAddress(dependencies.readStdin);
      return recipientContactMutationResult(
        await safeRecipientContactMutation(() =>
          cloudApi().requestRecipientContactVerification(
            requireOrganization(context),
            { address },
            idempotencyKey
          )
        )
      );
    }
    case RECIPIENT_CONTACT_VERIFY_COMMAND: {
      requireArity(positionals, 3, 'recipient-contacts verify <recipient-contact-id>');
      rejectLogOptions(arguments_);
      rejectFileOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (!arguments_.proofStdin) {
        throw usageError('--proof-stdin is required for recipient contact verification completion');
      }
      const contactId = positionalUuid(positionals, 2, 'recipient contact ID');
      const idempotencyKey = requireIdempotencyKey(arguments_);
      const proof = await readRecipientContactProof(dependencies.readStdin);
      return recipientContactMutationResult(
        await safeRecipientContactMutation(() =>
          cloudApi().completeRecipientContactVerification(
            requireOrganization(context),
            contactId,
            { proof },
            idempotencyKey
          )
        )
      );
    }
    case 'recipient-contacts revoke': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'recipient-contacts revoke <recipient-contact-id>',
        'recipient contact'
      );
      return recipientContactMutationResult(
        await safeRecipientContactMutation(() =>
          cloudApi().revokeRecipientContact(
            requireOrganization(context),
            positionalUuid(positionals, 2, 'recipient contact ID'),
            mutation.expectedVersion,
            mutation.idempotencyKey
          )
        )
      );
    }
    default:
      return undefined;
  }
}

async function readRecipientContactAddress(readStdin?: ReadStdin): Promise<string> {
  const address = await readBoundedUtf8Stdin(readStdin, 3, MAX_RECIPIENT_CONTACT_ADDRESS_BYTES, {
    read: 'unable to read recipient contact address from standard input',
    size: 'recipient contact address must contain 3 to 254 bytes',
    utf8: 'recipient contact address must be valid UTF-8',
  });
  try {
    validateRecipientContactAddress(address);
  } catch {
    throw usageError('recipient contact address must be a bounded canonical ASCII mailbox');
  }
  return address;
}

async function readRecipientContactProof(readStdin?: ReadStdin): Promise<string> {
  const proof = await readBoundedUtf8Stdin(readStdin, 1, MAX_RECIPIENT_CONTACT_PROOF_BYTES, {
    read: 'unable to read recipient contact proof from standard input',
    size: 'recipient contact proof must contain 1 to 4096 bytes',
    utf8: 'recipient contact proof must be valid UTF-8',
  });
  try {
    validateRecipientContactProof(proof);
  } catch {
    throw usageError('recipient contact proof is invalid');
  }
  return proof;
}

async function safeRecipientContactMutation<Result>(operation: () => Promise<Result>): Promise<Result> {
  try {
    return await operation();
  } catch (error) {
    if (error instanceof CloudApiError) {
      throw new CloudApiError(
        error.status,
        'recipient contact mutation failed',
        error.statusCode,
        error.requestId
      );
    }
    throw error;
  }
}
