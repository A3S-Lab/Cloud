import type { CloudFetch } from '@a3s/cloud-client';
import packageMetadata from '../package.json';
import { parseArguments } from './arguments';
import { executeCommand } from './commands';
import { type ProcessEnvironment, resolveContext } from './context';
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
  memberships list      List organization memberships
  memberships get ID    Get one organization membership
  memberships create KIND NAME ROLE Create a human or service Principal membership idempotently
  memberships change-role ID ROLE Change one membership role with optimistic concurrency
  memberships revoke ID Revoke one membership with optimistic concurrency
  membership-invitations list List organization membership invitation history
  membership-invitations get ID Get one organization membership invitation
  membership-invitations list-mine List invitations bound to the authenticated Principal
  membership-invitations create PRINCIPAL ROLE Invite one exact Principal idempotently
  membership-invitations accept ID Accept an invitation bound to the authenticated Principal
  membership-invitations revoke ID Revoke one invitation with optimistic concurrency
  resource-grants list MEMBERSHIP List Resource Grant history for one restricted membership
  resource-grants get ID Get one Resource Grant
  resource-grants create MEMBERSHIP KIND ID... Create one project, environment, or node grant
  resource-grants revoke ID Revoke one Resource Grant with optimistic concurrency
  projects list        List projects in the selected organization
  projects create NAME Create a project idempotently
  project-attribution get [ID] Get the current or one exact immutable attribution profile
  project-attribution update OWNER Create a new attribution profile with optimistic concurrency
  environments list    List environments in the selected project
  environments create NAME Create an environment idempotently
  ontologies list       List Ontologies in the selected project
  ontologies get ID     Get one Ontology aggregate
  ontologies create     Create an Ontology from A3S ACL
  ontologies revisions ID List immutable Ontology revision summaries
  ontologies revision ID REV Get one immutable Ontology revision and canonical ACL
  ontologies diff ID FROM TO Diff two revisions deterministically
  ontologies revise ID  Publish a version-checked Ontology revision from A3S ACL
  workflow-nodes list    Discover the frozen built-in Workflow node catalog
  workflow-definitions list List Workflow definitions in the selected project
  workflow-definitions get ID Get one WorkflowDefinition aggregate
  workflow-definitions create Publish a Workflow definition and exact ACL payload bundle
  workflow-definitions revisions ID List immutable Workflow revision summaries
  workflow-definitions revision ID REV Get one immutable Workflow revision and ACL payloads
  workflow-definitions revise ID Publish a version-checked Workflow revision
  workflow-goals list    List compiled Workflow goals in the selected project
  workflow-goals get ID  Get one exact WorkflowGoal
  workflow-goals create  Compile a WorkflowGoal from A3S ACL
  workflow-goals plan ID REV Get one immutable deterministic PlanRevision
  human-tasks list [STATUS] List bounded task summaries in the selected project
  human-tasks get ID     Get one protected HumanTask detail
  human-tasks claim ID   Claim one ready HumanTask with optimistic concurrency
  human-tasks release ID Release one HumanTask as its current claimant
  human-tasks submit ID  Submit a native A3S Form interaction from --file
  workflow-runs list     List recent WorkflowRuns in the selected project
  workflow-runs get ID   Get one WorkflowRun and its step projections
  workflow-runs start GOAL PLAN Start one exact PlanRevision idempotently
  workflow-runs wait ID  Wait up to a bounded interval for terminal state
  workflow-runs cancel ID Request WorkflowRun cancellation idempotently
  workflow-runs output ID Read the bounded output of a completed WorkflowRun
  workflow-runs variables ID Inspect typed values from immutable input and A3S Flow history
  workflow-runs history ID Read bounded, redacted A3S Flow history
  execution-templates list List immutable finite-task templates in the selected project
  execution-templates get ID REV Get one exact immutable ExecutionTemplate revision
  execution-templates create Publish an ACL-native ExecutionTemplate revision
  connector-profiles list List Connector profiles in the selected environment
  connector-profiles get ID Get one Connector profile and its current exact revision
  connector-profiles create NAME Create a Connector profile from A3S ACL
  connector-profiles revise ID Revise a Connector profile from A3S ACL
  connector-revisions list PROFILE List immutable revisions for one Connector profile
  connector-revisions get PROFILE REV Get one exact immutable Connector revision
  forms list            List Form drafts in the selected project
  forms get ID          Get one Form draft
  forms create          Create a Form draft from native Form JSON
  forms revise ID       Revise a Form draft with optimistic concurrency
  form-releases list FORM List immutable releases for one Form
  form-releases get FORM ID Get one immutable Form release
  form-releases publish FORM Publish the current Form draft immutably
  assets list           List Assets in the selected organization
  assets get ID         Get one Asset
  assets create NAME KIND Create an Agent, MCP, or Skill Asset idempotently
  assets archive ID     Archive one Asset idempotently
  asset-releases list ASSET List all releases, including draft and yanked releases
  asset-releases get ASSET ID Get one exact release, including a yanked release
  asset-releases select ASSET [VERSION] Select one published release for a new binding
  asset-releases create ASSET VERSION COMMIT Create a hosted release draft idempotently
  asset-releases yank ASSET ID Yank one published release idempotently
  asset-releases mcp-profile ASSET ID Read the immutable MCP Service Profile binding
  asset-releases bind-mcp-profile ASSET ID Bind an MCP Service Profile from A3S ACL
  asset-releases deploy ASSET RELEASE Deploy one published Agent release from A3S ACL
  asset-releases update WORKLOAD ASSET RELEASE Update an Agent workload to one published release
  skill-bindings bind WORKLOAD SKILL RELEASE Bind one exact Skill release to an Agent workload
  skill-bindings unbind WORKLOAD SKILL Unbind one Skill Asset through a new workload revision
  agent-conversations list List Agent conversations in the selected environment
  agent-conversations get ID Get one Agent conversation
  agent-conversations create Create one Agent conversation idempotently
  agent-conversations events ID Read one page of semantic execution events
  agent-executions list CONVERSATION List executions in one Agent conversation
  agent-executions get ID Get one Agent execution
  agent-executions changes ID Get the immutable Git-compatible result of one terminal execution
  agent-executions start CONVERSATION AGENT RELEASE Start one exact Agent release idempotently
  agent-executions cancel ID Cancel one Agent execution through its Code-owned run
  nodes list            List nodes in the selected organization
  nodes bootstrap NAME  Issue one enrollment credential and print a verified install invocation
  nodes ready ID        Mark one current node ready
  nodes drain ID        Drain one current node
  nodes revoke ID       Revoke one current node
  operations list       List recent operations in the selected organization
  audit-records list    List bounded, redacted organization audit history (owner/admin)
  notifications list    List the authenticated Principal's authorized in-app inbox
  notifications get ID  Get one authorized in-app notification
  notifications read ID Mark one notification read with optimistic concurrency
  search resources QUERY Search authorized resources in the selected organization
  plugin-registries list List trusted A3S Use Registry references
  plugin-registries get ID Get one trusted A3S Use Registry reference
  plugin-catalog search ID Search a refreshed signed A3S Use catalog from request JSON
  plugin-catalog search-cached ID Search an already verified catalog cache from request JSON
  plugin-catalog inspect ID Inspect a refreshed signed A3S Use release from request JSON
  plugin-catalog inspect-cached ID Inspect an already verified cached release from request JSON
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
  mcp-credentials list  List hosted MCP credential metadata in the selected environment
  mcp-credentials get ID Get one hosted MCP credential metadata record
  mcp-credentials create Create and print one recoverable hosted MCP bearer credential
  mcp-credentials rotate ID Rotate and print one hosted MCP bearer credential
  mcp-credentials revoke ID Revoke one hosted MCP credential idempotently
  mcp-routes list        List MCP route policies in the selected environment
  mcp-routes get ID      Get one MCP route policy and canonical ACL
  mcp-routes create      Create one MCP route policy from A3S ACL
  mcp-routes revise ID   Revise one MCP route policy from A3S ACL
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
  --cursor <cursor>       Opaque cursor for a log, Agent event, audit, or notification command
  --limit <n>             Search, log, Agent event, audit, or notification page limit
  --unread-only           Filter notifications list to unread records
  --stream <stdout|stderr> Filter a log command by stream
  --idempotency-key <key>  Required stable key for every mutation
  --file <path>             A3S ACL, native Form, Workflow, or catalog JSON input file
  --expected-version <n>    Current aggregate version for a versioned mutation
  --cost-attribution-code <code> Optional showback code for project-attribution update
  --label <key=value>        Repeatable bounded label for project-attribution update
  --migration-rule <id>     Target ACL migration rule for a breaking Ontology revision
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
  --principal <uuid>         Principal bound to a newly created API token
  --expires-at <timestamp>   RFC 3339 credential expiry
  --agent-release-url <url>  HTTPS node-agent release binary for nodes bootstrap
  --agent-release-sha256 <digest> SHA-256 of the node-agent release binary
  --node-config <path>       Absolute A3S ACL node config path on the target host
  --actor-principal <uuid>   Exact audit actor Principal filter
  --action <action>          Exact lowercase dot-separated audit action filter
  --aggregate <uuid>         Exact audit aggregate filter
  --request-id <uuid>        Exact audit request-correlation filter
  --from <timestamp>         Inclusive RFC 3339 audit lower bound
  --to <timestamp>           Inclusive RFC 3339 audit upper bound
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
