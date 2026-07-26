import type {
  GithubConnection,
  GithubConnectionInstall,
  GithubRepositorySubscription,
  GithubRepositorySubscriptionMutationResult,
  SourceRevision,
  SourceRevisionMutationResult,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const SOURCE_REVISION_COLUMNS: readonly TableColumn<SourceRevision>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'REPOSITORY', value: (row) => row.repository.identity },
  { header: 'COMMIT', value: (row) => row.commitSha },
  { header: 'RECIPE', value: (row) => row.recipeDigest },
  { header: 'PLATFORMS', value: (row) => row.recipe.platforms.join(',') },
  { header: 'ACCEPTED AT', value: (row) => row.acceptedAt },
];

export function sourceRevisionsResult(rows: SourceRevision[]): CommandResult {
  return listResult(rows, SOURCE_REVISION_COLUMNS);
}

export function sourceRevisionMutationResult(row: SourceRevisionMutationResult): CommandResult {
  return singleResult(row, [
    ...SOURCE_REVISION_COLUMNS,
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

export function githubConnectionResult(row: GithubConnection): CommandResult {
  return singleResult(row, [
    { header: 'ID', value: (value) => value.id },
    { header: 'ACCOUNT', value: (value) => value.account.login },
    { header: 'STATUS', value: (value) => value.status },
    { header: 'INSTALLATION', value: (value) => value.installationId },
    { header: 'AUTHORITY CHECKED', value: (value) => value.providerAuthority.checkedAt },
    { header: 'FAILURES', value: (value) => value.providerAuthority.consecutiveFailures },
    { header: 'LAST ERROR', value: (value) => value.providerAuthority.lastError },
  ]);
}

export function githubConnectionInstallResult(row: GithubConnectionInstall): CommandResult {
  return singleResult(row, [
    { header: 'PROVIDER', value: (value) => value.provider },
    { header: 'INSTALLATION URL', value: (value) => value.installationUrl },
    { header: 'EXPIRES AT', value: (value) => value.expiresAt },
  ]);
}

const GITHUB_SUBSCRIPTION_COLUMNS: readonly TableColumn<GithubRepositorySubscription>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'REPOSITORY', value: (row) => row.repository.identity },
  { header: 'BRANCH', value: (row) => row.branch },
  { header: 'STATUS', value: (row) => row.status },
  { header: 'PLATFORMS', value: (row) => row.recipe.platforms.join(',') },
  { header: 'CREATED AT', value: (row) => row.createdAt },
  { header: 'DEACTIVATED AT', value: (row) => row.deactivatedAt },
];

export function githubSubscriptionsResult(rows: GithubRepositorySubscription[]): CommandResult {
  return listResult(rows, GITHUB_SUBSCRIPTION_COLUMNS);
}

export function githubSubscriptionMutationResult(
  row: GithubRepositorySubscriptionMutationResult
): CommandResult {
  return singleResult(row, [
    ...GITHUB_SUBSCRIPTION_COLUMNS,
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

function singleResult<Row>(row: Row, columns: readonly TableColumn<Row>[]): CommandResult {
  return {
    json: row,
    table: renderTable([row], columns),
  };
}

function listResult<Row>(rows: Row[], columns: readonly TableColumn<Row>[]): CommandResult {
  return {
    json: rows,
    table: renderTable(rows, columns),
  };
}
