import {
  type CloudApi,
  encodeUserFileListOptions,
  type UserFileListOptions,
  USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
  USER_FILE_MAX_BYTES,
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
  requireExpectedVersion,
  requireIdempotencyKey,
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import type { CommandResult } from './results';
import {
  userFileContentWriteResult,
  userFileMutationResult,
  userFileQuotaResult,
  userFileResult,
  userFilesResult,
} from './user-file-results';

interface UserFileCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
  writeFile?: (path: string, content: Uint8Array) => Promise<void>;
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
    case 'user-files scan': {
      requireArity(arguments_.positionals, 3, 'user-files scan <user-file-id>');
      rejectAgentProviderKindOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      rejectFileOption(arguments_);
      if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
        throw usageError('cursor and stream options are valid only for log commands');
      }
      const expectedVersion = requireExpectedVersion(arguments_, 'UserFile');
      const idempotencyKey = requireIdempotencyKey(arguments_);
      const evidenceDigest = arguments_.evidenceDigest;
      if (evidenceDigest === undefined || evidenceDigest.length === 0) {
        throw usageError('--evidence-digest is required for user-files scan');
      }
      const decisionKind = arguments_.decision;
      if (decisionKind !== 'admitted' && decisionKind !== 'rejected') {
        throw usageError('--decision must be admitted or rejected');
      }
      const decision =
        decisionKind === 'admitted'
          ? { kind: 'admitted' as const }
          : (() => {
              const reasonCode = arguments_.reason;
              if (reasonCode === undefined || reasonCode.length === 0) {
                throw usageError('--reason is required when --decision=rejected');
              }
              return { kind: 'rejected' as const, reasonCode };
            })();
      return userFileMutationResult(
        await cloudApi().recordUserFileScan(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'UserFile ID'),
          { expectedVersion, evidenceDigest, decision },
          idempotencyKey
        )
      );
    }
    case 'user-files put-content': {
      requireArity(arguments_.positionals, 3, 'user-files put-content <user-file-id>');
      rejectAgentProviderKindOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
        throw usageError('cursor and stream options are valid only for log commands');
      }
      const filePath = arguments_.file;
      if (filePath === undefined || filePath.length === 0) {
        throw usageError('--file is required for user-files put-content');
      }
      const expectedVersion = requireExpectedVersion(arguments_, 'UserFile');
      const idempotencyKey = requireIdempotencyKey(arguments_);
      const readFile = dependencies.readFile ?? ((path: string) => Bun.file(path).bytes());
      const content = await readFile(filePath);
      if (content.byteLength === 0 || content.byteLength > USER_FILE_MAX_BYTES) {
        throw usageError(`UserFile content must be between 1 and ${USER_FILE_MAX_BYTES} bytes`);
      }
      return userFileMutationResult(
        await cloudApi().putUserFileContent(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'UserFile ID'),
          content,
          expectedVersion,
          idempotencyKey
        )
      );
    }
    case 'user-files get-content': {
      requireArity(arguments_.positionals, 3, 'user-files get-content <user-file-id>');
      rejectAgentProviderKindOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      rejectIdempotencyOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
        throw usageError('cursor and stream options are valid only for log commands');
      }
      const filePath = arguments_.file;
      if (filePath === undefined || filePath.length === 0) {
        throw usageError('--file is required for user-files get-content');
      }
      const content = await cloudApi().getUserFileContent(
        organizationId(),
        projectId(),
        positionalUuid(arguments_.positionals, 2, 'UserFile ID')
      );
      const writeFile =
        dependencies.writeFile ??
        (async (path: string, bytes: Uint8Array) => {
          await Bun.write(path, bytes);
        });
      await writeFile(filePath, content);
      return userFileContentWriteResult(filePath, content.byteLength);
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
