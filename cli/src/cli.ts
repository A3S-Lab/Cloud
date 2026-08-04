import type { CloudFetch } from '@a3s/cloud-client';
import packageMetadata from '../package.json';
import { parseArguments } from './arguments';
import { executeCommand } from './commands';
import { resolveContext, type ProcessEnvironment } from './context';
import { ExitCode, normalizeError } from './errors';
import { renderError, renderJson } from './output';

export const CLI_VERSION = packageMetadata.version;

export const HELP = `A3S Cloud CLI ${CLI_VERSION}

Usage:
  a3s-cloud [global options] <command> <action>

Commands:
  context show          Show resolved non-secret context
  diagnostics status    Show public platform and health diagnostics
  organizations list   List authorized organizations
  organizations create NAME Create an organization idempotently
  api-tokens list       List API token metadata in the selected organization
  api-tokens get ID     Get one API token metadata record
  api-tokens create NAME Create an API token from standard input idempotently
  api-tokens revoke ID  Revoke one API token idempotently
  projects list        List projects in the selected organization
  projects create NAME Create a project idempotently
  environments list    List environments in the selected project
  environments create NAME Create an environment idempotently
  assets list           List Assets in the selected organization
  assets get ID         Get one Asset
  assets create NAME KIND Create an Agent, MCP, or Skill Asset idempotently
  assets archive ID     Archive one Asset idempotently
  asset-releases list ASSET List all releases, including draft and yanked releases
  asset-releases get ASSET ID Get one exact release, including a yanked release
  asset-releases select ASSET [VERSION] Select one published release for a new binding
  asset-releases create ASSET VERSION COMMIT Create a hosted release draft idempotently
  asset-releases yank ASSET ID Yank one published release idempotently
  nodes list            List nodes in the selected organization
  nodes bootstrap NAME  Issue one enrollment credential and print a verified install invocation
  nodes ready ID        Mark one current node ready
  nodes drain ID        Drain one current node
  nodes revoke ID       Revoke one current node
  operations list       List recent operations in the selected organization
  search resources QUERY Search authorized resources in the selected organization
  workloads list        List workloads in the selected environment
  workloads get ID      Get one workload
  workloads logs ID REV Read one page of workload revision logs
  workloads create      Create one workload from A3S ACL
  workloads update ID   Update one workload from A3S ACL
  workloads stop ID     Stop one workload idempotently
  workloads rollback ID REV Roll back by cloning a proven revision
  source-revisions list   List pinned source revisions in the selected environment
  source-revisions resolve URL KIND REF Resolve one GitHub reference idempotently
  source-revisions deploy ID Deploy one built source revision from A3S ACL
  source-connections get  Show the organization GitHub connection
  source-connections begin Start the no-store GitHub installation flow
  source-subscriptions list List GitHub repository subscriptions
  source-subscriptions create URL BRANCH Create a GitHub subscription idempotently
  source-subscriptions deactivate ID Deactivate a GitHub subscription idempotently
  secrets list          List Secret metadata in the selected environment
  secrets get ID        Get Secret metadata and version states
  secrets create NAME   Create a Secret from standard input idempotently
  secrets add-version ID Add a Secret version from standard input idempotently
  secrets revoke-version ID VERSION Revoke one Secret version idempotently
  deployments get ID    Get one deployment
  deployments cancel ID Request deployment cancellation idempotently
  domain-claims list     List domain ownership claims in the selected environment
  domain-claims get ID   Get one domain ownership claim
  domain-claims create PATTERN Create a domain ownership claim idempotently
  domain-claims verify ID PROOF Verify one domain ownership claim idempotently
  domain-claims revoke ID REASON Revoke one domain ownership claim idempotently
  gateway-scopes list    List logical Gateway scopes in the selected environment
  gateway-scopes create NODE... Create a replicated Gateway scope idempotently
  routes list           List routes in the selected environment
  routes get ID         Get one route
  routes publish SCOPE REV CLAIM HOST PATH PORT Publish one managed route idempotently
  build-runs list       List recent BuildRuns in the selected environment
  build-runs get ID     Get one BuildRun
  build-runs evidence ID Get verified BuildRun evidence
  build-runs logs ID    Report Box BuildRun log availability
  build-runs cancel ID  Request BuildRun cancellation idempotently
  build-runs retry ID   Retry one terminal BuildRun idempotently

Global options:
  --url <url>             Cloud API URL ending in /api/v1
  --organization <uuid>   Organization context
  --project <uuid>        Project context
  --environment <uuid>    Environment context
  --output <table|json>   Output format (default: table)
  --timeout <ms>          Request timeout from 1 to 300000
  --cursor <cursor>       Opaque cursor for a log command
  --limit <n>             Search limit (1..50) or log record limit (1..256)
  --stream <stdout|stderr> Filter a log command by stream
  --idempotency-key <key>  Required stable key for every mutation
  --file <path>             A3S ACL file for a desired-state mutation
  --expected-version <n>    Current aggregate version for a node mutation
  --min-ready <n>           Required ready members for gateway-scopes create
  --max-unavailable <n>     Allowed unavailable members for gateway-scopes create
  --context-path <path>      Repository context for a Source build recipe
  --dockerfile-path <path>   Dockerfile path for a Source build recipe
  --target <stage>           Optional Dockerfile target stage
  --platforms <csv>          linux/amd64 and/or linux/arm64
  --value-stdin              Read Secret material exactly from standard input
  --token-stdin              Read a new API token credential from standard input
  --enrollment-token-stdin   Read a node enrollment credential from standard input
  --scopes <csv>             API token scopes for api-tokens create
  --expires-at <timestamp>   RFC 3339 credential expiry
  --agent-release-url <url>  HTTPS node-agent release binary for nodes bootstrap
  --agent-release-sha256 <digest> SHA-256 of the node-agent release binary
  --node-config <path>       Absolute A3S ACL node config path on the target host
  -h, --help              Show help
  -V, --version           Show version

Environment:
  A3S_CLOUD_TOKEN           API token; never accepted as a command-line option
  A3S_CLOUD_URL             Cloud API URL
  A3S_CLOUD_ORGANIZATION_ID Organization context
  A3S_CLOUD_PROJECT_ID      Project context
  A3S_CLOUD_ENVIRONMENT_ID  Environment context
  A3S_CLOUD_OUTPUT          table or json
  A3S_CLOUD_TIMEOUT_MS      Request timeout in milliseconds
`;

