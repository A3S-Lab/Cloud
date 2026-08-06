import type { Language } from '../../lib/i18n';

export interface LocalizedCopy {
  readonly 'zh-CN': string;
  readonly en: string;
}

export type CapabilityState = 'verified' | 'in-progress' | 'recertification' | 'planned';

export interface CapabilityGate {
  readonly code: string;
  readonly title: LocalizedCopy;
  readonly outcome: LocalizedCopy;
  readonly features: readonly LocalizedCopy[];
  readonly state: CapabilityState;
  readonly unavailable?: boolean;
}

export interface CapabilityGroup {
  readonly id: string;
  readonly title: LocalizedCopy;
  readonly description: LocalizedCopy;
  readonly gates: readonly CapabilityGate[];
}

export interface DocumentationVersion {
  readonly id: 'main' | '0.1';
  readonly label: string;
  readonly title: LocalizedCopy;
  readonly description: LocalizedCopy;
  readonly source: LocalizedCopy;
}

export interface ProductPillar {
  readonly id: 'unified-gateway' | 'workflow' | 'agent-factory';
  readonly title: LocalizedCopy;
  readonly promise: LocalizedCopy;
  readonly description: LocalizedCopy;
  readonly basedOn: string;
  readonly state: CapabilityState;
  readonly capabilities: readonly LocalizedCopy[];
  readonly gateCodes: readonly string[];
}

export const ROADMAP_SNAPSHOT = '2026-08-06';

export const CAPABILITY_STATES: Record<
  CapabilityState,
  { readonly label: LocalizedCopy; readonly description: LocalizedCopy }
> = {
  verified: {
    label: copy('已验证', 'Verified'),
    description: copy(
      '真实提供方、故障、恢复、清理与发布证据均已通过。',
      'Real-provider, failure, recovery, cleanup, and release evidence passes.'
    ),
  },
  'in-progress': {
    label: copy('实现中', 'In progress'),
    description: copy(
      '已有可用实现切片，但仍缺少路线图列明的退出证据。',
      'A usable implementation slice exists, with named exit evidence still open.'
    ),
  },
  recertification: {
    label: copy('待 Box 复认证', 'Box re-certification'),
    description: copy(
      '保留历史实现证据，Box-only 基线完成复认证前不视为当前发布证据。',
      'Historical evidence remains, but the Box-only baseline is not current release evidence yet.'
    ),
  },
  planned: {
    label: copy('计划中', 'Planned'),
    description: copy(
      '能力在所属 Gate 通过之前不可用。',
      'The capability remains unavailable until its owning gate passes.'
    ),
  },
};

