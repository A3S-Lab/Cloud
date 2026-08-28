export type GitProvider = 'github';
export type GitReferenceKind = 'branch' | 'tag' | 'commit';
export type BuildPlatform = 'linux/amd64' | 'linux/arm64';

export const DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE = 50;
export const MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE = 100;
export const MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES = 128;
export const MAXIMUM_GITHUB_REPOSITORY_CANONICAL_URL_BYTES = 159;

export interface GithubSourceDiscoveryPageOptions {
  cursor?: string;
  limit?: number;
}

export interface GitRepositoryInput {
  provider: GitProvider;
  url: string;
}

export interface GitRepository {
  provider: GitProvider;
  canonicalUrl: string;
  identity: string;
}

export interface GitReferenceInput {
  kind: GitReferenceKind;
  value: string;
}

export interface DockerfileBuildRecipe {
  schema: 'a3s.cloud.build-recipe.v1';
  kind: 'dockerfile';
  contextPath: string;
  dockerfilePath: string;
  target: string | null;
  platforms: BuildPlatform[];
}

export interface ResolveSourceRevisionInput {
  repository: GitRepositoryInput;
  reference: GitReferenceInput;
  recipe: DockerfileBuildRecipe;
  webhookDeliveryId?: string;
}

export interface SourceRevision {
  organizationId: string;
  projectId: string;
  environmentId: string;
  id: string;
  repository: GitRepository;
  commitSha: string;
  recipe: DockerfileBuildRecipe;
  recipeDigest: string;
  aggregateVersion: number;
  acceptedAt: string;
}

export interface SourceRevisionMutationResult extends SourceRevision {
  replayed: boolean;
}

export type GithubConnectionStatus =
  | 'active'
  | 'suspended'
  | 'verification_revoked'
  | 'installation_deleted'
  | 'account_changed';

export interface GithubConnection {
  id: string;
  organizationId: string;
  provider: 'github';
  installationId: number;
  account: {
    id: number;
    login: string;
    type: 'organization' | 'user';
  };
  verifiedBy: {
    id: number;
    login: string;
  };
  status: GithubConnectionStatus;
  providerAuthority: {
    checkedAt: string;
    checkAttemptedAt: string;
    nextCheckAt: string;
    consecutiveFailures: number;
    lastError: 'not_configured' | 'unavailable' | 'protocol' | null;
  };
  connectedAt: string;
  updatedAt: string;
}

export interface GithubConnectionInstall {
  provider: 'github';
  installationUrl: string;
  expiresAt: string;
}

export interface GithubDiscoveredRepository {
  repository: GitRepository;
  defaultBranch: string;
  private: boolean;
  fork: boolean;
  archived: boolean;
  disabled: boolean;
}

export interface GithubRepositoryDiscoveryPage {
  repositories: GithubDiscoveredRepository[];
  nextCursor: string | null;
}

export type GithubDiscoveredReferenceKind = 'branch' | 'tag';

export interface GithubDiscoveredBranch {
  kind: 'branch';
  name: string;
  commitSha: string;
  protected: boolean;
}

export interface GithubDiscoveredTag {
  kind: 'tag';
  name: string;
  commitSha: string;
  protected: null;
}

export type GithubDiscoveredReference = GithubDiscoveredBranch | GithubDiscoveredTag;

export interface GithubRepositoryReferenceDiscoveryPage {
  repository: GitRepository;
  kind: GithubDiscoveredReferenceKind;
  references: GithubDiscoveredReference[];
  nextCursor: string | null;
}

export interface CreateGithubRepositorySubscriptionInput {
  repository: GitRepositoryInput;
  branch: string;
  recipe: DockerfileBuildRecipe;
}

export interface GithubRepositorySubscription {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  connectionId: string;
  installationId: number;
  repository: GitRepository;
  branch: string;
  recipe: DockerfileBuildRecipe;
  recipeDigest: string;
  status: 'active' | 'inactive';
  aggregateVersion: number;
  createdAt: string;
  deactivatedAt: string | null;
}

export interface GithubRepositorySubscriptionMutationResult extends GithubRepositorySubscription {
  replayed: boolean;
}

export function encodeGithubSourceDiscoveryPageOptions(
  options: GithubSourceDiscoveryPageOptions = {}
): string {
  const parameters = new URLSearchParams();
  if (options.cursor !== undefined) {
    if (
      options.cursor.length < 1 ||
      options.cursor.length > MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES ||
      !/^[A-Za-z0-9_-]+$/.test(options.cursor)
    ) {
      throw new TypeError('GitHub source discovery cursor is invalid');
    }
    parameters.set('cursor', options.cursor);
  }
  const limit = options.limit ?? DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE) {
    throw new RangeError(
      `GitHub source discovery limit must be between 1 and ${MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE}`
    );
  }
  parameters.set('limit', String(limit));
  return `?${parameters.toString()}`;
}

export function validateGithubSourceDiscoveryReferenceKind(
  kind: string
): asserts kind is GithubDiscoveredReferenceKind {
  if (kind !== 'branch' && kind !== 'tag') {
    throw new TypeError('GitHub source discovery reference kind must be branch or tag');
  }
}

export function validateCanonicalGithubRepositoryUrl(value: string): string {
  if (
    typeof value !== 'string' ||
    value.length > MAXIMUM_GITHUB_REPOSITORY_CANONICAL_URL_BYTES ||
    /[%\0\r\n]/.test(value)
  ) {
    throw new TypeError('GitHub source discovery repository URL must be canonical');
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new TypeError('GitHub source discovery repository URL must be canonical');
  }
  const path = url.pathname.replace(/^\//, '').replace(/\/$/, '');
  const segments = path.split('/');
  const owner = segments[0] ?? '';
  const repository = segments[1] ?? '';
  const canonical = `https://github.com/${owner.toLowerCase()}/${repository.toLowerCase()}`;
  if (
    value !== canonical ||
    url.protocol !== 'https:' ||
    url.hostname !== 'github.com' ||
    url.port !== '' ||
    url.username !== '' ||
    url.password !== '' ||
    url.search !== '' ||
    url.hash !== '' ||
    segments.length !== 2 ||
    !/^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$/.test(owner) ||
    owner.includes('--') ||
    repository.length < 1 ||
    repository.length > 100 ||
    repository === '.' ||
    repository === '..' ||
    repository.endsWith('.git') ||
    !/^[a-z0-9._-]+$/.test(repository)
  ) {
    throw new TypeError('GitHub source discovery repository URL must be canonical');
  }
  return value;
}
