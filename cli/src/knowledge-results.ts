import type {
  KnowledgeBase,
  KnowledgeBaseMutationResult,
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
