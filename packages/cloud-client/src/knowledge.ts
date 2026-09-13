export const KNOWLEDGE_BASE_REVISION_SCHEMA = 'cloud.knowledge-base-revision.v1' as const;
export const KNOWLEDGE_PIPELINE_RELEASE_SCHEMA = 'cloud.knowledge-pipeline-release.v1' as const;
export const KNOWLEDGE_CONTRACT_MAX_ACL_BYTES = 64 * 1024;
export const DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT = 50;
export const MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT = 200;
export const DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT = 50;
export const MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT = 200;

const CONTENT_DIGEST_PATTERN = /^sha256:[0-9a-f]{64}$/;

export interface KnowledgeBase {
  organizationId: string;
  projectId: string;
  knowledgeBaseId: string;
  revisionId: string;
  generation: number;
  name: string;
  contractSchema: typeof KNOWLEDGE_BASE_REVISION_SCHEMA;
  revisionAcl: string;
  revisionDigest: string;
  createdAt: string;
  updatedAt: string;
}

export interface KnowledgeBaseMutationResult {
  knowledgeBase: KnowledgeBase;
  replayed: boolean;
}

export interface KnowledgePipeline {
  organizationId: string;
  projectId: string;
  pipelineId: string;
  releaseId: string;
  name: string;
  contractSchema: typeof KNOWLEDGE_PIPELINE_RELEASE_SCHEMA;
  releaseAcl: string;
  releaseDigest: string;
  createdAt: string;
  updatedAt: string;
}

export interface KnowledgePipelineMutationResult {
  knowledgePipeline: KnowledgePipeline;
  replayed: boolean;
}

export interface KnowledgeListOptions {
  limit?: number;
}

export interface CreateKnowledgeBaseInput {
  revisionAcl: string;
}

export interface AppendKnowledgeBaseInput {
  expectedRevisionDigest: string;
  revisionAcl: string;
}

export interface CreateKnowledgePipelineInput {
  releaseAcl: string;
}

export interface PublishKnowledgePipelineInput {
  expectedReleaseDigest: string;
  releaseAcl: string;
}

export function validateKnowledgeContractAcl(value: unknown, label: string): asserts value is string {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    new TextEncoder().encode(value).byteLength > KNOWLEDGE_CONTRACT_MAX_ACL_BYTES ||
    /\r(?!\n)/.test(value)
  ) {
    throw new TypeError(
      `${label} must be a bounded canonical .acl document of at most ${KNOWLEDGE_CONTRACT_MAX_ACL_BYTES} bytes`
    );
  }
}

export function validateKnowledgeContentDigest(value: unknown, label: string): asserts value is string {
  if (typeof value !== 'string' || !CONTENT_DIGEST_PATTERN.test(value)) {
    throw new TypeError(`${label} must match ^sha256:[0-9a-f]{64}$`);
  }
}

export function encodeKnowledgeBaseListOptions(options: KnowledgeListOptions = {}): string {
  return encodeKnowledgeListOptions(
    options,
    DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
    'KnowledgeBase'
  );
}

export function encodeKnowledgePipelineListOptions(options: KnowledgeListOptions = {}): string {
  return encodeKnowledgeListOptions(
    options,
    DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
    'KnowledgePipeline'
  );
}

function encodeKnowledgeListOptions(
  options: KnowledgeListOptions,
  defaultLimit: number,
  maximumLimit: number,
  label: string
): string {
  const limit = options.limit ?? defaultLimit;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > maximumLimit) {
    throw new RangeError(`${label} list limit must be between 1 and ${maximumLimit}`);
  }
  return `?limit=${limit}`;
}


export const KNOWLEDGE_DOCUMENT_SCHEMA = 'cloud.knowledge-document.v1' as const;
export const KNOWLEDGE_CHUNK_SCHEMA = 'cloud.knowledge-chunk.v1' as const;

export interface KnowledgeDocument {
  organizationId: string;
  projectId: string;
  documentId: string;
  knowledgeBaseId: string;
  knowledgeBaseRevisionId: string;
  title: string;
  contractSchema: typeof KNOWLEDGE_DOCUMENT_SCHEMA;
  documentAcl: string;
  documentDigest: string;
  createdAt: string;
}

export interface KnowledgeDocumentMutationResult {
  knowledgeDocument: KnowledgeDocument;
  replayed: boolean;
}

export interface KnowledgeChunk {
  organizationId: string;
  projectId: string;
  documentId: string;
  chunkId: string;
  ordinal: number;
  contractSchema: typeof KNOWLEDGE_CHUNK_SCHEMA;
  chunkAcl: string;
  chunkDigest: string;
  createdAt: string;
}

export interface KnowledgeChunkMutationResult {
  knowledgeChunk: KnowledgeChunk;
  replayed: boolean;
}

export interface CreateKnowledgeDocumentInput {
  documentAcl: string;
}

export interface CreateKnowledgeChunkInput {
  chunkAcl: string;
}
