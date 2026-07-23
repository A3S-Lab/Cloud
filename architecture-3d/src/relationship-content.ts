import type { ArchitectureEdgeDetail } from './architecture-schema';

export const ARCHITECTURE_EDGE_DETAILS = {
  'clients-web': {
    summary:
      'Operators use the management surface to turn human intent into explicit Cloud commands, queries, and observation requests.',
    transfers: ['Operator intent', 'Authenticated browser context', 'Navigation and filter state'],
    boundary:
      'The browser surface initiates work but does not decide placement, mutate durable state directly, or hold provider credentials.',
  },
  'web-gateway': {
    summary:
      'The Web Console sends same-origin management requests to the public Gateway, keeping browser delivery and API access behind one origin.',
    transfers: ['HTTPS /api request', 'Tenant and identity context', 'Idempotency metadata'],
    boundary:
      'The browser never addresses the private Boot API directly; Gateway remains the public transport boundary.',
  },
  'code-gateway': {
    summary:
      'A3S Code TUI submits Cloud commands and queries through the same public Gateway contract used by other management clients.',
    transfers: ['CLI/TUI command', 'Authentication context', 'Typed request payload'],
    boundary:
      'A3S Code is a client workload inside A3S Box, not a privileged path around Cloud authorization or tenancy.',
  },
  'gateway-api': {
    summary:
      'Gateway recognizes /api routes and forwards them to the private A3S Boot control-plane service over the trusted upstream boundary.',
    transfers: ['Validated HTTP request', 'Forwarded identity context', 'Streaming API response'],
    boundary:
      'Gateway performs transport routing only; Boot and its bounded contexts retain all business authorization and state authority.',
  },
  'github-sources': {
    summary:
      'The Sources context receives a signed provider event and resolves it to one immutable repository revision for reproducible processing.',
    transfers: ['Signed webhook event', 'Repository identity', 'Exact commit revision'],
    boundary: 'Unverified webhook payloads and floating branch names cannot become trusted build inputs.',
  },
  'api-identity': {
    summary:
      'A3S Boot delegates authentication and tenant-scoped authorization checks to the Identity bounded context before dispatching protected work.',
    transfers: ['Principal claims', 'Tenant scope', 'Authorization decision'],
    boundary:
      'Identity decides access, but it does not execute workload, source, or provider business operations.',
  },
  'api-projects': {
    summary:
      'The API resolves the target project and tenant boundary so downstream commands operate inside one explicit ownership scope.',
    transfers: ['Project identifier', 'Tenant membership', 'Scoped project projection'],
    boundary:
      'Project scope must be established before a command can create or change project-owned resources.',
  },
  'api-workloads': {
    summary:
      'A validated management command becomes a desired workload revision for asynchronous reconciliation.',
    transfers: ['Desired specification', 'Artifact or image digest', 'Idempotency key'],
    boundary:
      'The request records intent; it does not synchronously start containers or bypass the operation workflow.',
  },
  'api-sources': {
    summary:
      'A validated source command asks the Sources context to resolve and prepare an immutable source revision.',
    transfers: ['Repository reference', 'Requested revision', 'Build policy inputs'],
    boundary: 'The API does not clone repositories or execute builds inside the request handler.',
  },
  'api-inference': {
    summary:
      'An inference-oriented request is translated into typed accelerator, model, and serving intent before it enters normal Cloud reconciliation.',
    transfers: ['Model identity', 'Accelerator requirements', 'Serving policy'],
    boundary:
      'Inference intent does not select a physical GPU directly and does not place Cloud on the live request path.',
  },
  'inference-power': {
    summary:
      'Cloud Inference references a versioned A3S Power backend profile when compiling compatible model-serving intent.',
    transfers: [
      'Power backend revision',
      'Model and protocol compatibility',
      'Required runtime capabilities',
    ],
    boundary:
      'The catalog relationship does not let Power own Cloud models, placement, accelerator allocation, Workloads, or routes.',
  },
  'sources-artifacts': {
    summary:
      'Sources hands Artifacts a content-pinned build input so every build can be traced to one exact revision.',
    transfers: ['Immutable source archive', 'Commit metadata', 'Build context digest'],
    boundary:
      'Artifacts accepts immutable input only; mutable working trees and provider callbacks remain outside the build contract.',
  },
  'artifacts-operations': {
    summary:
      'Artifacts creates a durable BuildRun operation so a potentially long build can be observed, retried, and audited.',
    transfers: ['BuildRun command', 'Input digest', 'Builder requirements'],
    boundary: 'Creating the operation does not imply that a provider accepted or completed the build.',
  },
  'workloads-operations': {
    summary:
      'Workloads records deployment intent as a durable operation that can reconcile desired and observed state over time.',
    transfers: ['Deployment command', 'Desired revision', 'Convergence policy'],
    boundary: 'HTTP request lifetime is separated from provider execution and rollout completion.',
  },
  'operations-flow': {
    summary:
      'Operations starts or advances an A3S Flow workflow that preserves steps, retries, leases, and terminal outcomes.',
    transfers: ['Operation state', 'Workflow input', 'Retry and timeout policy'],
    boundary:
      'Flow coordinates execution history but does not become the authoritative owner of domain desired state.',
  },
  'flow-node-agent': {
    summary:
      'A3S Flow exposes a leased command that an outbound-connected Node Agent can claim and acknowledge safely.',
    transfers: ['Leased command', 'Expected revision', 'Lease and retry metadata'],
    boundary:
      'Cloud does not require an inbound control port on the managed node; expired or duplicate leases cannot create parallel authority.',
  },
  'node-runtime': {
    summary:
      'Node Agent asks the provider-neutral A3S Runtime to apply or remove one declared unit on the local host.',
    transfers: ['Apply/remove request', 'Desired unit specification', 'Operation correlation'],
    boundary:
      'Node Agent coordinates local work while Runtime owns provider invocation and normalized lifecycle behavior.',
  },
  'runtime-provider': {
    summary:
      'A3S Runtime invokes the Docker or BuildKit implementation through a typed provider contract and normalizes its result.',
    transfers: ['Provider task', 'Isolation and resource limits', 'Normalized status and output'],
    boundary: 'Provider-specific flags and credentials do not leak into Cloud domain contracts.',
  },
  'runtime-box': {
    summary:
      'A3S Runtime selects the A3S Box provider when a Cloud workload must run as an isolated OCI-backed Box unit.',
    transfers: ['Box workload specification', 'OCI image digest', 'Runtime limits and mounts'],
    boundary:
      'The Box provider is one conformant implementation; Workloads does not depend on its private lifecycle API.',
  },
  'box-workload': {
    summary:
      'The A3S Box provider creates, supervises, and removes the concrete isolated workload unit requested by Runtime.',
    transfers: ['Pinned OCI rootfs', 'Process and sandbox configuration', 'Health and exit state'],
    boundary:
      'The provider hosts arbitrary Cloud workloads; A3S Code is only one possible product workload, not the provider itself.',
  },
  'runtime-cpu': {
    summary:
      'Runtime resolves a provider plan onto advertised CPU capacity for general builds, services, and agent workloads.',
    transfers: ['CPU and memory request', 'Execution constraints', 'Runtime accounting'],
    boundary:
      'Runtime consumes advertised capability and does not invent capacity or bypass Fleet placement decisions.',
  },
  'runtime-gpu': {
    summary:
      'Runtime applies a typed accelerator execution plan to a compatible GPU resource exposed by a conformant provider.',
    transfers: ['GPU capability claim', 'Device and memory constraints', 'Accelerated execution status'],
    boundary:
      'This is the provider-neutral target contract; current Box GPU passthrough remains planned until conformance evidence exists.',
  },
  'power-cpu': {
    summary:
      'A3S Power can execute privacy-preserving inference on CPU-backed TEE capacity while producing verifiable request evidence.',
    transfers: [
      'TEE execution request',
      'Encrypted model and request state',
      'Attestation receipt and inference output',
    ],
    boundary:
      'CPU hardware supplies trusted execution capability; Power does not allocate the host or bypass Cloud workload lifecycle.',
  },
  'power-gpu': {
    summary:
      'A3S Power uses a compatible confidential GPU execution binding for accelerated inference and evidence generation.',
    transfers: [
      'Pinned GPU execution policy',
      'Confidential-computing evidence',
      'Streaming inference output',
    ],
    boundary:
      'GPU use remains gated by Cloud claims, Runtime enforcement, and Power backend conformance; Power never selects devices directly.',
  },
  'buildkit-cpu': {
    summary:
      'BuildKit executes an isolated source build on CPU compute with declared limits and reproducible inputs.',
    transfers: ['Build graph', 'Pinned source context', 'Build logs and result digest'],
    boundary:
      'Build execution cannot mutate Cloud desired state and receives only command-scoped credentials.',
  },
  'buildkit-registry': {
    summary:
      'A successful build publishes a validated, content-addressed OCI graph to the configured registry.',
    transfers: ['OCI layers', 'Manifest and config', 'Immutable image digest'],
    boundary:
      'Only a verified digest crosses into release state; mutable tags are never the authoritative handoff.',
  },
  'registry-workloads': {
    summary:
      'The published OCI digest is handed to Workloads so a release can reference immutable executable content.',
    transfers: ['Image digest', 'Registry location', 'Verification metadata'],
    boundary:
      'Registry availability does not itself deploy a workload; Workloads must record new desired state explicitly.',
  },
  'runtime-workload': {
    summary:
      'Runtime reports the provider-neutral unit that has converged for the requested workload revision.',
    transfers: ['Unit identity', 'Observed lifecycle state', 'Provider-normalized health'],
    boundary: 'A created unit is not routable until health, revision, and policy checks complete.',
  },
  'cpu-workload': {
    summary: 'CPU racks supply execution and continuous health evidence to CPU-backed Cloud workload units.',
    transfers: ['CPU execution', 'Resource telemetry', 'Health and process output'],
    boundary:
      'Hardware supplies capability; the workload unit and control plane retain lifecycle and routing semantics.',
  },
  'gpu-workload': {
    summary:
      'GPU racks supply accelerator execution and telemetry to GPU-backed inference or compute workload units.',
    transfers: ['Accelerator execution', 'Device telemetry', 'Health and inference output'],
    boundary: 'Physical GPU identity is abstracted behind typed capability and provider contracts.',
  },
  'workload-edge': {
    summary:
      'A healthy, revision-exact workload target becomes eligible input for Edge route policy generation.',
    transfers: ['Exact target identity', 'Health evidence', 'Revision and endpoint metadata'],
    boundary:
      'A target is never published from desired state alone; stale or unhealthy units remain excluded.',
  },
  'edge-agent': {
    summary:
      'Edge emits a complete versioned route snapshot for outbound distribution to the responsible Node Agent.',
    transfers: ['Complete route snapshot', 'Snapshot version', 'Policy and target set'],
    boundary: 'Snapshots are complete and atomic; partial route patches cannot become mixed runtime policy.',
  },
  'agent-gateway': {
    summary:
      'Node Agent applies the complete route snapshot to Gateway and acknowledges the exact committed version.',
    transfers: ['Versioned route snapshot', 'Atomic apply command', 'Apply acknowledgement'],
    boundary:
      'Gateway enforces only a fully accepted snapshot and never becomes rollout or scheduling authority.',
  },
  'clients-gateway': {
    summary: 'External clients establish the live HTTPS or streaming connection directly with A3S Gateway.',
    transfers: ['HTTPS request', 'Streaming payload', 'Client protocol metadata'],
    boundary: 'A3S Cloud control services remain off the live data path and are not queried per request.',
  },
  'gateway-workload': {
    summary:
      'Gateway applies its current atomic policy and streams the request to one exact healthy workload target.',
    transfers: ['Validated request stream', 'Selected upstream target', 'Streaming response'],
    boundary:
      'Gateway can select only targets in the committed snapshot; it cannot schedule, roll out, or repair workloads.',
  },
  'workload-agent-logs': {
    summary:
      'The workload provider forwards ordered stdout and stderr records to Node Agent with resumable cursor information.',
    transfers: ['Ordered log frames', 'Stream identity', 'Cursor and gap markers'],
    boundary:
      'Logs are observations, not commands; missing ranges must be explicit rather than silently reordered.',
  },
  'agent-fleet': {
    summary:
      'Node Agent reports resource and workload observations to Fleet using monotonic cursors for deduplication and recovery.',
    transfers: ['Node observation', 'Workload health', 'Monotonic cursor'],
    boundary:
      'Observed state cannot overwrite desired state and stale cursors cannot move the projection backward.',
  },
  'fleet-object-store': {
    summary: 'Fleet persists verified log chunks and their cursor metadata in immutable object storage.',
    transfers: ['Verified log chunk', 'Content digest', 'Cursor range and gap metadata'],
    boundary:
      'Object bytes are immutable; database projections retain indexing and authorization responsibility.',
  },
  'object-store-api': {
    summary: 'The private API reads an authorized, bounded page of log objects for the management client.',
    transfers: ['Bounded log page', 'Next cursor', 'Explicit gap metadata'],
    boundary: 'Clients receive only tenant-authorized pages and never direct object-store credentials.',
  },
  'contexts-postgres': {
    summary:
      'The Workloads context commits desired state and its concurrency metadata to authoritative PostgreSQL storage.',
    transfers: ['Desired workload revision', 'Tenant ownership', 'Optimistic concurrency version'],
    boundary: 'PostgreSQL stores domain truth but does not perform reconciliation or provider execution.',
  },
  'artifacts-store': {
    summary:
      'Artifacts writes command-bound source archives, build records, and immutable output bytes to content storage.',
    transfers: ['Source or artifact bytes', 'Content digest', 'Operation correlation'],
    boundary:
      'Every stored object must be attributable to an authorized command and immutable content identity.',
  },
  'contexts-event': {
    summary:
      'A committed project-domain change is published as a fact for projections and downstream reactions.',
    transfers: ['Committed domain fact', 'Aggregate identity', 'Sequence and correlation metadata'],
    boundary:
      'Events describe already committed facts; consumers cannot use the event stream to bypass aggregate rules.',
  },
  'inference-workloads': {
    summary:
      'Inference converts model-serving intent into a normal typed workload plan that Workloads can reconcile.',
    transfers: ['Model workload plan', 'Artifact and runtime requirements', 'Scaling and health policy'],
    boundary: 'Inference augments the workload specification but does not own generic deployment lifecycle.',
  },
  'inference-fleet': {
    summary:
      'Inference asks Fleet for compatible accelerator capability without binding product intent to one hardware vendor.',
    transfers: ['Typed accelerator claim', 'Memory and topology requirements', 'Placement constraints'],
    boundary:
      'Claims express requirements; Fleet retains capability matching, leasing, and observation authority.',
  },
  'fleet-gpu': {
    summary:
      'Fleet leases matching GPU capability and continuously receives device and workload observations from the resource pool.',
    transfers: ['Capability lease', 'GPU inventory', 'Health and utilization observation'],
    boundary:
      'A lease is bounded and revocable; it does not grant direct infrastructure credentials to inference clients.',
  },
  'inference-edge': {
    summary:
      'Inference contributes model-aware endpoint and protocol policy to the normal Edge snapshot generation path.',
    transfers: ['Model route intent', 'Protocol and endpoint metadata', 'Revision compatibility rules'],
    boundary: 'Inference never writes Gateway state directly; Edge remains the sole route-policy authority.',
  },
} as const satisfies Readonly<Record<string, ArchitectureEdgeDetail>>;

export function architectureEdgeDetail(id: string): ArchitectureEdgeDetail {
  const detail = (ARCHITECTURE_EDGE_DETAILS as Readonly<Record<string, ArchitectureEdgeDetail>>)[id];
  if (!detail) throw new Error(`Architecture edge ${id} is missing explanatory content`);
  return detail;
}
