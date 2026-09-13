import {
  type CloudApi,
  encodeExternalKnowledgeBindingListOptions,
  encodeKnowledgeBaseListOptions,
  encodeKnowledgeChunkListOptions,
  encodeKnowledgeDocumentListOptions,
  encodeKnowledgeIndexRevisionListOptions,
  encodeKnowledgePipelineListOptions,
  encodeKnowledgeRetrievalPolicyRevisionListOptions,
  KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
  type ExternalKnowledgeBindingListOptions,
  type KnowledgeChunkListOptions,
  type KnowledgeDocumentListOptions,
  type KnowledgeIndexRevisionListOptions,
  type KnowledgeListOptions,
  type KnowledgeRetrievalPolicyRevisionListOptions,
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
  requireIdempotencyKey,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import {
  externalKnowledgeBindingMutationResult,
  externalKnowledgeBindingResult,
  externalKnowledgeBindingsResult,
  knowledgeBaseMutationResult,
  knowledgeBaseResult,
  knowledgeBasesResult,
  knowledgeChunkMutationResult,
  knowledgeChunkResult,
  knowledgeChunksResult,
  knowledgeDocumentMutationResult,
  knowledgeDocumentResult,
  knowledgeDocumentsResult,
  knowledgeIndexRevisionMutationResult,
  knowledgeIndexRevisionResult,
  knowledgeIndexRevisionsResult,
  knowledgePipelineMutationResult,
  knowledgePipelineResult,
  knowledgePipelinesResult,
  knowledgeRetrievalPolicyRevisionMutationResult,
  knowledgeRetrievalPolicyRevisionResult,
  knowledgeRetrievalPolicyRevisionsResult,
} from './knowledge-results';
import type { CommandResult } from './results';

interface KnowledgeCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeKnowledgeCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: KnowledgeCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const organizationId = () => requireOrganization(context);
  const projectId = () => requireProject(context);
  switch (command) {
    case 'knowledge-bases create': {
      rejectExpectedDigestOption(arguments_);
      const mutation = requireAclMutationCommand(arguments_, 2, 'knowledge-bases create');
      rejectAgentProviderKindOption(arguments_);
      const revisionAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgeBase revision ACL',
        dependencies.readFile
      );
      return knowledgeBaseMutationResult(
        await cloudApi().createKnowledgeBase(
          organizationId(),
          projectId(),
          { revisionAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-bases list':
      rejectExpectedDigestOption(arguments_);
      requireKnowledgeListCommand(arguments_, 'knowledge-bases list');
      return knowledgeBasesResult(
        await cloudApi().listKnowledgeBases(
          organizationId(),
          projectId(),
          knowledgeListOptions(arguments_, encodeKnowledgeBaseListOptions, 'KnowledgeBase')
        )
      );
    case 'knowledge-bases get':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'knowledge-bases get <knowledge-base-id>');
      return knowledgeBaseResult(
        await cloudApi().getKnowledgeBase(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeBase ID')
        )
      );
    case 'knowledge-bases append': {
      const mutation = requireDigestAclMutationCommand(
        arguments_,
        3,
        'knowledge-bases append <knowledge-base-id>',
        'KnowledgeBase'
      );
      const revisionAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgeBase revision ACL',
        dependencies.readFile
      );
      return knowledgeBaseMutationResult(
        await cloudApi().appendKnowledgeBase(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeBase ID'),
          {
            expectedRevisionDigest: mutation.expectedDigest,
            revisionAcl,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-pipelines create': {
      rejectExpectedDigestOption(arguments_);
      const mutation = requireAclMutationCommand(arguments_, 2, 'knowledge-pipelines create');
      rejectAgentProviderKindOption(arguments_);
      const releaseAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgePipeline release ACL',
        dependencies.readFile
      );
      return knowledgePipelineMutationResult(
        await cloudApi().createKnowledgePipeline(
          organizationId(),
          projectId(),
          { releaseAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-pipelines list':
      rejectExpectedDigestOption(arguments_);
      requireKnowledgeListCommand(arguments_, 'knowledge-pipelines list');
      return knowledgePipelinesResult(
        await cloudApi().listKnowledgePipelines(
          organizationId(),
          projectId(),
          knowledgeListOptions(arguments_, encodeKnowledgePipelineListOptions, 'KnowledgePipeline')
        )
      );
    case 'knowledge-pipelines get':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'knowledge-pipelines get <pipeline-id>');
      return knowledgePipelineResult(
        await cloudApi().getKnowledgePipeline(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgePipeline ID')
        )
      );
    case 'knowledge-pipelines publish': {
      const mutation = requireDigestAclMutationCommand(
        arguments_,
        3,
        'knowledge-pipelines publish <pipeline-id>',
        'KnowledgePipeline'
      );
      const releaseAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgePipeline release ACL',
        dependencies.readFile
      );
      return knowledgePipelineMutationResult(
        await cloudApi().publishKnowledgePipeline(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgePipeline ID'),
          {
            expectedReleaseDigest: mutation.expectedDigest,
            releaseAcl,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-documents create': {
      rejectExpectedDigestOption(arguments_);
      const mutation = requireAclMutationCommand(arguments_, 2, 'knowledge-documents create');
      rejectAgentProviderKindOption(arguments_);
      const documentAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgeDocument ACL',
        dependencies.readFile
      );
      return knowledgeDocumentMutationResult(
        await cloudApi().createKnowledgeDocument(
          organizationId(),
          projectId(),
          { documentAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-documents list':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'knowledge-documents list <knowledge-base-id>');
      return knowledgeDocumentsResult(
        await cloudApi().listKnowledgeDocuments(
          organizationId(),
          projectId(),
          knowledgeDocumentListOptions(
            arguments_,
            positionalUuid(arguments_.positionals, 2, 'KnowledgeBase ID')
          )
        )
      );
    case 'knowledge-documents get':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'knowledge-documents get <document-id>');
      return knowledgeDocumentResult(
        await cloudApi().getKnowledgeDocument(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeDocument ID')
        )
      );
    case 'knowledge-chunks create': {
      rejectExpectedDigestOption(arguments_);
      const mutation = requireAclMutationCommand(arguments_, 3, 'knowledge-chunks create <document-id>');
      rejectAgentProviderKindOption(arguments_);
      const chunkAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgeChunk ACL',
        dependencies.readFile
      );
      return knowledgeChunkMutationResult(
        await cloudApi().createKnowledgeChunk(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeDocument ID'),
          { chunkAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-chunks list':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'knowledge-chunks list <document-id>');
      return knowledgeChunksResult(
        await cloudApi().listKnowledgeChunks(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeDocument ID'),
          knowledgeChunkListOptions(arguments_)
        )
      );
    case 'knowledge-chunks get':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'knowledge-chunks get <chunk-id>');
      return knowledgeChunkResult(
        await cloudApi().getKnowledgeChunk(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeChunk ID')
        )
      );
    case 'knowledge-index-revisions list':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(
        arguments_,
        'knowledge-index-revisions list <knowledge-base-revision-id>'
      );
      return knowledgeIndexRevisionsResult(
        await cloudApi().listKnowledgeIndexRevisions(
          organizationId(),
          projectId(),
          knowledgeIndexRevisionListOptions(
            arguments_,
            positionalUuid(arguments_.positionals, 2, 'KnowledgeBase revision ID')
          )
        )
      );
    case 'knowledge-index-revisions create': {
      rejectExpectedDigestOption(arguments_);
      const mutation = requireAclMutationCommand(arguments_, 2, 'knowledge-index-revisions create');
      rejectAgentProviderKindOption(arguments_);
      const indexAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgeIndexRevision ACL',
        dependencies.readFile
      );
      return knowledgeIndexRevisionMutationResult(
        await cloudApi().createKnowledgeIndexRevision(
          organizationId(),
          projectId(),
          { indexAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-index-revisions get':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'knowledge-index-revisions get <index-revision-id>');
      return knowledgeIndexRevisionResult(
        await cloudApi().getKnowledgeIndexRevision(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeIndexRevision ID')
        )
      );
    case 'knowledge-retrieval-policy-revisions list':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(
        arguments_,
        'knowledge-retrieval-policy-revisions list <knowledge-base-revision-id>'
      );
      return knowledgeRetrievalPolicyRevisionsResult(
        await cloudApi().listKnowledgeRetrievalPolicyRevisions(
          organizationId(),
          projectId(),
          knowledgeRetrievalPolicyRevisionListOptions(
            arguments_,
            positionalUuid(arguments_.positionals, 2, 'KnowledgeBase revision ID')
          )
        )
      );
    case 'knowledge-retrieval-policy-revisions create': {
      rejectExpectedDigestOption(arguments_);
      const mutation = requireAclMutationCommand(
        arguments_,
        2,
        'knowledge-retrieval-policy-revisions create'
      );
      rejectAgentProviderKindOption(arguments_);
      const policyAcl = await readKnowledgeAcl(
        mutation.file,
        'KnowledgeRetrievalPolicyRevision ACL',
        dependencies.readFile
      );
      return knowledgeRetrievalPolicyRevisionMutationResult(
        await cloudApi().createKnowledgeRetrievalPolicyRevision(
          organizationId(),
          projectId(),
          { policyAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'knowledge-retrieval-policy-revisions get':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(
        arguments_,
        'knowledge-retrieval-policy-revisions get <policy-revision-id>'
      );
      return knowledgeRetrievalPolicyRevisionResult(
        await cloudApi().getKnowledgeRetrievalPolicyRevision(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'KnowledgeRetrievalPolicyRevision ID')
        )
      );
    case 'external-knowledge-bindings list':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'external-knowledge-bindings list <knowledge-base-id>');
      return externalKnowledgeBindingsResult(
        await cloudApi().listExternalKnowledgeBindings(
          organizationId(),
          projectId(),
          externalKnowledgeBindingListOptions(
            arguments_,
            positionalUuid(arguments_.positionals, 2, 'KnowledgeBase ID')
          )
        )
      );
    case 'external-knowledge-bindings create': {
      rejectExpectedDigestOption(arguments_);
      const mutation = requireAclMutationCommand(arguments_, 2, 'external-knowledge-bindings create');
      rejectAgentProviderKindOption(arguments_);
      const bindingAcl = await readKnowledgeAcl(
        mutation.file,
        'ExternalKnowledgeBinding ACL',
        dependencies.readFile
      );
      return externalKnowledgeBindingMutationResult(
        await cloudApi().createExternalKnowledgeBinding(
          organizationId(),
          projectId(),
          { bindingAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'external-knowledge-bindings get':
      rejectExpectedDigestOption(arguments_);
      requireReadCommand(arguments_, 'external-knowledge-bindings get <binding-id>');
      return externalKnowledgeBindingResult(
        await cloudApi().getExternalKnowledgeBinding(
          organizationId(),
          projectId(),
          positionalUuid(arguments_.positionals, 2, 'ExternalKnowledgeBinding ID')
        )
      );
    default:
      return undefined;
  }
}

function requireKnowledgeListCommand(arguments_: ParsedArguments, usage: string): void {
  requireArity(arguments_.positionals, 2, usage);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectAgentProviderKindOption(arguments_);
  if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
    throw usageError('cursor and stream options are valid only for log commands');
  }
}


function knowledgeIndexRevisionListOptions(
  arguments_: ParsedArguments,
  knowledgeBaseRevisionId: string
): KnowledgeIndexRevisionListOptions {
  const options: KnowledgeIndexRevisionListOptions = { knowledgeBaseRevisionId };
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError('KnowledgeIndexRevision list limit must be an integer');
    }
    options.limit = Number(arguments_.limit);
  }
  try {
    encodeKnowledgeIndexRevisionListOptions(options);
  } catch (error) {
    if (error instanceof Error) {
      throw usageError(error.message);
    }
    throw error;
  }
  return options;
}

function knowledgeRetrievalPolicyRevisionListOptions(
  arguments_: ParsedArguments,
  knowledgeBaseRevisionId: string
): KnowledgeRetrievalPolicyRevisionListOptions {
  const options: KnowledgeRetrievalPolicyRevisionListOptions = { knowledgeBaseRevisionId };
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError('KnowledgeRetrievalPolicyRevision list limit must be an integer');
    }
    options.limit = Number(arguments_.limit);
  }
  try {
    encodeKnowledgeRetrievalPolicyRevisionListOptions(options);
  } catch (error) {
    if (error instanceof Error) {
      throw usageError(error.message);
    }
    throw error;
  }
  return options;
}

function externalKnowledgeBindingListOptions(
  arguments_: ParsedArguments,
  knowledgeBaseId: string
): ExternalKnowledgeBindingListOptions {
  const options: ExternalKnowledgeBindingListOptions = { knowledgeBaseId };
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError('ExternalKnowledgeBinding list limit must be an integer');
    }
    options.limit = Number(arguments_.limit);
  }
  try {
    encodeExternalKnowledgeBindingListOptions(options);
  } catch (error) {
    if (error instanceof Error) {
      throw usageError(error.message);
    }
    throw error;
  }
  return options;
}
function knowledgeDocumentListOptions(
  arguments_: ParsedArguments,
  knowledgeBaseId: string
): KnowledgeDocumentListOptions {
  const options: KnowledgeDocumentListOptions = { knowledgeBaseId };
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError('KnowledgeDocument list limit must be an integer');
    }
    options.limit = Number(arguments_.limit);
  }
  try {
    encodeKnowledgeDocumentListOptions(options);
  } catch (error) {
    if (error instanceof Error) {
      throw usageError(error.message);
    }
    throw error;
  }
  return options;
}

function knowledgeChunkListOptions(arguments_: ParsedArguments): KnowledgeChunkListOptions {
  return knowledgeListOptions(arguments_, encodeKnowledgeChunkListOptions, 'KnowledgeChunk');
}

function knowledgeListOptions(
  arguments_: ParsedArguments,
  encode: (options: KnowledgeListOptions) => string,
  label: string
): KnowledgeListOptions {
  let limit: number | undefined;
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError(`${label} list limit must be an integer`);
    }
    limit = Number(arguments_.limit);
  }
  const options = { limit };
  try {
    encode(options);
  } catch (error) {
    if (error instanceof Error) {
      throw usageError(error.message);
    }
    throw error;
  }
  return options;
}

function requireDigestAclMutationCommand(
  arguments_: ParsedArguments,
  arity: number,
  usage: string,
  label: string
): { expectedDigest: string; idempotencyKey: string; file: string } {
  requireArity(arguments_.positionals, arity, usage);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectAgentProviderKindOption(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const file = arguments_.file;
  if (file === undefined || file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError(`--file with a valid A3S ACL path is required for ${label} mutation`);
  }
  const expectedDigest = arguments_.expectedDigest;
  if (expectedDigest === undefined || !/^sha256:[0-9a-f]{64}$/.test(expectedDigest)) {
    throw usageError(`--expected-digest must match ^sha256:[0-9a-f]{64}$ for ${label} mutation`);
  }
  return { expectedDigest, idempotencyKey, file };
}

function rejectExpectedDigestOption(arguments_: ParsedArguments): void {
  if (arguments_.expectedDigest !== undefined) {
    throw usageError(
      '--expected-digest is valid only for knowledge-bases append and knowledge-pipelines publish'
    );
  }
}

function readKnowledgeAcl(
  path: string,
  label: string,
  readFile?: (path: string) => Promise<Uint8Array>
): Promise<string> {
  return readAclDocument(path, { label, maximumBytes: KNOWLEDGE_CONTRACT_MAX_ACL_BYTES }, readFile);
}
