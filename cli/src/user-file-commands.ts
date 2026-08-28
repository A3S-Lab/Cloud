import {
  type CloudApi,
  encodeUserFileListOptions,
  type UserFileListOptions,
  USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
} from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectAgentProviderKindOption,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  requireArity,
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import type { CommandResult } from './results';
import {
  userFileMutationResult,
  userFileQuotaResult,
  userFileResult,
  userFilesResult,
} from './user-file-results';

interface UserFileCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeUserFileCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: UserFileCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const organizationId = () => requireOrganization(context);
  const projectId = () => requireProject(context);
  switch (command) {
    case 'user-files reserve': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'user-files reserve');
      rejectAgentProviderKindOption(arguments_);
      const admissionAcl = await readAclDocument(
        mutation.file,
        {
          label: 'UserFile admission ACL',
          maximumBytes: USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
        },
        dependencies.readFile
      );
      return userFileMutationResult(
        await cloudApi().reserveUserFile(
          organizationId(),
          projectId(),
          { admissionAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'user-files list':
      requireUserFileListCommand(arguments_);
      return userFilesResult(
        await cloudApi().listUserFiles(organizationId(), projectId(), userFileListOptions(arguments_))
      );
    case 'user-files get':
      requireReadCommand(arguments_, 'user-files get <user-file-id>');
      return userFileResult(
        await cloudApi().getUserFile(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'UserFile ID')
        )
      );
    case 'user-files tombstone': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'user-files tombstone <user-file-id>',
        'UserFile'
      );
      return userFileMutationResult(
        await cloudApi().tombstoneUserFile(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'UserFile ID'),
          mutation.expectedVersion,
          mutation.idempotencyKey
        )
      );
    }
    case 'user-file-quota get':
      requireReadCommand(arguments_, 'user-file-quota get', 2);
      return userFileQuotaResult(await cloudApi().getUserFileQuota(organizationId()));
    default:
      return undefined;
  }
}

function requireUserFileListCommand(arguments_: ParsedArguments): void {
  requireArity(arguments_.positionals, 2, 'user-files list');
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectAgentProviderKindOption(arguments_);
  if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
    throw usageError('cursor and stream options are valid only for log commands');
  }
}

function userFileListOptions(arguments_: ParsedArguments): UserFileListOptions {
  let limit: number | undefined;
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError('UserFile list limit must be an integer');
    }
    limit = Number(arguments_.limit);
  }
  const options = { limit };
  try {
    encodeUserFileListOptions(options);
  } catch (error) {
    if (error instanceof Error) {
      throw usageError(error.message);
    }
    throw error;
  }
  return options;
}
