import {
  type ApplicationResponseMode,
  type CloudApi,
  type CreateApplicationAnnotationInput,
  type CreateApplicationFeedbackInput,
  type CreateApplicationMessageCitationInput,
  type CreateApplicationMessageFileReferenceInput,
  type CreateApplicationMessageVariantInput,
  DEFAULT_APPLICATION_MESSAGE_LIST_LIMIT,
  MAX_APPLICATION_ANNOTATION_CONTENT_BYTES,
  MAX_APPLICATION_FEEDBACK_COMMENT_CHARACTERS,
  MAX_APPLICATION_MESSAGE_VARIANT_INSTRUCTION_BYTES,
  MAX_APPLICATION_MESSAGE_LIST_LIMIT,
  MAX_APPLICATION_CONVERSATION_VARIABLES_BYTES,
  MAX_APPLICATION_INVOCATION_INPUT_BYTES,
  MAX_APPLICATION_RELEASE_ACL_BYTES,
  type RequestApplicationInvocationInput,
  validateApplicationAnnotationInput,
  validateApplicationFeedbackInput,
  validateApplicationMessageCitationInput,
  validateApplicationMessageFileReferenceInput,
  validateApplicationMessageVariantInput,
} from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand, requireVersionedAclMutationCommand } from './acl-file';
import {
  applicationMutationResult,
  applicationInvocationCancellationResult,
  applicationInvocationMutationResult,
  applicationInvocationResult,
  applicationAnnotationMutationResult,
  applicationAnnotationResult,
  applicationAnnotationsResult,
  applicationFeedbackMutationResult,
  applicationFeedbackResult,
  applicationFeedbacksResult,
  applicationMessageCitationMutationResult,
  applicationMessageCitationResult,
  applicationMessageCitationsResult,
  applicationMessageFileReferenceMutationResult,
  applicationMessageFileReferenceResult,
  applicationMessageFileReferencesResult,
  applicationMessageVariantMutationResult,
  applicationMessageVariantResult,
  applicationMessageVariantsResult,
  applicationMessagesResult,
  applicationResult,
  applicationReleaseResult,
  applicationReleasesResult,
  applicationSessionMutationResult,
  applicationSessionReplayResult,
  applicationSessionResult,
  applicationsResult,
} from './application-results';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
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
  requireVersionedMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { parseUuid, requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import { isJsonObject, readBoundedJsonFile } from './json-file';
import type { CommandResult } from './results';

interface ApplicationCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

const APPLICATION_INVOCATION_INPUT_FIELDS = new Set([
  'ontologyId',
  'ontologyRevisionId',
  'environmentId',
  'responseMode',
  'input',
  'timeoutSeconds',
]);

export async function executeApplicationCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: ApplicationCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  const organizationId = () => requireOrganization(context);
  const projectId = () => requireProject(context);
  switch (command) {
    case 'applications list':
      requireListCommand(arguments_);
      return applicationsResult(await cloudApi().listApplications(organizationId(), projectId()));
    case 'applications get':
      requireReadCommand(arguments_, 'applications get <application-id>');
      return applicationResult(
        await cloudApi().getApplication(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID')
        )
      );
    case 'applications create': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'applications create <name>');
      const releaseAcl = await readApplicationAcl(mutation.file, dependencies.readFile);
      return applicationMutationResult(
        await cloudApi().createApplication(
          organizationId(),
          projectId(),
          {
            name: positionalResourceName(positionals, 2),
            description: '',
            releaseAcl,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'applications publish': {
      const mutation = requireVersionedAclMutationCommand(
        arguments_,
        3,
        'applications publish <application-id>',
        'Application'
      );
      const releaseAcl = await readApplicationAcl(mutation.file, dependencies.readFile);
      return applicationMutationResult(
        await cloudApi().publishApplicationRelease(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          { expectedVersion: mutation.expectedVersion, releaseAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'application-releases list':
      requireReadCommand(arguments_, 'application-releases list <application-id>');
      return applicationReleasesResult(
        await cloudApi().listApplicationReleases(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID')
        )
      );
    case 'application-releases get':
      requireReadCommand(arguments_, 'application-releases get <application-id> <release-id>', 4);
      return applicationReleaseResult(
        await cloudApi().getApplicationRelease(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application release ID')
        )
      );
    case 'application-sessions open': {
      const mutation = requireApplicationJsonMutation(
        arguments_,
        4,
        'application-sessions open <application-id> <release-id>',
        false
      );
      const initialVariables = mutation.file
        ? await readApplicationObject(
            mutation.file,
            'Application initial variables',
            MAX_APPLICATION_CONVERSATION_VARIABLES_BYTES,
            dependencies.readFile
          )
        : {};
      return applicationSessionMutationResult(
        await cloudApi().openApplicationSession(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          {
            releaseId: positionalUuid(positionals, 3, 'Application release ID'),
            initialVariables,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'application-sessions get':
      requireReadCommand(arguments_, 'application-sessions get <application-id> <session-id>', 4);
      return applicationSessionResult(
        await cloudApi().getApplicationSession(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID')
        )
      );
    case 'application-sessions close': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        4,
        'application-sessions close <application-id> <session-id>',
        'Application session'
      );
      return applicationSessionMutationResult(
        await cloudApi().closeApplicationSession(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          { expectedVersion: mutation.expectedVersion },
          mutation.idempotencyKey
        )
      );
    }
    case 'application-sessions replay': {
      const pagination = applicationReplayPagination(
        arguments_,
        'application-sessions replay <application-id> <session-id>',
        'Application session replay'
      );
      return applicationSessionReplayResult(
        await cloudApi().replayApplicationSession(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          pagination.afterSequence,
          pagination.limit
        )
      );
    }
    case 'application-invocations request': {
      const mutation = requireApplicationJsonMutation(
        arguments_,
        4,
        'application-invocations request <application-id> <session-id>',
        true
      );
      const transport = await readBoundedJsonFile(
        mutation.file,
        {
          label: 'Application invocation request',
          maximumBytes: MAX_APPLICATION_INVOCATION_INPUT_BYTES + 16 * 1024,
        },
        dependencies.readFile
      );
      return applicationInvocationMutationResult(
        await cloudApi().requestApplicationInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          applicationInvocationInput(transport),
          mutation.idempotencyKey
        )
      );
    }
    case 'application-invocations get':
      requireReadCommand(
        arguments_,
        'application-invocations get <application-id> <session-id> <invocation-id>',
        5
      );
      return applicationInvocationResult(
        await cloudApi().getApplicationInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID')
        )
      );
    case 'application-invocations cancel': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        5,
        'application-invocations cancel <application-id> <session-id> <invocation-id>',
        'Application invocation'
      );
      return applicationInvocationCancellationResult(
        await cloudApi().cancelApplicationInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID'),
          { expectedVersion: mutation.expectedVersion },
          mutation.idempotencyKey
        )
      );
    }
    case 'application-messages list': {
      const pagination = applicationReplayPagination(
        arguments_,
        'application-messages list <application-id> <session-id>',
        'Application message'
      );
      return applicationMessagesResult(
        await cloudApi().listApplicationMessages(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          pagination.afterSequence,
          pagination.limit
        )
      );
    }
    case 'application-feedbacks create': {
      const mutation = requireFeedbackAnnotationCreate(
        arguments_,
        'application-feedbacks create <application-id> <session-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Application feedback',
        8 * 1024,
        dependencies.readFile
      );
      const input = applicationFeedbackInput(body);
      validateApplicationFeedbackInput(input);
      return applicationFeedbackMutationResult(
        await cloudApi().createApplicationFeedback(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          input
        )
      );
    }
    case 'application-feedbacks list':
      requireReadCommand(
        arguments_,
        'application-feedbacks list <application-id> <session-id>',
        4
      );
      return applicationFeedbacksResult(
        await cloudApi().listApplicationFeedback(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID')
        )
      );
    case 'application-feedbacks get':
      requireReadCommand(
        arguments_,
        'application-feedbacks get <application-id> <session-id> <feedback-id>',
        5
      );
      return applicationFeedbackResult(
        await cloudApi().getApplicationFeedback(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application feedback ID')
        )
      );
    case 'application-annotations create': {
      const mutation = requireFeedbackAnnotationCreate(
        arguments_,
        'application-annotations create <application-id> <session-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Application annotation',
        MAX_APPLICATION_ANNOTATION_CONTENT_BYTES + 1024,
        dependencies.readFile
      );
      const input = applicationAnnotationInput(body);
      validateApplicationAnnotationInput(input);
      return applicationAnnotationMutationResult(
        await cloudApi().createApplicationAnnotation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          input
        )
      );
    }
    case 'application-annotations list':
      requireReadCommand(
        arguments_,
        'application-annotations list <application-id> <session-id>',
        4
      );
      return applicationAnnotationsResult(
        await cloudApi().listApplicationAnnotations(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID')
        )
      );
    case 'application-annotations get':
      requireReadCommand(
        arguments_,
        'application-annotations get <application-id> <session-id> <annotation-id>',
        5
      );
      return applicationAnnotationResult(
        await cloudApi().getApplicationAnnotation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application annotation ID')
        )
      );
    case 'application-message-citations create': {
      const mutation = requireFeedbackAnnotationCreate(
        arguments_,
        'application-message-citations create <application-id> <session-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Application message citation',
        8192,
        dependencies.readFile
      );
      const input = applicationMessageCitationInput(body);
      validateApplicationMessageCitationInput(input);
      return applicationMessageCitationMutationResult(
        await cloudApi().createApplicationMessageCitation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          input
        )
      );
    }
    case 'application-message-citations list':
      requireReadCommand(
        arguments_,
        'application-message-citations list <application-id> <session-id>',
        4
      );
      return applicationMessageCitationsResult(
        await cloudApi().listApplicationMessageCitations(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID')
        )
      );
    case 'application-message-citations get':
      requireReadCommand(
        arguments_,
        'application-message-citations get <application-id> <session-id> <citation-id>',
        5
      );
      return applicationMessageCitationResult(
        await cloudApi().getApplicationMessageCitation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application message citation ID')
        )
      );
    case 'application-message-file-references create': {
      const mutation = requireFeedbackAnnotationCreate(
        arguments_,
        'application-message-file-references create <application-id> <session-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Application message file reference',
        4096,
        dependencies.readFile
      );
      const input = applicationMessageFileReferenceInput(body);
      validateApplicationMessageFileReferenceInput(input);
      return applicationMessageFileReferenceMutationResult(
        await cloudApi().createApplicationMessageFileReference(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          input
        )
      );
    }
    case 'application-message-file-references list':
      requireReadCommand(
        arguments_,
        'application-message-file-references list <application-id> <session-id>',
        4
      );
      return applicationMessageFileReferencesResult(
        await cloudApi().listApplicationMessageFileReferences(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID')
        )
      );
    case 'application-message-file-references get':
      requireReadCommand(
        arguments_,
        'application-message-file-references get <application-id> <session-id> <reference-id>',
        5
      );
      return applicationMessageFileReferenceResult(
        await cloudApi().getApplicationMessageFileReference(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application message file reference ID')
        )
      );
    case 'application-message-variants create': {
      const mutation = requireFeedbackAnnotationCreate(
        arguments_,
        'application-message-variants create <application-id> <session-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Application message variant',
        MAX_APPLICATION_MESSAGE_VARIANT_INSTRUCTION_BYTES + 1024,
        dependencies.readFile
      );
      const input = applicationMessageVariantInput(body);
      validateApplicationMessageVariantInput(input);
      return applicationMessageVariantMutationResult(
        await cloudApi().createApplicationMessageVariant(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          input
        )
      );
    }
    case 'application-message-variants list':
      requireReadCommand(
        arguments_,
        'application-message-variants list <application-id> <session-id>',
        4
      );
      return applicationMessageVariantsResult(
        await cloudApi().listApplicationMessageVariants(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID')
        )
      );
    case 'application-message-variants get':
      requireReadCommand(
        arguments_,
        'application-message-variants get <application-id> <session-id> <variant-id>',
        5
      );
      return applicationMessageVariantResult(
        await cloudApi().getApplicationMessageVariant(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application message variant ID')
        )
      );
    default:
      return undefined;
  }
}

