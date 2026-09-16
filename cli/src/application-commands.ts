import {
  type ApplicationResponseMode,
  type CloudApi,
  type CreateApplicationAnnotationInput,
  type CreateApplicationFeedbackInput,
  type CreateApplicationMessageCitationInput,
  type CreateApplicationMessageFileReferenceInput,
  type CreateApplicationMessageVariantInput,
  type OpenAnonymousApplicationSessionInput,
  type RegisterApplicationDeliveryCredentialInput,
  type CreateApplicationPublicationRouteIntentInput,
  type ApplicationPublicationChannel,
  type RequestAnonymousApplicationInvocationInput,
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
  validateCancelAnonymousApplicationInvocationInput,
  validateCloseAnonymousApplicationSessionInput,
  validateObserveAnonymousApplicationInvocationInput,
  validateOpenAnonymousApplicationSessionInput,
  validateRegisterApplicationDeliveryCredentialInput,
  validateRequestAnonymousApplicationInvocationInput,
  validateApplicationDeliveryCredentialExpectedGeneration,
  validateCreateApplicationPublicationRouteIntentInput,
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
  applicationBlockingObservationResult,
  applicationStreamingObservationResult,
  applicationAsynchronousObservationResult,
  applicationMessageCitationMutationResult,
  applicationMessageCitationResult,
  applicationMessageCitationsResult,
  applicationMessageFileReferenceMutationResult,
  applicationMessageFileReferenceResult,
  applicationMessageFileReferencesResult,
  applicationDeliveryCredentialMutationResult,
  applicationDeliveryCredentialResult,
  applicationDeliveryCredentialsResult,
  applicationPublicationRouteIntentMutationResult,
  applicationPublicationRouteIntentResult,
  applicationPublicationRouteIntentsResult,
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
    case 'application-blocking-observation observe':
      requireReadCommand(
        arguments_,
        'application-blocking-observation observe <application-id> <session-id> <invocation-id>',
        5
      );
      return applicationBlockingObservationResult(
        await cloudApi().observeApplicationBlockingInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID')
        )
      );

    case 'application-streaming-observation observe':
      requireReadCommand(
        arguments_,
        'application-streaming-observation observe <application-id> <session-id> <invocation-id>',
        5
      );
      return applicationStreamingObservationResult(
        await cloudApi().observeApplicationStreamingInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID'),
          streamingObservationAfterSequence(arguments_)
        )
      );

    case 'application-asynchronous-observation observe':
      requireReadCommand(
        arguments_,
        'application-asynchronous-observation observe <application-id> <session-id> <invocation-id>',
        5
      );
      return applicationAsynchronousObservationResult(
        await cloudApi().observeApplicationAsynchronousInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID')
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
    case 'application-anonymous-sessions open': {
      const mutation = requireAnonymousDeliveryFileMutation(
        arguments_,
        3,
        'application-anonymous-sessions open <application-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Anonymous application session',
        MAX_APPLICATION_CONVERSATION_VARIABLES_BYTES + 16 * 1024,
        dependencies.readFile
      );
      const input = anonymousSessionOpenInput(body);
      validateOpenAnonymousApplicationSessionInput(input);
      return applicationSessionMutationResult(
        await cloudApi().openAnonymousApplicationSession(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          input
        )
      );
    }
    case 'application-anonymous-invocations request': {
      const mutation = requireAnonymousDeliveryFileMutation(
        arguments_,
        4,
        'application-anonymous-invocations request <application-id> <session-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Anonymous application invocation',
        MAX_APPLICATION_INVOCATION_INPUT_BYTES + 16 * 1024,
        dependencies.readFile
      );
      const input = anonymousInvocationRequestInput(body);
      validateRequestAnonymousApplicationInvocationInput(input);
      return applicationInvocationMutationResult(
        await cloudApi().requestAnonymousApplicationInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          input
        )
      );
    }
    case 'application-anonymous-blocking-observation observe': {
      requireReadCommand(
        arguments_,
        'application-anonymous-blocking-observation observe <application-id> <session-id> <invocation-id> --lookup-key KEY',
        5
      );
      const input = anonymousObservationLookupInput(arguments_);
      validateObserveAnonymousApplicationInvocationInput(input);
      return applicationBlockingObservationResult(
        await cloudApi().observeAnonymousApplicationBlockingInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID'),
          input
        )
      );
    }
    case 'application-anonymous-streaming-observation observe': {
      requireReadCommand(
        arguments_,
        'application-anonymous-streaming-observation observe <application-id> <session-id> <invocation-id> --lookup-key KEY',
        5
      );
      const input = anonymousObservationLookupInput(arguments_);
      validateObserveAnonymousApplicationInvocationInput(input);
      return applicationStreamingObservationResult(
        await cloudApi().observeAnonymousApplicationStreamingInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID'),
          input,
          streamingObservationAfterSequence(arguments_)
        )
      );
    }
    case 'application-anonymous-asynchronous-observation observe': {
      requireReadCommand(
        arguments_,
        'application-anonymous-asynchronous-observation observe <application-id> <session-id> <invocation-id> --lookup-key KEY',
        5
      );
      const input = anonymousObservationLookupInput(arguments_);
      validateObserveAnonymousApplicationInvocationInput(input);
      return applicationAsynchronousObservationResult(
        await cloudApi().observeAnonymousApplicationAsynchronousInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID'),
          input
        )
      );
    }
    case 'application-anonymous-sessions close': {
      requireArity(
        arguments_.positionals,
        4,
        'application-anonymous-sessions close <application-id> <session-id> --expected-version N --lookup-key KEY'
      );
      const input = anonymousLifecycleMutationInput(arguments_);
      validateCloseAnonymousApplicationSessionInput(input);
      return applicationSessionMutationResult(
        await cloudApi().closeAnonymousApplicationSession(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          input
        )
      );
    }
    case 'application-anonymous-invocations cancel': {
      requireArity(
        arguments_.positionals,
        5,
        'application-anonymous-invocations cancel <application-id> <session-id> <invocation-id> --expected-version N --lookup-key KEY'
      );
      const input = anonymousLifecycleMutationInput(arguments_);
      validateCancelAnonymousApplicationInvocationInput(input);
      return applicationInvocationCancellationResult(
        await cloudApi().cancelAnonymousApplicationInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID'),
          input
        )
      );
    }

    case 'application-delivery-sessions open': {
      const mutation = requireApplicationJsonMutation(
        arguments_,
        4,
        'application-delivery-sessions open <application-id> <release-id>',
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
        await cloudApi().openDeliveryApplicationSession(
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
    case 'application-delivery-sessions close': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        4,
        'application-delivery-sessions close <application-id> <session-id>',
        'Application delivery session'
      );
      return applicationSessionMutationResult(
        await cloudApi().closeDeliveryApplicationSession(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          { expectedVersion: mutation.expectedVersion },
          mutation.idempotencyKey
        )
      );
    }
    case 'application-delivery-invocations request': {
      const mutation = requireApplicationJsonMutation(
        arguments_,
        4,
        'application-delivery-invocations request <application-id> <session-id>',
        true
      );
      const transport = await readBoundedJsonFile(
        mutation.file,
        {
          label: 'Application delivery invocation request',
          maximumBytes: MAX_APPLICATION_INVOCATION_INPUT_BYTES + 16 * 1024,
        },
        dependencies.readFile
      );
      return applicationInvocationMutationResult(
        await cloudApi().requestDeliveryApplicationInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          applicationInvocationInput(transport),
          mutation.idempotencyKey
        )
      );
    }
    case 'application-delivery-invocations cancel': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        5,
        'application-delivery-invocations cancel <application-id> <session-id> <invocation-id>',
        'Application delivery invocation'
      );
      return applicationInvocationCancellationResult(
        await cloudApi().cancelDeliveryApplicationInvocation(
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
    case 'application-delivery-blocking-observation observe':
      requireReadCommand(
        arguments_,
        'application-delivery-blocking-observation observe <application-id> <session-id> <invocation-id>',
        5
      );
      return applicationBlockingObservationResult(
        await cloudApi().observeDeliveryApplicationBlockingInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID')
        )
      );
    case 'application-delivery-streaming-observation observe':
      requireReadCommand(
        arguments_,
        'application-delivery-streaming-observation observe <application-id> <session-id> <invocation-id>',
        5
      );
      return applicationStreamingObservationResult(
        await cloudApi().observeDeliveryApplicationStreamingInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID'),
          streamingObservationAfterSequence(arguments_)
        )
      );
    case 'application-delivery-asynchronous-observation observe':
      requireReadCommand(
        arguments_,
        'application-delivery-asynchronous-observation observe <application-id> <session-id> <invocation-id>',
        5
      );
      return applicationAsynchronousObservationResult(
        await cloudApi().observeDeliveryApplicationAsynchronousInvocation(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application session ID'),
          positionalUuid(positionals, 4, 'Application invocation ID')
        )
      );

    case 'application-delivery-credentials register': {
      const mutation = requireDeliveryCredentialRegister(
        arguments_,
        'application-delivery-credentials register <application-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Application delivery credential',
        16 * 1024,
        dependencies.readFile
      );
      const input = applicationDeliveryCredentialRegisterInput(body);
      validateRegisterApplicationDeliveryCredentialInput(input);
      return applicationDeliveryCredentialMutationResult(
        await cloudApi().registerApplicationDeliveryCredential(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          input
        )
      );
    }
    case 'application-delivery-credentials list':
      requireReadCommand(
        arguments_,
        'application-delivery-credentials list <application-id>',
        3
      );
      return applicationDeliveryCredentialsResult(
        await cloudApi().listApplicationDeliveryCredentials(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID')
        )
      );
    case 'application-delivery-credentials get':
      requireReadCommand(
        arguments_,
        'application-delivery-credentials get <application-id> <credential-id>',
        4
      );
      return applicationDeliveryCredentialResult(
        await cloudApi().getApplicationDeliveryCredential(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application delivery credential ID')
        )
      );
    case 'application-delivery-credentials disable':
    case 'application-delivery-credentials enable':
    case 'application-delivery-credentials revoke': {
      const action = command.split(' ')[1]!;
      requireArity(
        positionals,
        4,
        `application-delivery-credentials ${action} <application-id> <credential-id>`
      );
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      rejectLogOptions(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (arguments_.stream !== undefined) {
        throw usageError('--stream is not valid for Application delivery credential lifecycle');
      }
      const expectedGeneration = requireDeliveryCredentialExpectedGeneration(arguments_);
      validateApplicationDeliveryCredentialExpectedGeneration(expectedGeneration);
      const applicationId = positionalUuid(positionals, 2, 'Application ID');
      const credentialId = positionalUuid(
        positionals,
        3,
        'Application delivery credential ID'
      );
      const input = { expectedGeneration };
      const api = cloudApi();
      const result =
        action === 'disable'
          ? await api.disableApplicationDeliveryCredential(
              organizationId(),
              projectId(),
              applicationId,
              credentialId,
              input
            )
          : action === 'enable'
            ? await api.enableApplicationDeliveryCredential(
                organizationId(),
                projectId(),
                applicationId,
                credentialId,
                input
              )
            : await api.revokeApplicationDeliveryCredential(
                organizationId(),
                projectId(),
                applicationId,
                credentialId,
                input
              );
      return applicationDeliveryCredentialMutationResult(result);
    }
    case 'application-publication-route-intents create': {
      const mutation = requirePublicationRouteIntentCreate(
        arguments_,
        'application-publication-route-intents create <application-id> <release-id>'
      );
      const body = await readApplicationObject(
        mutation.file,
        'Application publication route intent',
        16 * 1024,
        dependencies.readFile
      );
      const input = applicationPublicationRouteIntentCreateInput(body);
      validateCreateApplicationPublicationRouteIntentInput(input);
      return applicationPublicationRouteIntentMutationResult(
        await cloudApi().createApplicationPublicationRouteIntent(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application release ID'),
          input
        )
      );
    }
    case 'application-publication-route-intents get':
      requireReadCommand(
        arguments_,
        'application-publication-route-intents get <application-id> <intent-id>',
        4
      );
      return applicationPublicationRouteIntentResult(
        await cloudApi().getApplicationPublicationRouteIntent(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application publication route intent ID')
        )
      );
    case 'application-publication-route-intents list': {
      requireArity(
        positionals,
        5,
        'application-publication-route-intents list <application-id> <release-id> <application-release-digest>'
      );
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      rejectLogOptions(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      if (arguments_.stream !== undefined) {
        throw usageError('--stream is not valid for Application publication route intent list');
      }
      const digest = positionals[4]!;
      return applicationPublicationRouteIntentsResult(
        await cloudApi().listApplicationPublicationRouteIntentsByRelease(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application release ID'),
          digest
        )
      );
    }
    default:
      return undefined;
  }
}


