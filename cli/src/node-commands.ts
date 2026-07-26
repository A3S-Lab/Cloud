import { CloudApiError, type CloudApi } from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
  positionalUuid,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization } from './context';
import { usageError } from './errors';
import { nodeBootstrapResult } from './node-results';
import { nodeMutationResult, nodesResult, type CommandResult } from './results';
import { readBoundedUtf8Stdin, type ReadStdin } from './standard-input';
import { parseRfc3339Timestamp } from './timestamp';

const NODE_BOOTSTRAP_COMMAND = 'nodes bootstrap';
const NODE_AGENT_INSTALL_PATH = '/usr/local/bin/a3s-cloud-node-agent';

export interface NodeCommandDependencies {
  readStdin?: ReadStdin;
}

export function rejectMisplacedNodeOptions(command: string, arguments_: ParsedArguments): void {
  if (arguments_.enrollmentTokenStdin && command !== NODE_BOOTSTRAP_COMMAND) {
    throw usageError('--enrollment-token-stdin is valid only for nodes bootstrap');
  }
  if (
    (arguments_.agentReleaseUrl !== undefined ||
      arguments_.agentReleaseSha256 !== undefined ||
      arguments_.nodeConfig !== undefined) &&
    command !== NODE_BOOTSTRAP_COMMAND
  ) {
    throw usageError('agent release and node config options are valid only for nodes bootstrap');
  }
}

export async function executeNodeCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: NodeCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'nodes list':
      requireListCommand(arguments_);
      return nodesResult(await cloudApi().listNodes(requireOrganization(context)));
    case NODE_BOOTSTRAP_COMMAND: {
      const input = requireNodeBootstrapCommand(arguments_);
      const enrollmentCredential = await readEnrollmentCredential(dependencies.readStdin);
      const invocation = installationInvocation(
        input.agentReleaseUrl,
        input.agentReleaseSha256,
        input.nodeConfig
      );
      const result = await safeEnrollmentCredentialMutation(() =>
        cloudApi().issueEnrollmentToken(
          requireOrganization(context),
          {
            name: input.name,
            token: enrollmentCredential,
            expiresAt: input.expiresAt,
          },
          input.idempotencyKey
        )
      );
      return nodeBootstrapResult(result, invocation, enrollmentCredential, input.name);
    }
    case 'nodes ready':
    case 'nodes drain':
    case 'nodes revoke': {
      const mutation = requireNodeMutationCommand(arguments_, `nodes ${positionals[1]} <node-id>`);
      const organizationId = requireOrganization(context);
      const nodeId = positionalUuid(positionals, 2, 'node ID');
      const api = cloudApi();
      const node =
        command === 'nodes ready'
          ? await api.markNodeReady(organizationId, nodeId, mutation.expectedVersion, mutation.idempotencyKey)
          : command === 'nodes drain'
            ? await api.drainNode(organizationId, nodeId, mutation.expectedVersion, mutation.idempotencyKey)
            : await api.revokeNode(organizationId, nodeId, mutation.expectedVersion, mutation.idempotencyKey);
      return nodeMutationResult(node);
    }
    default:
      return undefined;
  }
}

function requireNodeBootstrapCommand(arguments_: ParsedArguments): {
  name: string;
  expiresAt: string;
  agentReleaseUrl: string;
  agentReleaseSha256: string;
  nodeConfig: string;
  idempotencyKey: string;
} {
  requireArity(arguments_.positionals, 3, 'nodes bootstrap <name>');
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.expectedVersion !== undefined) {
    throw usageError('--expected-version is valid only for node lifecycle mutations');
  }
  if (!arguments_.enrollmentTokenStdin) {
    throw usageError('--enrollment-token-stdin is required for nodes bootstrap');
  }
  return {
    name: positionalResourceName(arguments_.positionals, 2),
    expiresAt: parseRfc3339Timestamp(arguments_.expiresAt, 'enrollment credential'),
    agentReleaseUrl: requireAgentReleaseUrl(arguments_.agentReleaseUrl),
    agentReleaseSha256: requireAgentReleaseSha256(arguments_.agentReleaseSha256),
    nodeConfig: requireNodeConfig(arguments_.nodeConfig),
    idempotencyKey: requireIdempotencyKey(arguments_),
  };
}

