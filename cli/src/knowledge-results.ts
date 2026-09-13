import type {
  KnowledgeBase,
  KnowledgeBaseMutationResult,
  KnowledgeChunk,
  KnowledgeChunkMutationResult,
  KnowledgeDocument,
  KnowledgeDocumentMutationResult,
  KnowledgePipeline,
  KnowledgePipelineMutationResult,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const KNOWLEDGE_BASE_COLUMNS: readonly TableColumn<KnowledgeBase>[] = [
  { header: 'ID', value: (row) => row.knowledgeBaseId },
  { header: 'NAME', value: (row) => row.name },
  { header: 'GENERATION', value: (row) => row.generation },
  { header: 'REVISION', value: (row) => row.revisionId },
  { header: 'DIGEST', value: (row) => row.revisionDigest },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

const KNOWLEDGE_PIPELINE_COLUMNS: readonly TableColumn<KnowledgePipeline>[] = [
  { header: 'ID', value: (row) => row.pipelineId },
  { header: 'NAME', value: (row) => row.name },
  { header: 'RELEASE', value: (row) => row.releaseId },
  { header: 'DIGEST', value: (row) => row.releaseDigest },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

export function knowledgeBasesResult(rows: KnowledgeBase[]): CommandResult {
  return { json: rows, table: renderTable(rows, KNOWLEDGE_BASE_COLUMNS) };
}

export function knowledgeBaseResult(row: KnowledgeBase): CommandResult {
  return { json: row, table: renderTable([row], KNOWLEDGE_BASE_COLUMNS) };
}

export function knowledgeBaseMutationResult(result: KnowledgeBaseMutationResult): CommandResult {
  const row = { ...result.knowledgeBase, replayed: result.replayed };
  return {
    json: result,
    table: renderTable(
      [row],
      [...KNOWLEDGE_BASE_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

export function knowledgePipelinesResult(rows: KnowledgePipeline[]): CommandResult {
  return { json: rows, table: renderTable(rows, KNOWLEDGE_PIPELINE_COLUMNS) };
}

export function knowledgePipelineResult(row: KnowledgePipeline): CommandResult {
  return { json: row, table: renderTable([row], KNOWLEDGE_PIPELINE_COLUMNS) };
}

export function knowledgePipelineMutationResult(result: KnowledgePipelineMutationResult): CommandResult {
  const row = { ...result.knowledgePipeline, replayed: result.replayed };
  return {
    json: result,
    table: renderTable(
      [row],
      [...KNOWLEDGE_PIPELINE_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}
const KNOWLEDGE_DOCUMENT_COLUMNS: readonly TableColumn<KnowledgeDocument>[] = [
  { header: 'ID', value: (row) => row.documentId },
  { header: 'TITLE', value: (row) => row.title },
  { header: 'BASE', value: (row) => row.knowledgeBaseId },
  { header: 'REVISION', value: (row) => row.knowledgeBaseRevisionId },
  { header: 'DIGEST', value: (row) => row.documentDigest },
  { header: 'CREATED AT', value: (row) => row.createdAt },
];

const KNOWLEDGE_CHUNK_COLUMNS: readonly TableColumn<KnowledgeChunk>[] = [
  { header: 'ID', value: (row) => row.chunkId },
  { header: 'DOCUMENT', value: (row) => row.documentId },
  { header: 'ORDINAL', value: (row) => row.ordinal },
  { header: 'DIGEST', value: (row) => row.chunkDigest },
  { header: 'CREATED AT', value: (row) => row.createdAt },
];

export function knowledgeDocumentsResult(rows: KnowledgeDocument[]): CommandResult {
  return { json: rows, table: renderTable(rows, KNOWLEDGE_DOCUMENT_COLUMNS) };
}

export function knowledgeDocumentResult(row: KnowledgeDocument): CommandResult {
  return { json: row, table: renderTable([row], KNOWLEDGE_DOCUMENT_COLUMNS) };
}

export function knowledgeDocumentMutationResult(result: KnowledgeDocumentMutationResult): CommandResult {
  const row = { ...result.knowledgeDocument, replayed: result.replayed };
  return {
    json: result,
    table: renderTable(
      [row],
      [...KNOWLEDGE_DOCUMENT_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

export function knowledgeChunksResult(rows: KnowledgeChunk[]): CommandResult {
  return { json: rows, table: renderTable(rows, KNOWLEDGE_CHUNK_COLUMNS) };
}

export function knowledgeChunkResult(row: KnowledgeChunk): CommandResult {
  return { json: row, table: renderTable([row], KNOWLEDGE_CHUNK_COLUMNS) };
}

export function knowledgeChunkMutationResult(result: KnowledgeChunkMutationResult): CommandResult {
  const row = { ...result.knowledgeChunk, replayed: result.replayed };
  return {
    json: result,
    table: renderTable(
      [row],
      [...KNOWLEDGE_CHUNK_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}