function requireAnonymousDeliveryFileMutation(
  arguments_: ParsedArguments,
  arity: number,
  usage: string
): { file: string } {
  requireArity(arguments_.positionals, arity, usage);
  rejectIdempotencyOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectLogOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is not valid for anonymous Application delivery');
  }
  const file = arguments_.file;
  if (file === undefined || file.length === 0) {
    throw usageError(`--file is required for ${usage}`);
  }
  return { file };
}

function anonymousSessionOpenInput(
  value: Record<string, unknown>
): OpenAnonymousApplicationSessionInput {
  for (const key of Object.keys(value)) {
    if (
      key !== 'sessionId' &&
      key !== 'releaseId' &&
      key !== 'lookupKey' &&
      key !== 'initialVariables'
    ) {
      throw usageError(`unknown anonymous session open field: ${key}`);
    }
  }
  if (typeof value.sessionId !== 'string' || value.sessionId.length === 0) {
    throw usageError('anonymous session open requires sessionId');
  }
  if (typeof value.releaseId !== 'string' || value.releaseId.length === 0) {
    throw usageError('anonymous session open requires releaseId');
  }
  if (typeof value.lookupKey !== 'string' || value.lookupKey.length === 0) {
    throw usageError('anonymous session open requires lookupKey');
  }
  const initialVariables = value.initialVariables;
  if (initialVariables !== undefined && !isJsonObject(initialVariables)) {
    throw usageError('anonymous session open initialVariables must be a JSON object');
  }
  return {
    sessionId: value.sessionId,
    releaseId: value.releaseId,
    lookupKey: value.lookupKey,
    initialVariables: initialVariables as Record<string, unknown> | undefined,
  };
}