function requireFeedbackAnnotationCreate(
  arguments_: ParsedArguments,
  usage: string
): { file: string } {
  requireArity(arguments_.positionals, 4, usage);
  rejectIdempotencyOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectLogOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is not valid for Application feedback/annotation creates');
  }
  if (arguments_.file === undefined) {
    throw usageError(`--file is required for ${usage}`);
  }
  return { file: arguments_.file };
}

function applicationMessageCitationInput(
  value: Record<string, unknown>
): CreateApplicationMessageCitationInput {
  const allowed = new Set([
    'messageId',
    'knowledgeBaseId',
    'knowledgeBaseRevisionId',
    'knowledgeDocumentId',
    'knowledgeChunkId',
    'excerpt',
  ]);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw usageError(`Application message citation field '${key}' is not supported`);
    }
  }
  if (typeof value.messageId !== 'string') {
    throw usageError('Application message citation messageId must be a string');
  }
  for (const key of [
    'knowledgeBaseId',
    'knowledgeBaseRevisionId',
    'knowledgeDocumentId',
    'knowledgeChunkId',
  ] as const) {
    if (typeof value[key] !== 'string') {
      throw usageError(`Application message citation ${key} must be a string`);
    }
  }
  if (value.excerpt !== undefined && value.excerpt !== null && typeof value.excerpt !== 'string') {
    throw usageError('Application message citation excerpt must be a string when provided');
  }
  return {
    messageId: value.messageId,
    knowledgeBaseId: value.knowledgeBaseId as string,
    knowledgeBaseRevisionId: value.knowledgeBaseRevisionId as string,
    knowledgeDocumentId: value.knowledgeDocumentId as string,
    knowledgeChunkId: value.knowledgeChunkId as string,
    excerpt: (value.excerpt as string | null | undefined) ?? undefined,
  };
}

