export type GitProvider = 'github';
export type GitReferenceKind = 'branch' | 'tag' | 'commit';
export type BuildPlatform = 'linux/amd64' | 'linux/arm64';

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
