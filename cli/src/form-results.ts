import type {
  FormDraft,
  FormDraftMutationResult,
  FormPublicationMutationResult,
  FormRelease,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const FORM_DRAFT_COLUMNS = [
  { header: 'ID', value: (row: FormDraft) => row.id },
  { header: 'NAME', value: (row: FormDraft) => row.name },
  { header: 'VERSION', value: (row: FormDraft) => row.aggregateVersion },
  { header: 'LATEST RELEASE', value: (row: FormDraft) => row.latestRelease?.revision },
  { header: 'DRAFT DIGEST', value: (row: FormDraft) => row.draftDigest },
  { header: 'UPDATED AT', value: (row: FormDraft) => row.updatedAt },
] as const;

export function formDraftsResult(rows: FormDraft[]): CommandResult {
  return { json: rows, table: renderTable(rows, FORM_DRAFT_COLUMNS) };
}

export function formDraftResult(row: FormDraft): CommandResult {
  return { json: row, table: renderTable([row], FORM_DRAFT_COLUMNS) };
}

export function formDraftMutationResult(row: FormDraftMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.form.id },
        { header: 'NAME', value: (value) => value.form.name },
        { header: 'VERSION', value: (value) => value.form.aggregateVersion },
        { header: 'DRAFT DIGEST', value: (value) => value.form.draftDigest },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

const FORM_RELEASE_COLUMNS = [
  { header: 'ID', value: (row: FormRelease) => row.id },
  { header: 'REVISION', value: (row: FormRelease) => row.revision },
  { header: 'SOURCE VERSION', value: (row: FormRelease) => row.sourceDraftVersion },
  { header: 'DIGEST', value: (row: FormRelease) => row.contentDigest },
  { header: 'COMPILER', value: (row: FormRelease) => row.compilerRevision },
  { header: 'PUBLISHED AT', value: (row: FormRelease) => row.publishedAt },
] as const;

export function formReleasesResult(rows: FormRelease[]): CommandResult {
  return { json: rows, table: renderTable(rows, FORM_RELEASE_COLUMNS) };
}

export function formReleaseResult(row: FormRelease): CommandResult {
  return { json: row, table: renderTable([row], FORM_RELEASE_COLUMNS) };
}

export function formPublicationMutationResult(row: FormPublicationMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'FORM', value: (value) => value.form.id },
        { header: 'RELEASE', value: (value) => value.release.id },
        { header: 'REVISION', value: (value) => value.release.revision },
        { header: 'DIGEST', value: (value) => value.release.contentDigest },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}