function requireNodeMutationCommand(
  arguments_: ParsedArguments,
  usage: string
): { expectedVersion: number; idempotencyKey: string } {
  requireArity(arguments_.positionals, 3, usage);
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const rawVersion = arguments_.expectedVersion;
  if (rawVersion === undefined) {
    throw usageError('--expected-version is required for node lifecycle mutations');
  }
  if (!/^[0-9]+$/.test(rawVersion)) {
    throw usageError('expected node version must be a positive safe integer');
  }
  const expectedVersion = Number(rawVersion);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError('expected node version must be a positive safe integer');
  }
  return { expectedVersion, idempotencyKey };
}

function requireAgentReleaseUrl(value: string | undefined): string {
  if (value === undefined) {
    throw usageError('--agent-release-url is required for nodes bootstrap');
  }
  if (value.length > 2_048 || /[\0\r\n]/.test(value)) {
    throw usageError('agent release URL is invalid');
  }
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    throw usageError('agent release URL is invalid');
  }
  if (parsed.protocol !== 'https:') {
    throw usageError('agent release URL must use HTTPS');
  }
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw usageError('agent release URL cannot contain credentials, a query, or a fragment');
  }
  return parsed.toString();
}

function requireAgentReleaseSha256(value: string | undefined): string {
  if (value === undefined) {
    throw usageError('--agent-release-sha256 is required for nodes bootstrap');
  }
  if (!/^[0-9a-f]{64}$/.test(value)) {
    throw usageError('agent release SHA-256 must contain 64 lowercase hex digits');
  }
  return value;
}

function requireNodeConfig(value: string | undefined): string {
  if (
    value === undefined ||
    value.length > 1_024 ||
    !value.startsWith('/') ||
    !value.endsWith('.acl') ||
    /[\0\r\n]/.test(value) ||
    value
      .split('/')
      .slice(1)
      .some((segment) => !segment || segment === '.' || segment === '..')
  ) {
    throw usageError('node config must be an absolute .acl path');
  }
  return value;
}

async function readEnrollmentCredential(readStdin?: ReadStdin): Promise<string> {
  const credential = await readBoundedUtf8Stdin(readStdin, 69, 69, {
    read: 'unable to read node enrollment credential from standard input',
    size: 'node enrollment credential must contain exactly 69 bytes',
    utf8: 'node enrollment credential must be valid UTF-8',
  });
  if (!/^a3sn_[0-9a-f]{64}$/.test(credential)) {
    throw usageError(
      'node enrollment credential must use the a3sn_ prefix followed by 64 lowercase hex digits'
    );
  }
  return credential;
}

function installationInvocation(releaseUrl: string, releaseSha256: string, nodeConfig: string): string {
  return [
    '(',
    '  set -euo pipefail',
    '  staging="$(mktemp)"',
    '  trap \'rm -f "$staging"\' EXIT HUP INT TERM',
    `  curl --fail --location --proto '=https' --tlsv1.2 --output "$staging" ${shellQuote(releaseUrl)}`,
    `  printf '%s  %s\\n' ${shellQuote(releaseSha256)} "$staging" | sha256sum --check --strict -`,
    `  sudo install --mode=0755 "$staging" ${shellQuote(NODE_AGENT_INSTALL_PATH)}`,
    '  rm -f "$staging"',
    '  trap - EXIT HUP INT TERM',
    "  read -r -s -p 'Enrollment credential: ' A3S_CLOUD_ENROLLMENT_TOKEN",
    "  printf '\\n' >&2",
    '  [[ "$A3S_CLOUD_ENROLLMENT_TOKEN" =~ ^a3sn_[0-9a-f]{64}$ ]] || {',
    "    printf 'Invalid enrollment credential.\\n' >&2",
    '    exit 2',
    '  }',
    '  export A3S_CLOUD_ENROLLMENT_TOKEN',
    `  exec ${shellQuote(NODE_AGENT_INSTALL_PATH)} ${shellQuote(nodeConfig)}`,
    ')',
  ].join('\n');
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'"'"'`)}'`;
}

async function safeEnrollmentCredentialMutation<Result>(operation: () => Promise<Result>): Promise<Result> {
  try {
    return await operation();
  } catch (error) {
    if (error instanceof CloudApiError) {
      const requestId =
        error.requestId !== undefined &&
        /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(error.requestId)
          ? error.requestId
          : undefined;
      throw new CloudApiError(
        error.status,
        'node enrollment credential issuance failed',
        error.statusCode,
        requestId
      );
    }
    throw error;
  }
}
