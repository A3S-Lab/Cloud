import type { JourneyId } from './architecture';

export const SIMULATION_ENTRY_IDS = ['web', 'code'] as const;
export type SimulationEntryId = (typeof SIMULATION_ENTRY_IDS)[number];

export const SIMULATION_SCENARIO_IDS = [
  'deploy-cpu',
  'source-release',
  'gpu-inference',
  'live-traffic',
  'observe-recover',
] as const;
export type SimulationScenarioId = (typeof SIMULATION_SCENARIO_IDS)[number];

export interface SimulationEntry {
  id: SimulationEntryId;
  label: string;
  shortLabel: string;
  description: string;
  nodeIds: readonly string[];
  edgeIds: readonly string[];
}

export interface SimulationFrame {
  id: string;
  title: string;
  actor: string;
  description: string;
  nodeIds: readonly string[];
  edgeIds: readonly string[];
  durationMs: number;
}

export interface SimulationScenario {
  id: SimulationScenarioId;
  label: string;
  shortLabel: string;
  description: string;
  journey: JourneyId;
  color: string;
  steps: readonly SimulationFrame[];
}

export const SIMULATION_ENTRIES: readonly SimulationEntry[] = [
  {
    id: 'web',
    label: 'A3S Web Console',
    shortLabel: 'A3S Web',
    description: 'An operator uses the same-origin management SPA.',
    nodeIds: ['clients', 'web', 'api'],
    edgeIds: ['clients-web', 'web-api'],
  },
  {
    id: 'code',
    label: 'A3S Box → A3S Code TUI',
    shortLabel: 'Code TUI',
    description:
      'A local A3S Box carries A3S Code as one of its possible workloads; Code then issues typed Cloud commands.',
    nodeIds: ['a3s-box', 'code-tui', 'api'],
    edgeIds: ['code-api'],
  },
] as const;