function applicationMessageFileReferenceInput(
  value: Record<string, unknown>
): CreateApplicationMessageFileReferenceInput {
  const allowed = new Set(['messageId', 'userFileId', 'contentDigest']);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw usageError(`Application message file reference field '${key}' is not supported`);
    }
  }
  if (typeof value.messageId !== 'string') {
    throw usageError('Application message file reference messageId must be a string');
  }
  if (typeof value.userFileId !== 'string') {
    throw usageError('Application message file reference userFileId must be a string');
  }
  if (typeof value.contentDigest !== 'string') {
    throw usageError('Application message file reference contentDigest must be a string');
  }
  return {
    messageId: value.messageId,
    userFileId: value.userFileId,
    contentDigest: value.contentDigest,
  };
}

function applicationMessageVariantInput(
  value: Record<string, unknown>
): CreateApplicationMessageVariantInput {
  const allowed = new Set(['sourceMessageId', 'instruction']);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw usageError(`Application message variant field '${key}' is not supported`);
    }
  }
  if (typeof value.sourceMessageId !== 'string') {
    throw usageError('Application message variant sourceMessageId must be a string');
  }
  if (
    value.instruction !== undefined &&
    (value.instruction === null ||
      Array.isArray(value.instruction) ||
      typeof value.instruction !== 'object')
  ) {
    throw usageError('Application message variant instruction must be a JSON object');
  }
  return {
    sourceMessageId: value.sourceMessageId,
    ...(value.instruction !== undefined
      ? { instruction: value.instruction as Record<string, unknown> }
      : {}),
  };
}