export const PRODUCT_PILLARS: readonly ProductPillar[] = [
  {
    id: 'unified-gateway',
    title: copy('A3S Gateway 统一网关', 'A3S Gateway unified gateway'),
    promise: copy(
      '让所有智能流量通过一个可信入口',
      'Move every intelligent workload through one trusted gateway'
    ),
    description: copy(
      'Cloud API 管理 Workflow、Agent、MCP、模型 API 与业务服务的身份和期望策略，A3S Gateway 承载协议、路由与实时流量。Sentry、AnySentry、审计和安全响应仍是该产品的关键治理能力。',
      'Cloud API owns identity and desired policy for Workflows, Agents, MCP, model APIs, and business services, while A3S Gateway owns protocol, routing, and live traffic. Sentry, AnySentry, audit, and security response remain key governance capabilities of this product.'
    ),
    basedOn: 'Cloud API + A3S Gateway',
    state: 'in-progress',
    capabilities: [
      copy('统一接入与协议', 'Unified ingress and protocols'),
      copy('身份、ACL、配额与路由', 'Identity, ACL, quota, and routing'),
      copy('端云流量与发布策略', 'Cloud-edge traffic and release policy'),
      copy('Sentry / AnySentry 安全证据', 'Sentry / AnySentry security evidence'),
    ],
    gateCodes: ['E0', 'C0', 'H0', 'MCP0', 'I0'],
  },
  {
    id: 'workflow',
    title: copy('Workflow 自主工作流编排', 'Workflow autonomous orchestration'),
    promise: copy(
      '把业务本体变成可执行、可恢复的长期流程',
      'Turn business ontology into executable, recoverable long-running workflows'
    ),
    description: copy(
      '以业务对象、关系、规则、目标和约束为统一语义，让系统从意图生成计划，协调 Agent、工具与人工节点，并在中断后从持久状态继续。',
      'Use business objects, relationships, rules, goals, and constraints as shared semantics so the system can plan from intent, coordinate Agents, tools, and people, and resume from durable state.'
    ),
    basedOn: 'A3S Workflow',
    state: 'planned',
    capabilities: [
      copy('本体驱动的业务语义', 'Ontology-driven business semantics'),
      copy('自主计划与多步骤编排', 'Autonomous planning and multi-step orchestration'),
      copy('持久状态、重试与补偿', 'Durable state, retry, and compensation'),
      copy('人工检查点与完整证据', 'Human checkpoints and complete evidence'),
    ],
    gateCodes: ['W0', 'C0', 'A1', 'MCP0', 'I0'],
  },
  {
    id: 'agent-factory',
    title: copy('Agent Factory 异构智能体工厂', 'Heterogeneous Agent Factory'),
    promise: copy(
      '把一次性 Agent 原型变成可版本化的数字资产',
      'Turn one-off Agent prototypes into versioned digital assets'
    ),
    description: copy(
      '通过唯一的 Provider 中立执行合约适配不同 Agent 框架、模型、语言、Harness、Skill、MCP 与安全策略，并复用同一资产、调度、Runtime 和证据链路。A3S Code 是原生 Provider，而不是唯一可托管 Harness。',
      'Adapt different Agent frameworks, models, languages, harnesses, Skills, MCP servers, and security policies through one provider-neutral execution contract while reusing the same asset, scheduling, Runtime, and evidence paths. A3S Code is the native provider, not the only harness that can be hosted.'
    ),
    basedOn: 'A3S Runtime + A3S Box',
    state: 'in-progress',
    capabilities: [
      copy('Agent / Skill / MCP 资产发布', 'Agent / Skill / MCP asset publishing'),
      copy('版本固定与安全装配', 'Version pinning and safe assembly'),
      copy('单一 Provider 合约与异构 Harness', 'One provider contract for heterogeneous harnesses'),
      copy('会话、语义事件与轨迹', 'Conversations, semantic events, and trajectories'),
    ],
    gateCodes: ['A0', 'A1', 'MCP0', 'I0', 'EV0'],
  },
];