function anonymousInvocationRequestInput(
  value: Record<string, unknown>
): RequestAnonymousApplicationInvocationInput {
  const allowed = new Set([
    'invocationId',
    'expectedSessionVersion',
    'lookupKey',
    'responseMode',
    'input',
    'ontologyId',
    'ontologyRevisionId',
    'ontologyDigest',
    'environmentId',
    'timeoutSeconds',
  ]);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw usageError(`unknown anonymous invocation request field: ${key}`);
    }
  }
  for (const key of [
    'invocationId',
    'lookupKey',
    'responseMode',
    'ontologyId',
    'ontologyRevisionId',
    'ontologyDigest',
  ] as const) {
    if (typeof value[key] !== 'string' || (value[key] as string).length === 0) {
      throw usageError(`anonymous invocation request requires ${key}`);
    }
  }
  if (
    typeof value.expectedSessionVersion !== 'number' ||
    !Number.isSafeInteger(value.expectedSessionVersion) ||
    (value.expectedSessionVersion as number) < 1
  ) {
    throw usageError('anonymous invocation request requires expectedSessionVersion');
  }
  if (!isJsonObject(value.input)) {
    throw usageError('anonymous invocation request requires input object');
  }
  if (value.environmentId !== undefined && typeof value.environmentId !== 'string') {
    throw usageError('anonymous invocation request environmentId must be a string');
  }
  if (value.timeoutSeconds !== undefined) {
    if (
      typeof value.timeoutSeconds !== 'number' ||
      !Number.isSafeInteger(value.timeoutSeconds) ||
      (value.timeoutSeconds as number) < 1
    ) {
      throw usageError('anonymous invocation request timeoutSeconds must be a positive integer');
    }
  }
  return {
    invocationId: value.invocationId as string,
    expectedSessionVersion: value.expectedSessionVersion as number,
    lookupKey: value.lookupKey as string,
    responseMode: value.responseMode as RequestAnonymousApplicationInvocationInput['responseMode'],
    input: value.input as Record<string, unknown>,
    ontologyId: value.ontologyId as string,
    ontologyRevisionId: value.ontologyRevisionId as string,
    ontologyDigest: value.ontologyDigest as string,
    environmentId: value.environmentId as string | undefined,
    timeoutSeconds: value.timeoutSeconds as number | undefined,
  };
}

