import {
  type CloudApi,
  type FormDraftInput,
  MAX_FORM_DOCUMENT_BYTES,
  validateFormDraftInput,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import {
  formDraftMutationResult,
  formDraftResult,
  formDraftsResult,
  formPublicationMutationResult,
  formReleaseResult,
  formReleasesResult,
} from './form-results';
import type { CommandResult } from './results';

const MAX_FORM_DRAFT_FILE_BYTES = MAX_FORM_DOCUMENT_BYTES + 8 * 1024;

interface FormCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeFormCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: FormCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'forms list':
      requireListCommand(arguments_);
      return formDraftsResult(
        await cloudApi().listFormDrafts(requireOrganization(context), requireProject(context))
      );
    case 'forms get':
      requireReadCommand(arguments_, 'forms get <form-id>');
      return formDraftResult(
        await cloudApi().getFormDraft(requireOrganization(context), positionalUuid(positionals, 2, 'Form ID'))
      );
    case 'forms create': {
      const mutation = requireFormDraftMutation(arguments_, false);
      const input = await readFormDraftInput(mutation.file, dependencies.readFile);
      return formDraftMutationResult(
        await cloudApi().createFormDraft(
          requireOrganization(context),
          requireProject(context),
          input,
          mutation.idempotencyKey
        )
      );
    }
    case 'forms revise': {
      const mutation = requireFormDraftMutation(arguments_, true);
      const input = await readFormDraftInput(mutation.file, dependencies.readFile);
      return formDraftMutationResult(
        await cloudApi().reviseFormDraft(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Form ID'),
          input,
          { expectedVersion: mutation.expectedVersion },
          mutation.idempotencyKey
        )
      );
    }
    case 'form-releases list':
      requireArity(positionals, 3, 'form-releases list <form-id>');
      rejectReadMutationOptions(arguments_);
      return formReleasesResult(
        await cloudApi().listFormReleases(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Form ID')
        )
      );
    case 'form-releases get':
      requireArity(positionals, 4, 'form-releases get <form-id> <release-id>');
      rejectReadMutationOptions(arguments_);
      return formReleaseResult(
        await cloudApi().getFormRelease(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Form ID'),
          positionalUuid(positionals, 3, 'Form release ID')
        )
      );
    case 'form-releases publish': {
      requireArity(positionals, 3, 'form-releases publish <form-id>');
      rejectLogOptions(arguments_);
      rejectFileOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      const idempotencyKey = requireIdempotencyKey(arguments_);
      const expectedVersion = requireExpectedFormVersion(arguments_);
      return formPublicationMutationResult(
        await cloudApi().publishFormRelease(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Form ID'),
          { expectedVersion },
          idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function requireFormDraftMutation(
  arguments_: ParsedArguments,
  revision: true
): { idempotencyKey: string; file: string; expectedVersion: number };
function requireFormDraftMutation(
  arguments_: ParsedArguments,
  revision: false
): { idempotencyKey: string; file: string; expectedVersion?: never };
function requireFormDraftMutation(
  arguments_: ParsedArguments,
  revision: boolean
): { idempotencyKey: string; file: string; expectedVersion?: number } {
  requireArity(
    arguments_.positionals,
    revision ? 3 : 2,
    revision ? 'forms revise <form-id>' : 'forms create'
  );
  rejectLogOptions(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const file = arguments_.file;
  if (file === undefined || file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError('--file with a valid native Form draft JSON path is required');
  }
  if (!revision) {
    rejectExpectedVersionOption(arguments_);
    return { idempotencyKey, file };
  }
  return { idempotencyKey, file, expectedVersion: requireExpectedFormVersion(arguments_) };
}

function requireExpectedFormVersion(arguments_: ParsedArguments): number {
  const rawVersion = arguments_.expectedVersion;
  if (rawVersion === undefined || !/^[0-9]+$/.test(rawVersion)) {
    throw usageError('--expected-version must be a positive safe integer for Form mutation');
  }
  const expectedVersion = Number(rawVersion);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError('--expected-version must be a positive safe integer for Form mutation');
  }
  return expectedVersion;
}

function rejectReadMutationOptions(arguments_: ParsedArguments): void {
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
}

async function readFormDraftInput(
  path: string,
  readFile: (path: string) => Promise<Uint8Array> = (value) => Bun.file(value).bytes()
): Promise<FormDraftInput> {
  let bytes: Uint8Array;
  try {
    bytes = await readFile(path);
  } catch {
    throw usageError('unable to read the native Form draft JSON file');
  }
  if (bytes.byteLength < 1 || bytes.byteLength > MAX_FORM_DRAFT_FILE_BYTES) {
    throw usageError(`Form draft input must contain between 1 and ${MAX_FORM_DRAFT_FILE_BYTES} UTF-8 bytes`);
  }
  let decoded: string;
  try {
    decoded = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw usageError('Form draft input must be valid UTF-8');
  }
  let value: unknown;
  try {
    value = JSON.parse(decoded);
  } catch {
    throw usageError('Form draft input must be valid JSON transport');
  }
  if (!isFormDraftInput(value)) {
    throw usageError('Form draft input must contain only name, optional description, and document');
  }
  try {
    validateFormDraftInput(value);
  } catch (error) {
    throw usageError(error instanceof Error ? error.message : 'Form draft input is invalid');
  }
  return value;
}

function isFormDraftInput(value: unknown): value is FormDraftInput {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const record = value as Record<string, unknown>;
  return (
    !Object.keys(record).some((key) => !['name', 'description', 'document'].includes(key)) &&
    typeof record.name === 'string' &&
    (record.description === undefined || typeof record.description === 'string') &&
    typeof record.document === 'object' &&
    record.document !== null &&
    !Array.isArray(record.document)
  );
}
