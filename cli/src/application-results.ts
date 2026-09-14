import type {
  Application,
  ApplicationAnnotation,
  ApplicationAnnotationMutationResult,
  ApplicationFeedback,
  ApplicationFeedbackMutationResult,
  ApplicationMessageCitation,
  ApplicationMessageCitationMutationResult,
  ApplicationMessageFileReference,
  ApplicationMessageFileReferenceMutationResult,
  ApplicationMessageVariant,
  ApplicationMessageVariantMutationResult,
  ApplicationInvocation,
  ApplicationInvocationCancellationResult,
  ApplicationInvocationMutationResult,
  ApplicationMessage,
  ApplicationMutationResult,
  ApplicationRelease,
  ApplicationSession,
  ApplicationSessionMutationResult,
  ApplicationSessionReplay,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const APPLICATION_COLUMNS = [
  { header: 'NAME', value: (row: Application) => row.name },
  { header: 'APPLICATION', value: (row: Application) => row.applicationId },
  { header: 'EXPERIENCE', value: (row: Application) => row.experience },
  { header: 'RELEASE', value: (row: Application) => row.currentReleaseNumber },
  { header: 'DIGEST', value: (row: Application) => row.currentReleaseDigest },
  { header: 'VERSION', value: (row: Application) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row: Application) => row.updatedAt },
] as const;

const APPLICATION_RELEASE_COLUMNS = [
  { header: 'APPLICATION', value: (row: ApplicationRelease) => row.applicationId },
  { header: 'NUMBER', value: (row: ApplicationRelease) => row.releaseNumber },
  { header: 'RELEASE', value: (row: ApplicationRelease) => row.releaseId },
  { header: 'EXPERIENCE', value: (row: ApplicationRelease) => row.experience },
  { header: 'DIGEST', value: (row: ApplicationRelease) => row.contractDigest },
  { header: 'PARENT', value: (row: ApplicationRelease) => row.parentReleaseId ?? '' },
  { header: 'CREATED AT', value: (row: ApplicationRelease) => row.createdAt },
] as const;

const APPLICATION_SESSION_COLUMNS = [
  { header: 'SESSION', value: (row: ApplicationSession) => row.sessionId },
  { header: 'APPLICATION', value: (row: ApplicationSession) => row.applicationId },
  { header: 'RELEASE', value: (row: ApplicationSession) => row.applicationReleaseNumber },
  { header: 'MODE', value: (row: ApplicationSession) => row.interactionMode },
  { header: 'STATUS', value: (row: ApplicationSession) => row.status },
  { header: 'MESSAGES', value: (row: ApplicationSession) => row.lastMessageSequence },
  { header: 'VERSION', value: (row: ApplicationSession) => row.aggregateVersion },
] as const;

const APPLICATION_INVOCATION_COLUMNS = [
  { header: 'INVOCATION', value: (row: ApplicationInvocation) => row.invocationId },
  { header: 'SESSION', value: (row: ApplicationInvocation) => row.sessionId },
  { header: 'MODE', value: (row: ApplicationInvocation) => row.responseMode },
  { header: 'STATUS', value: (row: ApplicationInvocation) => row.status },
  { header: 'WORKFLOW RUN', value: (row: ApplicationInvocation) => row.workflowRunId ?? '' },
  { header: 'REQUESTED AT', value: (row: ApplicationInvocation) => row.requestedAt },
] as const;

const APPLICATION_MESSAGE_COLUMNS = [
  { header: 'SEQUENCE', value: (row: ApplicationMessage) => row.sequence },
  { header: 'KIND', value: (row: ApplicationMessage) => row.kind },
  { header: 'INVOCATION', value: (row: ApplicationMessage) => row.invocationId },
  { header: 'MESSAGE', value: (row: ApplicationMessage) => row.messageId },
  { header: 'DIGEST', value: (row: ApplicationMessage) => row.contentDigest },
  { header: 'CREATED AT', value: (row: ApplicationMessage) => row.createdAt },
] as const;

const APPLICATION_FEEDBACK_COLUMNS = [
  { header: 'FEEDBACK', value: (row: ApplicationFeedback) => row.feedbackId },
  { header: 'SESSION', value: (row: ApplicationFeedback) => row.sessionId },
  { header: 'RATING', value: (row: ApplicationFeedback) => row.rating },
  { header: 'DIGEST', value: (row: ApplicationFeedback) => row.contentDigest },
  { header: 'CREATED AT', value: (row: ApplicationFeedback) => row.createdAt },
] as const;

const APPLICATION_ANNOTATION_COLUMNS = [
  { header: 'ANNOTATION', value: (row: ApplicationAnnotation) => row.annotationId },
  { header: 'SESSION', value: (row: ApplicationAnnotation) => row.sessionId },
  { header: 'DIGEST', value: (row: ApplicationAnnotation) => row.contentDigest },
  { header: 'CREATED AT', value: (row: ApplicationAnnotation) => row.createdAt },
] as const;

const APPLICATION_MESSAGE_CITATION_COLUMNS = [
  { header: 'CITATION', value: (row: ApplicationMessageCitation) => row.citationId },
  { header: 'MESSAGE', value: (row: ApplicationMessageCitation) => row.messageId },
  { header: 'CHUNK', value: (row: ApplicationMessageCitation) => row.knowledgeChunkId },
  { header: 'EXCERPT DIGEST', value: (row: ApplicationMessageCitation) => row.excerptDigest },
  { header: 'CREATED AT', value: (row: ApplicationMessageCitation) => row.createdAt },
] as const;