function requireDeliveryCredentialRegister(
  arguments_: ParsedArguments,
  usage: string
): { file: string } {
  requireArity(arguments_.positionals, 3, usage);
  rejectIdempotencyOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectLogOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is not valid for Application delivery credential register');
  }
  const file = arguments_.file;
  if (file === undefined || file.length === 0) {
    throw usageError(`--file is required for ${usage}`);
  }
  return { file };
}

function requirePublicationRouteIntentCreate(
  arguments_: ParsedArguments,
  usage: string
): { file: string } {
  requireArity(arguments_.positionals, 4, usage);
  rejectIdempotencyOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectLogOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is not valid for Application publication route intent create');
  }
  const file = arguments_.file;
  if (file === undefined || file.length === 0) {
    throw usageError(`--file is required for ${usage}`);
  }
  return { file };
}

function applicationPublicationRouteIntentCreateInput(
  value: Record<string, unknown>
): CreateApplicationPublicationRouteIntentInput {
  for (const key of Object.keys(value)) {
    if (
      key !== 'applicationReleaseDigest' &&
      key !== 'channels' &&
      key !== 'embedOriginAllowlist' &&
      key !== 'rateShapingPolicy'
    ) {
      throw usageError(`Application publication route intent field '${key}' is not supported`);
    }
  }
  const applicationReleaseDigest = value.applicationReleaseDigest;
  const channels = value.channels;
  const embedOriginAllowlist = value.embedOriginAllowlist;
  const rateShapingPolicy = value.rateShapingPolicy;
  if (typeof applicationReleaseDigest !== 'string') {
    throw usageError('Application publication route intent applicationReleaseDigest must be a string');
  }
  if (!Array.isArray(channels)) {
    throw usageError('Application publication route intent channels must be an array');
  }
  if (embedOriginAllowlist !== undefined && !Array.isArray(embedOriginAllowlist)) {
    throw usageError('Application publication route intent embedOriginAllowlist must be an array');
  }
  if (
    rateShapingPolicy === null ||
    Array.isArray(rateShapingPolicy) ||
    typeof rateShapingPolicy !== 'object'
  ) {
    throw usageError('Application publication route intent rateShapingPolicy must be an object');
  }
  const policy = rateShapingPolicy as Record<string, unknown>;
  for (const key of Object.keys(policy)) {
    if (key !== 'profileId' && key !== 'policyRevisionDigest') {
      throw usageError(
        `Application publication route intent rateShapingPolicy field '${key}' is not supported`
      );
    }
  }
  if (typeof policy.profileId !== 'string') {
    throw usageError('Application publication route intent rateShapingPolicy.profileId must be a string');
  }
  if (typeof policy.policyRevisionDigest !== 'string') {
    throw usageError(
      'Application publication route intent rateShapingPolicy.policyRevisionDigest must be a string'
    );
  }
  return {
    applicationReleaseDigest,
    channels: channels as ApplicationPublicationChannel[],
    embedOriginAllowlist:
      embedOriginAllowlist === undefined ? undefined : (embedOriginAllowlist as string[]),
    rateShapingPolicy: {
      profileId: policy.profileId,
      policyRevisionDigest: policy.policyRevisionDigest,
    },
  };
}

