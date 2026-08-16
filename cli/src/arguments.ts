import { usageError } from './errors';

export interface ParsedArguments {
  positionals: string[];
  url?: string;
  organizationId?: string;
  projectId?: string;
  environmentId?: string;
  output?: string;
  timeoutMs?: string;
  cursor?: string;
  limit?: string;
  stream?: string;
  idempotencyKey?: string;
  file?: string;
  serviceProfileFile?: string;
  providerWorkloadFile?: string;
  storageBindingFile?: string;
  expectedVersion?: string;
  costAttributionCode?: string;
  projectAttributionLabels: string[];
  migrationRuleId?: string;
  minReady?: string;
  maxUnavailable?: string;
  contextPath?: string;
  dockerfilePath?: string;
  target?: string;
  platforms?: string;
  scopes?: string;
  apiTokenPrincipalId?: string;
  expiresAt?: string;
  agentReleaseUrl?: string;
  agentReleaseSha256?: string;
  nodeConfig?: string;
  workflowRunTimeoutSeconds?: string;
  workflowRunWaitSeconds?: string;
  reason?: string;
  auditActorPrincipalId?: string;
  auditAction?: string;
  auditAggregateId?: string;
  auditRequestId?: string;
  auditFrom?: string;
  auditTo?: string;
  unreadOnly: boolean;
  valueStdin: boolean;
  tokenStdin: boolean;
  enrollmentTokenStdin: boolean;
  help: boolean;
  version: boolean;
}

type ValueOption = Exclude<
  keyof ParsedArguments,
  | 'enrollmentTokenStdin'
  | 'help'
  | 'positionals'
  | 'projectAttributionLabels'
  | 'tokenStdin'
  | 'unreadOnly'
  | 'valueStdin'
  | 'version'
>;

const VALUE_OPTIONS: Readonly<Record<string, ValueOption>> = {
  '--url': 'url',
  '--organization': 'organizationId',
  '--project': 'projectId',
  '--environment': 'environmentId',
  '--output': 'output',
  '--timeout': 'timeoutMs',
  '--cursor': 'cursor',
  '--limit': 'limit',
  '--stream': 'stream',
  '--idempotency-key': 'idempotencyKey',
  '--file': 'file',
  '--service-profile-file': 'serviceProfileFile',
  '--provider-workload-file': 'providerWorkloadFile',
  '--storage-binding-file': 'storageBindingFile',
  '--expected-version': 'expectedVersion',
  '--cost-attribution-code': 'costAttributionCode',
  '--migration-rule': 'migrationRuleId',
  '--min-ready': 'minReady',
  '--max-unavailable': 'maxUnavailable',
  '--context-path': 'contextPath',
  '--dockerfile-path': 'dockerfilePath',
  '--target': 'target',
  '--platforms': 'platforms',
  '--scopes': 'scopes',
  '--principal': 'apiTokenPrincipalId',
  '--expires-at': 'expiresAt',
  '--agent-release-url': 'agentReleaseUrl',
  '--agent-release-sha256': 'agentReleaseSha256',
  '--node-config': 'nodeConfig',
  '--run-timeout-seconds': 'workflowRunTimeoutSeconds',
  '--wait-seconds': 'workflowRunWaitSeconds',
  '--reason': 'reason',
  '--actor-principal': 'auditActorPrincipalId',
  '--action': 'auditAction',
  '--aggregate': 'auditAggregateId',
  '--request-id': 'auditRequestId',
  '--from': 'auditFrom',
  '--to': 'auditTo',
};

export function parseArguments(argv: readonly string[]): ParsedArguments {
  const parsed: ParsedArguments = {
    positionals: [],
    projectAttributionLabels: [],
    valueStdin: false,
    tokenStdin: false,
    enrollmentTokenStdin: false,
    unreadOnly: false,
    help: false,
    version: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--help' || argument === '-h') {
      parsed.help = true;
      continue;
    }
    if (argument === '--version' || argument === '-V') {
      parsed.version = true;
      continue;
    }
    if (argument === '--value-stdin') {
      if (parsed.valueStdin) {
        throw usageError('option --value-stdin may be specified only once');
      }
      parsed.valueStdin = true;
      continue;
    }
    if (argument.startsWith('--value-stdin=')) {
      throw usageError('option --value-stdin does not accept a value');
    }
    if (argument === '--token-stdin') {
      if (parsed.tokenStdin) {
        throw usageError('option --token-stdin may be specified only once');
      }
      parsed.tokenStdin = true;
      continue;
    }
    if (argument === '--enrollment-token-stdin') {
      if (parsed.enrollmentTokenStdin) {
        throw usageError('option --enrollment-token-stdin may be specified only once');
      }
      parsed.enrollmentTokenStdin = true;
      continue;
    }
    if (argument.startsWith('--enrollment-token-stdin=')) {
      throw usageError('option --enrollment-token-stdin does not accept a value');
    }
    if (argument.startsWith('--token-stdin=')) {
      throw usageError('option --token-stdin does not accept a value');
    }
    if (argument === '--token' || argument.startsWith('--token=')) {
      throw usageError('API tokens are accepted only through A3S_CLOUD_TOKEN');
    }
    if (argument === '--unread-only') {
      if (parsed.unreadOnly) {
        throw usageError('option --unread-only may be specified only once');
      }
      parsed.unreadOnly = true;
      continue;
    }
    if (argument.startsWith('--unread-only=')) {
      throw usageError('option --unread-only does not accept a value');
    }
    if (argument === '--label' || argument.startsWith('--label=')) {
      const separator = argument.indexOf('=');
      const inlineValue = separator === -1 ? undefined : argument.slice(separator + 1);
      const value = inlineValue ?? argv[index + 1];
      if (!value || (inlineValue === undefined && value.startsWith('-'))) {
        throw usageError('option --label requires a key=value pair');
      }
      parsed.projectAttributionLabels.push(value);
      if (inlineValue === undefined) {
        index += 1;
      }
      continue;
    }
    if (argument.startsWith('-')) {
      const separator = argument.indexOf('=');
      const name = separator === -1 ? argument : argument.slice(0, separator);
      const key = VALUE_OPTIONS[name];
      if (!key) {
        throw usageError(`unknown option ${JSON.stringify(name)}`);
      }
      if (parsed[key] !== undefined) {
        throw usageError(`option ${name} may be specified only once`);
      }
      const inlineValue = separator === -1 ? undefined : argument.slice(separator + 1);
      const value = inlineValue ?? argv[index + 1];
      if (!value || (inlineValue === undefined && value.startsWith('-'))) {
        throw usageError(`option ${name} requires a value`);
      }
      parsed[key] = value;
      if (inlineValue === undefined) {
        index += 1;
      }
      continue;
    }
    parsed.positionals.push(argument);
  }

  return parsed;
}