function applicationFeedbackInput(value: Record<string, unknown>): CreateApplicationFeedbackInput {
  const allowed = new Set(['rating', 'comment', 'sourceMessageId']);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw usageError(`Application feedback field '${key}' is not supported`);
    }
  }
  if (typeof value.rating !== 'string') {
    throw usageError('Application feedback rating must be a string');
  }
  if (value.comment !== undefined && typeof value.comment !== 'string') {
    throw usageError('Application feedback comment must be a string');
  }
  if (value.sourceMessageId !== undefined && typeof value.sourceMessageId !== 'string') {
    throw usageError('Application feedback sourceMessageId must be a string');
  }
  if (
    value.comment !== undefined &&
    Array.from(value.comment).length > MAX_APPLICATION_FEEDBACK_COMMENT_CHARACTERS
  ) {
    throw usageError(
      `Application feedback comment must contain at most ${MAX_APPLICATION_FEEDBACK_COMMENT_CHARACTERS} characters`
    );
  }
  return {
    rating: value.rating as CreateApplicationFeedbackInput['rating'],
    ...(value.comment !== undefined ? { comment: value.comment } : {}),
    ...(value.sourceMessageId !== undefined ? { sourceMessageId: value.sourceMessageId } : {}),
  };
}

function applicationAnnotationInput(
  value: Record<string, unknown>
): CreateApplicationAnnotationInput {
  const allowed = new Set(['content', 'sourceMessageId']);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw usageError(`Application annotation field '${key}' is not supported`);
    }
  }
  if (!('content' in value)) {
    throw usageError('Application annotation content is required');
  }
  if (value.sourceMessageId !== undefined && typeof value.sourceMessageId !== 'string') {
    throw usageError('Application annotation sourceMessageId must be a string');
  }
  return {
    content: value.content,
    ...(value.sourceMessageId !== undefined ? { sourceMessageId: value.sourceMessageId } : {}),
  };
}