export const SIMULATION_SCENARIOS: readonly SimulationScenario[] = [
  {
    id: 'deploy-cpu',
    label: 'Deploy a Box-isolated CPU service',
    shortLabel: 'CPU deploy',
    description:
      'Commit desired state, select the A3S Box Runtime provider, converge an isolated CPU unit, then publish a safe route.',
    journey: 'deploy',
    color: '#b8f36b',
    steps: [
      {
        id: 'authorize-project',
        title: 'Authorize and scope the command',
        actor: 'Boot API · Identity · Projects',
        description:
          'Boot establishes tenant context, Identity authorizes the caller, and Projects resolves the exact environment.',
        nodeIds: ['api', 'identity', 'projects'],
        edgeIds: ['api-identity', 'api-projects'],
        durationMs: 2700,
      },
      {
        id: 'commit-workload',
        title: 'Commit desired workload state',
        actor: 'Workloads · PostgreSQL · Operations',
        description:
          'Workloads accepts one immutable revision, commits desired state and idempotency, then exposes a durable operation.',
        nodeIds: ['api', 'workloads', 'postgres', 'operations'],
        edgeIds: ['api-workloads', 'contexts-postgres', 'workloads-operations'],
        durationMs: 3000,
      },
      {
        id: 'coordinate-deploy',
        title: 'Coordinate replay-safe convergence',
        actor: 'Operations · A3S Flow · Node Agent',
        description:
          'The operation starts a durable Flow run and leases one exact command to the outbound-connected Node Agent.',
        nodeIds: ['operations', 'flow', 'node-agent'],
        edgeIds: ['operations-flow', 'flow-node-agent'],
        durationMs: 2900,
      },
      {
        id: 'execute-cpu',
        title: 'Launch the workload inside A3S Box',
        actor: 'Node Agent · Runtime · A3S Box · CPU Compute',
        description:
          'For stronger isolation, Runtime selects its conformant Box driver; A3S Box creates the MicroVM or sandbox that carries the Cloud workload on CPU hardware.',
        nodeIds: ['node-agent', 'runtime', 'box-provider', 'cpu-compute', 'workload-unit'],
        edgeIds: [
          'node-runtime',
          'runtime-box',
          'box-workload',
          'runtime-cpu',
          'cpu-workload',
          'runtime-workload',
        ],
        durationMs: 3200,
      },
      {
        id: 'publish-route',
        title: 'Publish and acknowledge the route',
        actor: 'Edge · Node Agent · Gateway',
        description:
          'Only the exact healthy target enters a complete Gateway snapshot; the route becomes active after atomic acknowledgement.',
        nodeIds: ['workload-unit', 'edge', 'node-agent', 'gateway'],
        edgeIds: ['workload-edge', 'edge-agent', 'agent-gateway'],
        durationMs: 3100,
      },
    ],
  },
  {
    id: 'source-release',
    label: 'Build a source release',
    shortLabel: 'Git → OCI',
    description:
      'Resolve an immutable Git revision, run an isolated BuildKit task, and publish a verified OCI graph.',
    journey: 'source',
    color: '#f3c86b',
    steps: [
      {
        id: 'authorize-source',
        title: 'Authorize source intent',
        actor: 'Boot API · Identity · Sources',
        description:
          'The selected management surface submits source intent under one tenant and environment boundary.',
        nodeIds: ['api', 'identity', 'projects', 'sources'],
        edgeIds: ['api-identity', 'api-projects', 'api-sources'],
        durationMs: 2700,
      },
      {
        id: 'resolve-revision',
        title: 'Resolve an exact Git revision',
        actor: 'GitHub · Sources',
        description:
          'A signed GitHub event and short-lived installation authority resolve an immutable commit without storing credentials.',
        nodeIds: ['github', 'sources'],
        edgeIds: ['github-sources'],
        durationMs: 2800,
      },
      {
        id: 'open-build-run',
        title: 'Open a durable BuildRun',
        actor: 'Artifacts · Operations · Flow',
        description:
          'Artifacts validates immutable inputs, records a BuildRun, and starts a replay-safe build workflow.',
        nodeIds: ['sources', 'artifacts', 'operations', 'flow'],
        edgeIds: ['sources-artifacts', 'artifacts-operations', 'operations-flow'],
        durationMs: 3000,
      },
      {
        id: 'run-buildkit',
        title: 'Run the isolated BuildKit task',
        actor: 'Node Agent · Runtime · Docker/BuildKit · CPU',
        description:
          'The leased task crosses the provider-neutral Runtime boundary and executes on CPU hardware under dual network denial.',
        nodeIds: ['flow', 'node-agent', 'runtime', 'docker-buildkit', 'cpu-compute'],
        edgeIds: ['flow-node-agent', 'node-runtime', 'runtime-provider', 'buildkit-cpu'],
        durationMs: 3400,
      },
      {
        id: 'publish-oci',
        title: 'Verify and publish OCI content',
        actor: 'Artifacts · Object Store · OCI Registry',
        description:
          'Cloud captures command-bound output, verifies every descriptor, then publishes the complete digest-addressed OCI graph.',
        nodeIds: ['artifacts', 'docker-buildkit', 'object-storage', 'registry'],
        edgeIds: ['artifacts-store', 'buildkit-registry'],
        durationMs: 3200,
      },
      {
        id: 'handoff-digest',
        title: 'Hand off the immutable release',
        actor: 'OCI Registry · Workloads',
        description:
          'Only the verified digest crosses into Workloads; mutable tags and provider credentials do not.',
        nodeIds: ['registry', 'workloads'],
        edgeIds: ['registry-workloads'],
        durationMs: 2500,
      },
    ],
  },
  {
    id: 'gpu-inference',
    label: 'Deploy GPU inference',
    shortLabel: 'GPU inference',
    description:
      'Turn typed inference intent into leased accelerator capacity, a healthy model target, and a live route.',
    journey: 'all',
    color: '#d7b6ff',
    steps: [
      {
        id: 'submit-inference',
        title: 'Submit typed inference intent',
        actor: 'Boot API · Inference Profile',
        description:
          'The management surface selects a model profile; Boot preserves one application path and hands off a typed plan.',
        nodeIds: ['api', 'identity', 'inference'],
        edgeIds: ['api-identity', 'api-inference'],
        durationMs: 2800,
      },
      {
        id: 'plan-inference',
        title: 'Plan workload and accelerator claims',
        actor: 'Inference · Workloads · Fleet',
        description:
          'Inference reuses Workloads for revision intent and Fleet for advertised accelerator capabilities and leases.',
        nodeIds: ['inference', 'workloads', 'fleet', 'operations'],
        edgeIds: ['inference-workloads', 'inference-fleet', 'workloads-operations'],
        durationMs: 3100,
      },
      {
        id: 'lease-gpu',
        title: 'Lease an exact GPU capability',
        actor: 'Flow · Fleet · GPU Compute',
        description:
          'Durable coordination selects an eligible node and binds the command to observed GPU devices and accelerator memory.',
        nodeIds: ['operations', 'flow', 'fleet', 'node-agent', 'gpu-compute'],
        edgeIds: ['operations-flow', 'flow-node-agent', 'fleet-gpu'],
        durationMs: 3300,
      },
      {
        id: 'execute-gpu',
        title: 'Start the model through Runtime',
        actor: 'Node Agent · A3S Runtime · GPU Compute',
        description:
          'Node Agent applies the leased command; Runtime turns the provider-neutral plan into GPU-backed execution.',
        nodeIds: ['node-agent', 'runtime', 'gpu-compute', 'workload-unit'],
        edgeIds: ['node-runtime', 'runtime-gpu', 'gpu-workload'],
        durationMs: 3200,
      },
      {
        id: 'route-model',
        title: 'Publish the healthy model route',
        actor: 'Inference · Edge · Gateway',
        description:
          'Health evidence and model route policy converge into one complete snapshot, applied atomically by Gateway.',
        nodeIds: ['inference', 'workload-unit', 'edge', 'node-agent', 'gateway'],
        edgeIds: ['inference-edge', 'workload-edge', 'edge-agent', 'agent-gateway'],
        durationMs: 3200,
      },
      {
        id: 'stream-inference',
        title: 'Stream an inference response',
        actor: 'Client · Gateway · GPU Runtime Unit',
        description:
          'The client streams through Gateway to the exact healthy target; the Cloud control plane stays off the request path.',
        nodeIds: ['clients', 'gateway', 'workload-unit', 'gpu-compute'],
        edgeIds: ['clients-gateway', 'gateway-workload', 'gpu-workload'],
        durationMs: 3000,
      },
    ],
  },
  {
    id: 'live-traffic',
    label: 'Serve live traffic',
    shortLabel: 'Live request',
    description:
      'Follow an HTTPS or streaming request through Gateway to an exact healthy CPU or GPU target.',
    journey: 'traffic',
    color: '#72b7ff',
    steps: [
      {
        id: 'enter-gateway',
        title: 'Terminate the client connection',
        actor: 'Client · A3S Gateway',
        description:
          'Gateway terminates managed TLS, validates request-path policy, and keeps Cloud off the live data plane.',
        nodeIds: ['clients', 'gateway'],
        edgeIds: ['clients-gateway'],
        durationMs: 2600,
      },
      {
        id: 'select-target',
        title: 'Select an acknowledged healthy target',
        actor: 'Gateway · Runtime Unit',
        description:
          'The atomic route snapshot contains only the exact healthy target acknowledged by the node.',
        nodeIds: ['gateway', 'workload-unit', 'edge'],
        edgeIds: ['gateway-workload', 'workload-edge'],
        durationMs: 2800,
      },
      {
        id: 'execute-request',
        title: 'Execute on available compute',
        actor: 'CPU/GPU Compute · Runtime Unit',
        description:
          'The provider resource consumes the scheduled CPU or GPU hardware while preserving one Runtime lifecycle contract.',
        nodeIds: ['workload-unit', 'cpu-compute', 'gpu-compute'],
        edgeIds: ['cpu-workload', 'gpu-workload'],
        durationMs: 3000,
      },
      {
        id: 'stream-response',
        title: 'Stream the response to the client',
        actor: 'Runtime Unit · Gateway · Client',
        description:
          'Gateway streams the result without consulting tenant state, schedulers, or the Cloud API on the request path.',
        nodeIds: ['workload-unit', 'gateway', 'clients'],
        edgeIds: ['gateway-workload', 'clients-gateway'],
        durationMs: 2700,
      },
    ],
  },
  {
    id: 'observe-recover',
    label: 'Inspect logs and recover',
    shortLabel: 'Logs + repair',
    description:
      'Move ordered output into durable objects, query bounded pages, and preserve resumable operation state.',
    journey: 'observe',
    color: '#ef9cff',
    steps: [
      {
        id: 'capture-output',
        title: 'Capture ordered provider output',
        actor: 'CPU/GPU · Runtime Unit · Node Agent',
        description:
          'Runtime output and health evidence are journaled with exact cursors before the node reports observations.',
        nodeIds: ['cpu-compute', 'gpu-compute', 'workload-unit', 'node-agent'],
        edgeIds: ['cpu-workload', 'gpu-workload', 'workload-agent-logs'],
        durationMs: 3000,
      },
      {
        id: 'ingest-observation',
        title: 'Ingest observations and cursor gaps',
        actor: 'Node Agent · Fleet',
        description:
          'Fleet accepts ordered batches, records explicit gaps, and never grants nodes direct access to durable Cloud state.',
        nodeIds: ['node-agent', 'fleet'],
        edgeIds: ['agent-fleet'],
        durationMs: 2800,
      },
      {
        id: 'persist-chunks',
        title: 'Persist verified log chunks',
        actor: 'Fleet · Artifact & Log Store',
        description:
          'Verified immutable chunks move to object storage while PostgreSQL retains bounded descriptors and cursors.',
        nodeIds: ['fleet', 'object-storage', 'postgres'],
        edgeIds: ['fleet-object-store', 'contexts-postgres'],
        durationMs: 3000,
      },
      {
        id: 'query-logs',
        title: 'Query a bounded log page',
        actor: 'Object Store · Boot API',
        description:
          'Boot returns one authorized page and resumable cursor rather than loading unbounded log bodies into business rows.',
        nodeIds: ['object-storage', 'api', 'identity'],
        edgeIds: ['object-store-api', 'api-identity'],
        durationMs: 2900,
      },
      {
        id: 'render-operation',
        title: 'Render progress and recovery state',
        actor: 'A3S Web/Code · Operations',
        description:
          'The selected surface displays durable operation history, cancellation, replay, and repair without owning business rules.',
        nodeIds: ['web', 'code-tui', 'api', 'operations', 'flow'],
        edgeIds: ['web-api', 'code-api', 'operations-flow'],
        durationMs: 3100,
      },
    ],
  },
] as const;

export function simulationFramesFor(
  entryId: SimulationEntryId,
  scenarioId: SimulationScenarioId
): readonly SimulationFrame[] {
  const entry = SIMULATION_ENTRIES.find((candidate) => candidate.id === entryId);
  const scenario = SIMULATION_SCENARIOS.find((candidate) => candidate.id === scenarioId);
  if (!entry || !scenario) return [];
  return [
    {
      id: `${entry.id}-entry`,
      title: `Start from ${entry.label}`,
      actor: entry.label,
      description: entry.description,
      nodeIds: entry.nodeIds,
      edgeIds: entry.edgeIds,
      durationMs: 2400,
    },
    ...scenario.steps,
  ];
}