export const CAPABILITY_GROUPS: readonly CapabilityGroup[] = [
  {
    id: 'control',
    title: copy('控制平面与生产运营', 'Control plane and production operations'),
    description: copy(
      '租户权威、管理接口、持久化操作与生产级规模能力。',
      'Tenant authority, management surfaces, durable operations, and production-scale controls.'
    ),
    gates: [
      gate(
        'F0',
        '基础控制平面',
        'Foundation',
        'A3S Boot、PostgreSQL、租户、身份、Flow 操作、Outbox、投影、API 与 Web Shell。',
        'A3S Boot, PostgreSQL, tenancy, identity, Flow operations, outbox, projections, API, and web shell.',
        'verified',
        [
          ['组织 / 项目 / 环境', 'Organizations / projects / environments'],
          ['范围化身份与授权', 'Scoped identity and grants'],
          ['持久化操作与事件投影', 'Durable operations and event projections'],
        ]
      ),
      gate(
        'C0',
        '控制与协作界面',
        'Control surfaces',
        'REST、CLI、管理 MCP、授权、搜索、协作、通知、审计与受限终端能力。',
        'REST, CLI, management MCP, grants, search, collaboration, notifications, audit, and bounded terminal access.',
        'in-progress',
        [
          ['OpenAPI 与 TypeScript Client', 'OpenAPI and TypeScript client'],
          ['Web / CLI / 管理 MCP', 'Web / CLI / management MCP'],
          ['搜索、审计与协作', 'Search, audit, and collaboration'],
        ]
      ),
      gate(
        'H0',
        '生产规模与高可用',
        'Production scale',
        '持久副本、多节点调度、私网、Gateway 复制、控制平面高可用与可度量自动扩缩。',
        'Durable replicas, multi-node placement, private networking, Gateway replication, control-plane HA, and measured autoscaling.',
        'in-progress',
        [
          ['副本与多节点调度', 'Replicas and multi-node placement'],
          ['资源 Claim 与故障隔离', 'Resource claims and fencing'],
          ['高可用与自动扩缩', 'High availability and autoscaling'],
        ]
      ),
      gate(
        'W0',
        '本体驱动的 Workflow',
        'Ontology-driven Workflow',
        '版本化业务本体、确定性计划、类型化能力步骤与基于 Operations 和 A3S Flow 的可恢复长期运行。',
        'Versioned business ontologies, deterministic plans, typed capability steps, and recoverable long-running work on Operations and A3S Flow.',
        'planned',
        [
          ['本体与 Workflow 修订', 'Ontology and Workflow revisions'],
          ['目标到确定性计划', 'Goals to deterministic plans'],
          ['类型化步骤与长期恢复', 'Typed steps and long-running recovery'],
        ]
      ),
    ],
  },
  {
    id: 'delivery',
    title: copy('执行、交付与网络', 'Execution, delivery, and networking'),
    description: copy(
      '从节点接入、OCI 工作负载、源码构建到 TLS 与 Gateway 可达性的统一路径。',
      'One path from node enrollment and OCI workloads to source builds, TLS, and Gateway reachability.'
    ),
    gates: [
      gate(
        'BX0',
        'Box-only 平台',
        'Box-only platform',
        'A3S Box 是唯一节点执行与构建提供方，重新认证 Runtime、部署、交付、恢复与清理基线。',
        'A3S Box is the sole node execution and build provider, re-certifying Runtime, deployment, delivery, recovery, and cleanup.',
        'in-progress',
        [
          ['Task / Service 生命周期', 'Task / Service lifecycle'],
          ['MicroVM / Sandbox 隔离', 'MicroVM / Sandbox isolation'],
          ['构建、缓存、镜像与清理', 'Builds, cache, images, and cleanup'],
        ]
      ),
      gate(
        'R0',
        '通用 Runtime',
        'Universal Runtime',
        '通用 Task 与 Service 契约、持久身份、能力匹配与真实提供方一致性。',
        'General Task and Service contracts, durable identity, capability matching, and real-provider conformance.',
        'recertification',
        [
          ['通用 Task 与 Service', 'General Tasks and Services'],
          ['持久 Runtime 身份', 'Durable Runtime identity'],
          ['能力匹配与一致性', 'Capability matching and conformance'],
        ]
      ),
      gate(
        'N0',
        '仅出站节点控制',
        'Outbound node control',
        '节点注册、mTLS、命令租约、观测、持久命令 Journal 与唯一 Box Driver。',
        'Node enrollment, mTLS, command leases, observations, durable command journal, and the sole Box driver.',
        'recertification',
        [
          ['节点注册与身份轮换', 'Enrollment and identity rotation'],
          ['Lease / Command / Receipt', 'Leases / commands / receipts'],
          ['版本化节点库存', 'Versioned node inventory'],
        ]
      ),
      gate(
        'D0',
        'OCI 工作负载部署',
        'OCI workload deployment',
        'Digest 固定修订、调度、应用、健康激活、停止、取消与恢复。',
        'Digest-pinned revisions, scheduling, apply, health activation, stop, cancellation, and recovery.',
        'recertification',
        [
          ['不可变 Workload Revision', 'Immutable workload revisions'],
          ['健康证据后激活', 'Activation after health evidence'],
          ['更新、回滚与取消', 'Update, rollback, and cancellation'],
        ]
      ),
      gate(
        'E0',
        '可达服务与安全变更',
        'Reachable services and safe changes',
        '托管 TLS、完整 Gateway Snapshot、加密 Secret、持久日志、不可变更新与克隆回滚。',
        'Managed TLS, complete Gateway snapshots, encrypted Secrets, durable logs, immutable updates, and cloned rollback.',
        'recertification',
        [
          ['DomainClaim 与 TLS', 'Domain claims and TLS'],
          ['Gateway Scope 与完整快照', 'Gateway scopes and complete snapshots'],
          ['Secret、日志、更新与回滚', 'Secrets, logs, updates, and rollback'],
        ]
      ),
      gate(
        'G0',
        '外部源码交付',
        'External source delivery',
        '固定 Git 提交、Box 原生构建、OCI 校验发布、SPDX/SLSA 证据与 Workload 交接。',
        'Pinned Git commits, Box-native builds, OCI admission and publication, SPDX/SLSA evidence, and Workload handoff.',
        'in-progress',
        [
          ['GitHub Source Revision', 'GitHub source revisions'],
          ['cloud.build@5', 'cloud.build@5'],
          ['OCI、SBOM 与 Provenance', 'OCI, SBOM, and provenance'],
        ]
      ),
      gate(
        'P0',
        '开发者工作流',
        'Developer workflows',
        '构建检测、Web / Worker / Scheduled Profile、预览环境、Monorepo 与封闭 Compose 导入。',
        'Build detection, web / worker / scheduled profiles, previews, monorepos, and closed Compose import.',
        'planned',
        [
          ['自动构建检测', 'Automatic build detection'],
          ['预览环境与 Monorepo', 'Preview environments and monorepos'],
          ['封闭 Compose 导入', 'Closed Compose import'],
        ]
      ),
    ],
  },
  {
    id: 'agents',
    title: copy('Agent、MCP 与插件生态', 'Agents, MCP, and plugin ecosystem'),
    description: copy(
      '资产发布、A3S Code 执行、托管 MCP 与 A3S Use 分配共享同一 A3S OS 控制路径。',
      'Asset releases, A3S Code execution, hosted MCP, and A3S Use assignments share one A3S OS control path.'
    ),
    gates: [
      gate(
        'A0',
        'Agent 与能力资产目录',
        'Agent and capability release catalog',
        '不可变 Agent、MCP、Skill 资产与发布，Agent 部署及 Skill 绑定复用通用交付路径。',
        'Immutable Agent, MCP, and Skill assets and releases, with Agent deployment and Skill binding on the common delivery path.',
        'in-progress',
        [
          ['草稿 / 发布 / 撤回', 'Draft / publish / yank'],
          ['Agent Workload 绑定', 'Agent Workload binding'],
          ['Skill 与 MCP 绑定', 'Skill and MCP binding'],
        ]
      ),
      gate(
        'A1',
        '持久化 Agent 执行',
        'Durable Agent execution',
        '会话、执行、语义事件、审批、检查点、分叉与轨迹沿既有 Cloud 路径运行；A3S Code 是原生 Provider，A1.3 冻结唯一的 Provider 中立合约。',
        'Conversations, executions, semantic events, approvals, checkpoints, forks, and trajectories use existing Cloud paths. A3S Code is the native provider, while A1.3 freezes the single provider-neutral contract.',
        'in-progress',
        [
          ['A1.0 事件流已验证', 'A1.0 event stream verified'],
          ['A1.1 会话基础已实现', 'A1.1 conversation foundation implemented'],
          ['A1.2 Code 协议与节点传输实现中', 'A1.2 Code protocol and node transport in progress'],
          ['A1.3 异构 Harness 合约计划中', 'A1.3 heterogeneous Harness contract planned'],
        ]
      ),
      gate(
        'MCP0',
        '托管 MCP 服务',
        'Hosted MCP services',
        '现代 MCP 发布准入、Runtime Service 托管、Cloud 编排、Gateway 协议执行与联合恢复。',
        'Modern MCP release admission, Runtime Service hosting, Cloud orchestration, Gateway enforcement, and joint recovery.',
        'in-progress',
        [
          ['MCP 2026-07-28 准入', 'MCP 2026-07-28 admission'],
          ['Runtime Service 托管', 'Runtime Service hosting'],
          ['Gateway 协议执行', 'Gateway protocol enforcement'],
        ],
        true
      ),
      gate(
        'U0',
        'A3S Use 插件分配',
        'A3S Use plugin assignments',
        '可信 Registry、精确 Workspace Package 分配、审阅 Plan / Apply、启用、观测与恢复。',
        'Trusted registry, exact workspace package assignments, reviewed plan / apply, enablement, observations, and recovery.',
        'in-progress',
        [
          ['可信 Registry Enrollment', 'Trusted registry enrollment'],
          ['Package Plan / Apply', 'Package plan / apply'],
          ['共享 Plugin Manager', 'Shared Plugin Manager'],
        ],
        true
      ),
      gate(
        'EV0',
        '治理式自主进化',
        'Governed self-evolution',
        '经授权的证据数据集、可复现评测与奖励策略、候选版本、人工审批、灰度、自动停止和精确回滚。',
        'Authorized evidence datasets, reproducible evaluation and reward policy, candidate revisions, human approval, canaries, automatic halt, and exact rollback.',
        'planned',
        [
          ['授权、脱敏与来源证明', 'Authorization, redaction, and provenance'],
          ['可复现评测与候选任务', 'Reproducible evaluation and candidate jobs'],
          ['审批、灰度与精确回滚', 'Approval, canary, and exact rollback'],
        ]
      ),
    ],
  },
  {
    id: 'data',
    title: copy('推理与有状态平台', 'Inference and stateful platform'),
    description: copy(
      '在通用 Workload、Runtime、Box 与 Gateway 边界上扩展模型服务和持久数据能力。',
      'Model serving and persistent data extend the common Workload, Runtime, Box, and Gateway boundaries.'
    ),
    gates: [
      gate(
        'PW0',
        'A3S Power 推理边界',
        'A3S Power inference boundary',
        'ACL 原生不可变 Power Service Profile、Box MicroVM / TEE 证据、健康、推理、恢复与清理。',
        'ACL-native immutable Power Service profiles, Box MicroVM / TEE evidence, health, inference, recovery, and cleanup.',
        'planned',
        [
          ['不可变 Power Service Profile', 'Immutable Power Service profiles'],
          ['MicroVM / TEE 证明', 'MicroVM / TEE evidence'],
          ['健康、恢复与清理', 'Health, recovery, and cleanup'],
        ]
      ),
      gate(
        'S0',
        '有状态与分布式存储平台',
        'Stateful and distributed storage platform',
        '在同一存储平面中区分不可变对象与带 Fencing 的可变 Volume，并提供数据库、备份、恢复、保留策略与分布式提供方一致性。',
        'One storage plane distinguishes immutable objects from fenced mutable volumes and provides databases, backup, restore, retention, and distributed-provider conformance.',
        'planned',
        [
          ['不可变对象与数据库 / Volume', 'Immutable objects and databases / volumes'],
          ['Fencing、备份与恢复', 'Fencing, backup, and restore'],
          ['分布式提供方、保留与导入', 'Distributed providers, retention, and import'],
        ]
      ),
      gate(
        'I0',
        '模型推理平台',
        'Inference profile',
        '加速器模型服务、OpenAI 兼容流量、范围化 Key、Provider、路由、用量与治理式自助服务。',
        'Accelerator-backed model serving, OpenAI-compatible traffic, scoped keys, providers, routing, usage, and governed self-service.',
        'planned',
        [
          ['Accelerator 与模型服务', 'Accelerators and model serving'],
          ['OpenAI 兼容流量与 Key', 'OpenAI-compatible traffic and keys'],
          ['Provider、路由与用量', 'Providers, routing, and usage'],
        ]
      ),
    ],
  },
];