const APPLICATION_MESSAGE_FILE_REFERENCE_COLUMNS = [
  { header: 'REFERENCE', value: (row: ApplicationMessageFileReference) => row.referenceId },
  { header: 'MESSAGE', value: (row: ApplicationMessageFileReference) => row.messageId },
  { header: 'FILE', value: (row: ApplicationMessageFileReference) => row.userFileId },
  { header: 'DIGEST', value: (row: ApplicationMessageFileReference) => row.contentDigest },
  { header: 'CREATED AT', value: (row: ApplicationMessageFileReference) => row.createdAt },
] as const;

const APPLICATION_MESSAGE_VARIANT_COLUMNS = [
  { header: 'VARIANT', value: (row: ApplicationMessageVariant) => row.variantId },
  { header: 'SOURCE', value: (row: ApplicationMessageVariant) => row.sourceMessageId },
  { header: 'KIND', value: (row: ApplicationMessageVariant) => row.sourceMessageKind },
  { header: 'INVOCATION', value: (row: ApplicationMessageVariant) => row.invocationId },
  { header: 'CREATED AT', value: (row: ApplicationMessageVariant) => row.createdAt },
] as const;

export function applicationsResult(rows: Application[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_COLUMNS) };
}

export function applicationResult(row: Application): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_COLUMNS) };
}

export function applicationReleasesResult(rows: ApplicationRelease[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_RELEASE_COLUMNS) };
}

export function applicationReleaseResult(row: ApplicationRelease): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_RELEASE_COLUMNS) };
}

export function applicationMutationResult(result: ApplicationMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.record.application, replayed: result.replayed }],
      [...APPLICATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationSessionResult(row: ApplicationSession): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_SESSION_COLUMNS) };
}

export function applicationSessionMutationResult(result: ApplicationSessionMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.session, replayed: result.replayed }],
      [...APPLICATION_SESSION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationInvocationResult(row: ApplicationInvocation): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_INVOCATION_COLUMNS) };
}

export function applicationInvocationMutationResult(
  result: ApplicationInvocationMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.invocation, replayed: result.replayed }],
      [...APPLICATION_INVOCATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationInvocationCancellationResult(
  result: ApplicationInvocationCancellationResult
): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.invocation, replayed: result.replayed }],
      [...APPLICATION_INVOCATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationMessagesResult(rows: ApplicationMessage[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_MESSAGE_COLUMNS) };
}

export function applicationSessionReplayResult(result: ApplicationSessionReplay): CommandResult {
  return {
    json: result,
    table: renderTable(result.messages, APPLICATION_MESSAGE_COLUMNS),
  };
}

export function applicationFeedbacksResult(rows: ApplicationFeedback[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_FEEDBACK_COLUMNS) };
}

export function applicationFeedbackResult(row: ApplicationFeedback): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_FEEDBACK_COLUMNS) };
}

export function applicationFeedbackMutationResult(
  result: ApplicationFeedbackMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable([{ ...result.feedback, replayed: result.replayed }], [
      ...APPLICATION_FEEDBACK_COLUMNS,
      { header: 'REPLAYED', value: (row: ApplicationFeedback & { replayed: boolean }) => row.replayed },
    ]),
  };
}

export function applicationAnnotationsResult(rows: ApplicationAnnotation[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_ANNOTATION_COLUMNS) };
}

export function applicationAnnotationResult(row: ApplicationAnnotation): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_ANNOTATION_COLUMNS) };
}

export function applicationAnnotationMutationResult(
  result: ApplicationAnnotationMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable([{ ...result.annotation, replayed: result.replayed }], [
      ...APPLICATION_ANNOTATION_COLUMNS,
      {
        header: 'REPLAYED',
        value: (row: ApplicationAnnotation & { replayed: boolean }) => row.replayed,
      },
    ]),
  };
}

export function applicationMessageCitationsResult(
  rows: ApplicationMessageCitation[]
): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_MESSAGE_CITATION_COLUMNS) };
}

export function applicationMessageCitationResult(row: ApplicationMessageCitation): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_MESSAGE_CITATION_COLUMNS) };
}

export function applicationMessageCitationMutationResult(
  result: ApplicationMessageCitationMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable([{ ...result.citation, replayed: result.replayed }], [
      ...APPLICATION_MESSAGE_CITATION_COLUMNS,
      {
        header: 'REPLAYED',
        value: (row: ApplicationMessageCitation & { replayed: boolean }) => row.replayed,
      },
    ]),
  };
}

export function applicationMessageFileReferencesResult(
  rows: ApplicationMessageFileReference[]
): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_MESSAGE_FILE_REFERENCE_COLUMNS) };
}

export function applicationMessageFileReferenceResult(
  row: ApplicationMessageFileReference
): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_MESSAGE_FILE_REFERENCE_COLUMNS) };
}

export function applicationMessageFileReferenceMutationResult(
  result: ApplicationMessageFileReferenceMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable([{ ...result.reference, replayed: result.replayed }], [
      ...APPLICATION_MESSAGE_FILE_REFERENCE_COLUMNS,
      {
        header: 'REPLAYED',
        value: (row: ApplicationMessageFileReference & { replayed: boolean }) => row.replayed,
      },
    ]),
  };
}

export function applicationMessageVariantsResult(
  rows: ApplicationMessageVariant[]
): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_MESSAGE_VARIANT_COLUMNS) };
}

export function applicationMessageVariantResult(row: ApplicationMessageVariant): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_MESSAGE_VARIANT_COLUMNS) };
}

export function applicationMessageVariantMutationResult(
  result: ApplicationMessageVariantMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable([{ ...result.variant, replayed: result.replayed }], [
      ...APPLICATION_MESSAGE_VARIANT_COLUMNS,
      {
        header: 'REPLAYED',
        value: (row: ApplicationMessageVariant & { replayed: boolean }) => row.replayed,
      },
    ]),
  };
}