function requireDeliveryCredentialExpectedGeneration(arguments_: ParsedArguments): number {
  const raw = arguments_.expectedVersion;
  if (raw === undefined || !/^[0-9]+$/u.test(raw)) {
    throw usageError(
      '--expected-version must be a positive safe integer for Application delivery credential lifecycle mutation'
    );
  }
  const expectedGeneration = Number(raw);
  if (!Number.isSafeInteger(expectedGeneration) || expectedGeneration < 1) {
    throw usageError(
      '--expected-version must be a positive safe integer for Application delivery credential lifecycle mutation'
    );
  }
  return expectedGeneration;
}

function applicationDeliveryCredentialRegisterInput(
  value: Record<string, unknown>
): RegisterApplicationDeliveryCredentialInput {
  for (const key of Object.keys(value)) {
    if (
      key !== 'credentialId' &&
      key !== 'applicationReleaseId' &&
      key !== 'lookupKey' &&
      key !== 'secretId' &&
      key !== 'secretVersion'
    ) {
      throw usageError(`Application delivery credential field '${key}' is not supported`);
    }
  }
  const credentialId = value.credentialId;
  const applicationReleaseId = value.applicationReleaseId;
  const lookupKey = value.lookupKey;
  const secretId = value.secretId;
  const secretVersion = value.secretVersion;
  if (typeof credentialId !== 'string') {
    throw usageError('Application delivery credential credentialId must be a string');
  }
  if (typeof applicationReleaseId !== 'string') {
    throw usageError('Application delivery credential applicationReleaseId must be a string');
  }
  if (typeof lookupKey !== 'string') {
    throw usageError('Application delivery credential lookupKey must be a string');
  }
  if (typeof secretId !== 'string') {
    throw usageError('Application delivery credential secretId must be a string');
  }
  if (typeof secretVersion !== 'number') {
    throw usageError('Application delivery credential secretVersion must be a number');
  }
  return {
    credentialId,
    applicationReleaseId,
    lookupKey,
    secretId,
    secretVersion,
  };
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



function anonymousLifecycleMutationInput(arguments_: ParsedArguments): {
  expectedVersion: number;
  lookupKey: string;
} {
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectLogOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is not valid for anonymous Application lifecycle mutation');
  }
  const lookupKey = arguments_.lookupKey;
  if (typeof lookupKey !== 'string' || lookupKey.length === 0) {
    throw usageError('anonymous lifecycle mutation requires --lookup-key');
  }
  const raw = arguments_.expectedVersion;
  if (raw === undefined || !/^[0-9]+$/u.test(raw)) {
    throw usageError(
      '--expected-version must be a positive safe integer for anonymous Application lifecycle mutation'
    );
  }
  const expectedVersion = Number(raw);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError(
      '--expected-version must be a positive safe integer for anonymous Application lifecycle mutation'
    );
  }
  return { expectedVersion, lookupKey };
}

function anonymousObservationLookupInput(arguments_: ParsedArguments): {
  lookupKey: string;
} {
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is not valid for anonymous Application observation');
  }
  const lookupKey = arguments_.lookupKey;
  if (typeof lookupKey !== 'string' || lookupKey.length === 0) {
    throw usageError('anonymous observation requires --lookup-key');
  }
  return { lookupKey };
}

function streamingObservationAfterSequence(arguments_: ParsedArguments): number | undefined {
  if (arguments_.afterSequence === undefined) {
    return undefined;
  }
  return boundedApplicationMessageInteger(
    arguments_.afterSequence,
    'Application streaming observation after-sequence',
    0,
    Number.MAX_SAFE_INTEGER,
    0
  );
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