export const DOCUMENTATION_VERSIONS: readonly DocumentationVersion[] = [
  {
    id: 'main',
    label: 'main',
    title: copy('当前开发版', 'Current development'),
    description: copy(
      '跟随当前代码与 2026-08-06 路线图快照，包含已验证、实现中、待复认证及计划能力。',
      'Tracks the current code and the 2026-08-06 roadmap snapshot, including verified, active, re-certification, and planned work.'
    ),
    source: copy(
      '来源：当前分支、README、ROADMAP 与开发计划',
      'Source: current branch, README, ROADMAP, and development plan'
    ),
  },
  {
    id: '0.1',
    label: 'v0.1.x',
    title: copy('0.1 兼容线', '0.1 compatibility line'),
    description: copy(
      '对应 Cargo 与 Web Package 0.1.0，以及 REST v1 的 1.6.0 合同。版本号不代表全部路线图 Gate 已验证。',
      'Covers Cargo and web package 0.1.0 with REST v1 contract 1.6.0. The version number does not imply that every roadmap gate is verified.'
    ),
    source: copy(
      '来源：Cargo.toml、package.json 与 openapi/v1.json',
      'Source: Cargo.toml, package.json, and openapi/v1.json'
    ),
  },
];

export const ALL_CAPABILITY_GATES = CAPABILITY_GROUPS.flatMap((group) => group.gates);

export const CAPABILITY_COUNTS = ALL_CAPABILITY_GATES.reduce<Record<CapabilityState, number>>(
  (counts, capability) => {
    counts[capability.state] += 1;
    return counts;
  },
  { verified: 0, 'in-progress': 0, recertification: 0, planned: 0 }
);

export function localize(value: LocalizedCopy, language: Language): string {
  return value[language];
}

function copy(zhCN: string, en: string): LocalizedCopy {
  return { 'zh-CN': zhCN, en };
}

function gate(
  code: string,
  zhTitle: string,
  enTitle: string,
  zhOutcome: string,
  enOutcome: string,
  state: CapabilityState,
  features: ReadonlyArray<readonly [string, string]>,
  unavailable = false
): CapabilityGate {
  return {
    code,
    title: copy(zhTitle, enTitle),
    outcome: copy(zhOutcome, enOutcome),
    features: features.map(([zhCN, en]) => copy(zhCN, en)),
    state,
    unavailable,
  };
}
