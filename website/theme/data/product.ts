export type CapabilityVisual =
  'intent' | 'box' | 'delivery' | 'gateway' | 'recovery' | 'surfaces';

export type Capability = {
  gate: string;
  eyebrow: string;
  title: string;
  body: string;
  visual: CapabilityVisual;
  facts: string[];
};

export type FutureTrack = {
  gate: string;
  label: string;
  title: string;
  body: string;
  signal: string;
};

export const convergenceSteps = [
  {
    code: '01',
    name: 'Commit intent',
    system: 'A3S Boot API',
    detail:
      'Validate tenant-scoped A3S ACL and commit desired state with an operation identity.',
    evidence: 'intent.accepted',
  },
  {
    code: '02',
    name: 'Persist truth',
    system: 'A3S ORM · PostgreSQL',
    detail:
      'Write business state, idempotency, and outbox facts through one typed persistence boundary.',
    evidence: 'transaction.committed',
  },
  {
    code: '03',
    name: 'Resume work',
    system: 'A3S Flow',
    detail:
      'Continue durable operations across retries, worker restarts, leases, and cleanup.',
    evidence: 'operation.resumed',
  },
  {
    code: '04',
    name: 'Lease command',
    system: 'Fleet · Node Agent',
    detail:
      'An outbound agent leases one idempotent, generation-bound command over mTLS.',
    evidence: 'command.leased',
  },
  {
    code: '05',
    name: 'Apply exact unit',
    system: 'A3S Runtime · A3S Box',
    detail:
      'Apply the immutable Task or Service through the sole Box execution contract.',
    evidence: 'runtime.applied',
  },
  {
    code: '06',
    name: 'Observe convergence',
    system: 'Gateway · Evidence',
    detail:
      'Health, logs, exact Gateway snapshots, and receipts prove the observed state.',
    evidence: 'desired.observed',
  },
] as const;

export const capabilities: Capability[] = [
  {
    gate: 'F0',
    eyebrow: 'DURABLE CONTROL',
    title: 'Intent survives the request',
    body: 'Cloud commits desired state before execution. A3S Flow resumes leases, retries, projections, and cleanup after interruption.',
    visual: 'intent',
    facts: ['A3S Boot API', 'A3S ORM', 'PostgreSQL', 'A3S Flow'],
  },
  {
    gate: 'BX0',
    eyebrow: 'SOLE EXECUTION PATH',
    title: 'Everything converges through Box',
    body: 'Task, Service, build, network, mount, Secret, log, output, and cleanup behavior share one Runtime contract and one Box provider.',
    visual: 'box',
    facts: ['A3S Runtime', 'A3S Box', 'generation fencing'],
  },
  {
    gate: 'G0',
    eyebrow: 'SOURCE DELIVERY',
    title: 'Commit to signed evidence',
    body: 'Resolve immutable source, build in isolation, validate complete OCI graphs, publish by digest, and retain signed SPDX and SLSA evidence.',
    visual: 'delivery',
    facts: ['Git revision', 'OCI digest', 'SPDX', 'SLSA'],
  },
  {
    gate: 'H0',
    eyebrow: 'MANAGED REACHABILITY',
    title: 'Gateway policy converges atomically',
    body: 'Cloud owns domains, TLS, target generations, rollout thresholds, and complete snapshots. Gateway stays on the request path.',
    visual: 'gateway',
    facts: ['managed TLS', 'exact snapshot', 'rollback'],
  },
  {
    gate: 'BX0',
    eyebrow: 'RECOVERY EVIDENCE',
    title: 'Restart without inventing state',
    body: 'Receipt-gated batches, ordered logs, immutable chunks, claims, and native observations make replay explicit and inspectable.',
    visual: 'recovery',
    facts: ['receipts', 'ordered logs', 'claims', 'replay'],
  },
  {
    gate: 'C0',
    eyebrow: 'ONE APPLICATION MODEL',
    title: 'Web, CLI, REST, and MCP agree',
    body: 'Every control surface reuses the same commands, queries, authorization, idempotency, typed client, and API envelope.',
    visual: 'surfaces',
    facts: ['REST', 'CLI', 'Web', 'management MCP'],
  },
];

export const futureTracks: FutureTrack[] = [
  {
    gate: 'PW0',
    label: 'POWER BOUNDARY',
    title: 'Confidential inference units',
    body: 'Compile immutable A3S Power Service profiles into Box-hosted MicroVM or TEE evidence without moving placement authority out of Cloud.',
    signal: 'profile → power service → evidence',
  },
  {
    gate: 'P0',
    label: 'DEVELOPER WORKFLOWS',
    title: 'From repository to preview',
    body: 'Add build detection, workload profiles, previews, monorepo selection, and a bounded import path over the existing delivery loop.',
    signal: 'detect → build → preview → promote',
  },
  {
    gate: 'A0',
    label: 'RELEASE CATALOG',
    title: 'Agents, MCPs, and Skills as releases',
    body: 'Publish immutable, tenant-authorized assets through the same source, artifact, workload, and Gateway paths.',
    signal: 'source → release → bind → operate',
  },
  {
    gate: 'A1',
    label: 'AGENT EXECUTION',
    title: 'Durable work with human checkpoints',
    body: 'Conversations, executions, approvals, checkpoints, forks, and trajectories build on existing operations and outbound node control.',
    signal: 'execute → approve → checkpoint → recover',
  },
  {
    gate: 'S0',
    label: 'STATEFUL PLATFORM',
    title: 'State with explicit fencing',
    body: 'Databases and volumes add ownership, backup, restore, retention, and failure fencing without creating another scheduler.',
    signal: 'claim → attach → protect → restore',
  },
  {
    gate: 'I0',
    label: 'INFERENCE PROFILE',
    title: 'Governed model serving',
    body: 'Add accelerator claims, OpenAI-compatible traffic, scoped keys, routing, exact shared limits, usage, and provider governance.',
    signal: 'model → route → stream → account',
  },
];
