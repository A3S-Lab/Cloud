import type { BuildPlatform, CloudApi, DockerfileBuildRecipe, GitReferenceKind } from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireListCommand,
  requireMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireEnvironment, requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import type { CommandResult } from './results';
import {
  githubConnectionInstallResult,
  githubConnectionResult,
  githubSubscriptionMutationResult,
  githubSubscriptionsResult,
  sourceRevisionMutationResult,
  sourceRevisionsResult,
} from './source-results';

const RECIPE_COMMANDS = new Set(['source-revisions resolve', 'source-subscriptions create']);

export function rejectMisplacedSourceRecipeOptions(command: string, arguments_: ParsedArguments): void {
  if (
    !RECIPE_COMMANDS.has(command) &&
    (arguments_.contextPath !== undefined ||
      arguments_.dockerfilePath !== undefined ||
      arguments_.target !== undefined ||
      arguments_.platforms !== undefined)
  ) {
    throw usageError(
      '--context-path, --dockerfile-path, --target, and --platforms are valid only for Source recipe mutations'
    );
  }
}

export async function executeSourceCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'source-revisions list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return sourceRevisionsResult(
        await cloudApi().listSourceRevisions(scope.organizationId, scope.projectId, scope.environmentId)
      );
    }
    case 'source-revisions resolve': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        5,
        'source-revisions resolve <repository-url> <branch|tag|commit> <reference>'
      );
      const scope = requireEnvironmentScope(context);
      return sourceRevisionMutationResult(
        await cloudApi().resolveSourceRevision(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          {
            repository: {
              provider: 'github',
              url: githubRepositoryUrl(positionals[2]),
            },
            reference: gitReference(positionals[3], positionals[4]),
            recipe: sourceRecipe(arguments_),
          },
          idempotencyKey
        )
      );
    }
    case 'source-connections get':
      requireSimpleCommand(arguments_, 'source-connections get');
      return githubConnectionResult(await cloudApi().getGithubConnection(requireOrganization(context)));
    case 'source-connections begin':
      requireSimpleCommand(arguments_, 'source-connections begin');
      return githubConnectionInstallResult(
        await cloudApi().beginGithubConnection(requireOrganization(context))
      );
    case 'source-subscriptions list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return githubSubscriptionsResult(
        await cloudApi().listGithubRepositorySubscriptions(
          scope.organizationId,
          scope.projectId,
          scope.environmentId
        )
      );
    }
    case 'source-subscriptions create': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'source-subscriptions create <repository-url> <branch>'
      );
      const scope = requireEnvironmentScope(context);
      return githubSubscriptionMutationResult(
        await cloudApi().createGithubRepositorySubscription(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          {
            repository: {
              provider: 'github',
              url: githubRepositoryUrl(positionals[2]),
            },
            branch: namedGitReference(positionals[3], 'Git branch'),
            recipe: sourceRecipe(arguments_),
          },
          idempotencyKey
        )
      );
    }
    case 'source-subscriptions deactivate': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        3,
        'source-subscriptions deactivate <subscription-id>'
      );
      const scope = requireEnvironmentScope(context);
      return githubSubscriptionMutationResult(
        await cloudApi().deactivateGithubRepositorySubscription(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          positionalUuid(positionals, 2, 'source subscription ID'),
          idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function requireSimpleCommand(arguments_: ParsedArguments, usage: string): void {
  requireArity(arguments_.positionals, 2, usage);
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
}

function requireEnvironmentScope(context: CloudContext): {
  organizationId: string;
  projectId: string;
  environmentId: string;
} {
  return {
    organizationId: requireOrganization(context),
    projectId: requireProject(context),
    environmentId: requireEnvironment(context),
  };
}

function githubRepositoryUrl(value: string | undefined): string {
  if (!value || value.length > 256 || value.includes('%') || /[\0\r\n]/.test(value)) {
    throw usageError('repository URL must identify one HTTPS github.com owner/repository');
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw usageError('repository URL must identify one HTTPS github.com owner/repository');
  }
  const path = url.pathname.replace(/^\//, '').replace(/\/$/, '');
  const segments = path.split('/');
  const repository = segments[1]?.replace(/\.git$/i, '');
  if (
    url.protocol !== 'https:' ||
    url.hostname !== 'github.com' ||
    url.port !== '' ||
    url.username !== '' ||
    url.password !== '' ||
    url.search !== '' ||
    url.hash !== '' ||
    segments.length !== 2 ||
    !/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$/.test(segments[0] ?? '') ||
    segments[0]?.includes('--') ||
    !repository ||
    repository.length > 100 ||
    !/^[A-Za-z0-9._-]+$/.test(repository) ||
    repository === '.' ||
    repository === '..'
  ) {
    throw usageError('repository URL must identify one HTTPS github.com owner/repository');
  }
  return value;
}

function gitReference(
  kind: string | undefined,
  value: string | undefined
): { kind: GitReferenceKind; value: string } {
  if (kind === 'commit') {
    if (!value || !/^[0-9a-f]{40}$/i.test(value)) {
      throw usageError('Git commit reference must be one full 40-character SHA');
    }
    return { kind, value: value.toLowerCase() };
  }
  if (kind !== 'branch' && kind !== 'tag') {
    throw usageError('Git reference kind must be branch, tag, or commit');
  }
  return { kind, value: namedGitReference(value, `Git ${kind}`) };
}

function namedGitReference(value: string | undefined, label: string): string {
  if (
    !value ||
    value.length > 255 ||
    value === '@' ||
    value.startsWith('refs/') ||
    value.startsWith('/') ||
    value.endsWith('/') ||
    value.endsWith('.') ||
    value.includes('..') ||
    value.includes('//') ||
    !/^[A-Za-z0-9._/-]+$/.test(value) ||
    value.split('/').some((segment) => {
      return !segment || segment.startsWith('.') || segment.endsWith('.') || segment.endsWith('.lock');
    })
  ) {
    throw usageError(`${label} must be a bounded safe name without a refs/ prefix`);
  }
  return value;
}

function sourceRecipe(arguments_: ParsedArguments): DockerfileBuildRecipe {
  const contextPath = repositoryPath(arguments_.contextPath, true, '--context-path');
  const dockerfilePath = repositoryPath(arguments_.dockerfilePath, false, '--dockerfile-path');
  const platforms = buildPlatforms(arguments_.platforms);
  const target = arguments_.target;
  if (target !== undefined && (target.length > 128 || !/^[A-Za-z0-9._-]+$/.test(target))) {
    throw usageError('--target must be a bounded Dockerfile stage name');
  }
  return {
    schema: 'a3s.cloud.build-recipe.v1',
    kind: 'dockerfile',
    contextPath,
    dockerfilePath,
    target: target ?? null,
    platforms,
  };
}

function repositoryPath(value: string | undefined, allowRoot: boolean, option: string): string {
  if (!value) {
    throw usageError(`${option} is required for Source recipe mutations`);
  }
  const normalized = value.startsWith('./') ? value.slice(2) : value;
  if (
    normalized.length > 255 ||
    normalized.startsWith('/') ||
    /[\0\\%]/.test(normalized) ||
    (normalized === '.' && !allowRoot) ||
    (normalized !== '.' &&
      normalized.split('/').some((segment) => {
        return !segment || segment === '.' || segment === '..' || !/^[A-Za-z0-9._@+-]+$/.test(segment);
      }))
  ) {
    throw usageError(`${option} must be a bounded relative POSIX path`);
  }
  return normalized;
}

function buildPlatforms(value: string | undefined): BuildPlatform[] {
  if (!value) {
    throw usageError('--platforms is required for Source recipe mutations');
  }
  const platforms = value.split(',');
  if (
    platforms.length < 1 ||
    platforms.length > 8 ||
    new Set(platforms).size !== platforms.length ||
    platforms.some((platform) => platform !== 'linux/amd64' && platform !== 'linux/arm64')
  ) {
    throw usageError('--platforms must contain one or two unique supported Linux platforms');
  }
  return platforms as BuildPlatform[];
}