function applicationReplayPagination(
  arguments_: ParsedArguments,
  usage: string,
  label: string
): {
  afterSequence: number;
  limit: number;
} {
  requireArity(arguments_.positionals, 4, usage);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError(`--stream is not valid for ${label} reads`);
  }
  return {
    afterSequence: boundedApplicationMessageInteger(
      arguments_.cursor,
      `${label} cursor`,
      0,
      Number.MAX_SAFE_INTEGER,
      0
    ),
    limit: boundedApplicationMessageInteger(
      arguments_.limit,
      `${label} limit`,
      1,
      MAX_APPLICATION_MESSAGE_LIST_LIMIT,
      DEFAULT_APPLICATION_MESSAGE_LIST_LIMIT
    ),
  };
}

function boundedApplicationMessageInteger(
  raw: string | undefined,
  label: string,
  minimum: number,
  maximum: number,
  fallback: number
): number {
  if (raw === undefined) {
    return fallback;
  }
  if (!/^[0-9]+$/u.test(raw)) {
    throw usageError(`${label} must be an integer between ${minimum} and ${maximum}`);
  }
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw usageError(`${label} must be an integer between ${minimum} and ${maximum}`);
  }
  return value;
}

function requireApplicationJsonMutation(
  arguments_: ParsedArguments,
  arity: number,
  usage: string,
  fileRequired: true
): { idempotencyKey: string; file: string };
function requireApplicationJsonMutation(
  arguments_: ParsedArguments,
  arity: number,
  usage: string,
  fileRequired: false
): { idempotencyKey: string; file?: string };
function requireApplicationJsonMutation(
  arguments_: ParsedArguments,
  arity: number,
  usage: string,
  fileRequired: boolean
): { idempotencyKey: string; file?: string } {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (fileRequired && arguments_.file === undefined) {
    throw usageError('--file is required for the Application invocation request');
  }
  return {
    idempotencyKey: requireIdempotencyKey(arguments_),
    file: arguments_.file,
  };
}

async function readApplicationObject(
  file: string,
  label: string,
  maximumBytes: number,
  readFile: ((path: string) => Promise<Uint8Array>) | undefined
): Promise<Record<string, unknown>> {
  const value = await readBoundedJsonFile(file, { label, maximumBytes }, readFile);
  if (!isJsonObject(value)) {
    throw usageError(`${label} must be a JSON object`);
  }
  return { ...value };
}

function applicationInvocationInput(value: unknown): RequestApplicationInvocationInput {
  if (!isJsonObject(value)) {
    throw usageError('Application invocation request must be a JSON object');
  }
  const unknownFields = Object.keys(value)
    .filter((field) => !APPLICATION_INVOCATION_INPUT_FIELDS.has(field))
    .sort();
  if (unknownFields.length > 0) {
    throw usageError(
      `Application invocation request contains unsupported fields: ${unknownFields.join(', ')}`
    );
  }
  const responseMode = value.responseMode;
  if (responseMode !== 'asynchronous' && responseMode !== 'blocking' && responseMode !== 'streaming') {
    throw usageError('Application invocation responseMode is unsupported');
  }
  if (!isJsonObject(value.input)) {
    throw usageError('Application invocation input must be a JSON object');
  }
  const timeoutSeconds = optionalPositiveInteger(value.timeoutSeconds, 'timeoutSeconds');
  return {
    ontologyId: parseUuid(requiredString(value.ontologyId, 'ontologyId'), 'Ontology ID'),
    ontologyRevisionId: parseUuid(
      requiredString(value.ontologyRevisionId, 'ontologyRevisionId'),
      'Ontology revision ID'
    ),
    environmentId:
      value.environmentId === undefined
        ? undefined
        : parseUuid(requiredString(value.environmentId, 'environmentId'), 'Environment ID'),
    responseMode: responseMode as ApplicationResponseMode,
    input: { ...value.input },
    timeoutSeconds,
  };
}

function requiredString(value: unknown, field: string): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw usageError(`Application invocation ${field} must be a non-empty string`);
  }
  return value;
}

function optionalPositiveInteger(value: unknown, field: string): number | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!Number.isSafeInteger(value) || (value as number) < 1) {
    throw usageError(`Application invocation ${field} must be a positive safe integer`);
  }
  return value as number;
}

function readApplicationAcl(
  file: string,
  readFile: ((path: string) => Promise<Uint8Array>) | undefined
): Promise<string> {
  return readAclDocument(
    file,
    {
      label: 'Application release ACL',
      maximumBytes: MAX_APPLICATION_RELEASE_ACL_BYTES,
    },
    readFile
  );
}