export interface CliRuntime {
  environment?: ProcessEnvironment;
  fetch?: CloudFetch;
  readFile?: (path: string) => Promise<Uint8Array>;
  readStdin?: (limitBytes: number) => Promise<Uint8Array>;
  writeStdout?: (value: string) => void;
  writeStderr?: (value: string) => void;
}

export async function runCli(argv: readonly string[], runtime: CliRuntime = {}): Promise<number> {
  const environment = runtime.environment ?? process.env;
  const writeStdout = runtime.writeStdout ?? ((value: string) => process.stdout.write(value));
  const writeStderr = runtime.writeStderr ?? ((value: string) => process.stderr.write(value));
  try {
    const arguments_ = parseArguments(argv);
    if (arguments_.version) {
      writeStdout(`${CLI_VERSION}\n`);
      return ExitCode.Success;
    }
    if (arguments_.help || arguments_.positionals.length === 0) {
      writeStdout(HELP);
      return ExitCode.Success;
    }
    const context = resolveContext(arguments_, environment);
    const result = await executeCommand(arguments_, context, {
      fetch: runtime.fetch,
      readFile: runtime.readFile,
      readStdin: runtime.readStdin,
    });
    writeStdout(
      redactToken(context.output === 'json' ? renderJson(result.json) : result.table, context.token)
    );
    return result.exitCode ?? ExitCode.Success;
  } catch (error) {
    const normalized = normalizeError(error);
    writeStderr(
      redactToken(renderError(normalized, requestsJson(argv, environment)), environment.A3S_CLOUD_TOKEN)
    );
    return normalized.exitCode;
  }
}

function redactToken(value: string, token: string | undefined): string {
  return token ? value.split(token).join('[redacted]') : value;
}

function requestsJson(argv: readonly string[], environment: ProcessEnvironment): boolean {
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument.startsWith('--output=')) {
      return argument.slice('--output='.length) === 'json';
    }
    if (argument === '--output') {
      return argv[index + 1] === 'json';
    }
  }
  return environment.A3S_CLOUD_OUTPUT === 'json';
}
