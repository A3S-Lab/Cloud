export type HomeLanguage = 'zh' | 'en';

export type LocalizedText = {
  zh: string;
  en: string;
};

export type ProductId = 'workflow' | 'agent-factory' | 'unified-gateway';

export type ProductChapter = {
  id: ProductId;
  index: string;
  basedOn: string;
  navLabel: LocalizedText;
  title: LocalizedText;
  promise: LocalizedText;
  body: LocalizedText;
  capabilities: Array<{
    title: LocalizedText;
    body: LocalizedText;
  }>;
  gateCodes: string[];
  roadmapNote: LocalizedText;
};

export const localize = (copy: LocalizedText, language: HomeLanguage) =>
  copy[language];

export const productChapters: ProductChapter[] = [
  {
    id: 'unified-gateway',
    index: '01',
    basedOn: 'Cloud API + A3S Gateway',
    navLabel: { zh: '统一网关', en: 'Unified Gateway' },
    title: {
      zh: '让所有智能流量通过一个可信入口',
      en: 'Move every intelligent workload through one trusted gateway',
    },
    promise: {
      zh: 'A3S Gateway 统一网关',
      en: 'A3S Gateway unified gateway',
    },
    body: {
      zh: '统一网关产品由 Cloud API 管理面与 A3S Gateway 实时数据面共同组成：Cloud 管理 Workflow、Agent、MCP、模型 API 与业务服务的身份和期望策略，Gateway 承载协议、路由与实时流量。AnySentry 贯穿基础设施与应用，并把经过授权的证据回流到治理链路。',
      en: 'The unified gateway product combines the Cloud API management plane with the A3S Gateway live data plane. Cloud owns identity and desired policy for Workflows, Agents, MCP, model APIs, and business services; Gateway owns protocol, routing, and live traffic. AnySentry observes infrastructure and applications and returns authorized evidence to governance.',
    },
    capabilities: [
      {
        title: { zh: '统一接入与协议', en: 'Unified ingress and protocols' },
        body: {
          zh: '让 Agent、MCP、模型 API、工作流和业务服务通过一个端云一致的入口。',
          en: 'Give Agents, MCP, model APIs, workflows, and business services one consistent cloud-edge ingress.',
        },
      },
      {
        title: { zh: '身份、策略与路由', en: 'Identity, policy, and routing' },
        body: {
          zh: '在请求路径上统一执行租户身份、ACL、mTLS、配额、路由和发布策略。',
          en: 'Enforce tenant identity, ACL, mTLS, quota, routing, and publication policy on the request path.',
        },
      },
      {
        title: { zh: '端云流量与证据', en: 'Cloud-edge traffic and evidence' },
        body: {
          zh: '统一治理实时流量、长连接和边缘入口，并把网关证据交给 AnySentry 回流。',
          en: 'Govern live traffic, long-lived connections, and edge ingress while returning gateway evidence through AnySentry.',
        },
      },
    ],
    gateCodes: ['E0', 'C0', 'H0', 'MCP0', 'I0'],
    roadmapNote: {
      zh: '统一网关产品持续收敛端云接入、身份、策略、路由与证据链路；未通过路线 Gate 的能力仍明确标记为不可用。',
      en: 'The unified gateway product continues to converge cloud-edge ingress, identity, policy, routing, and evidence; capabilities outside verified roadmap gates remain explicitly unavailable.',
    },
  },
  {
    id: 'workflow',
    index: '02',
    basedOn: 'A3S Workflow',
    navLabel: { zh: '工作流编排', en: 'Workflow Orchestration' },
    title: {
      zh: '让业务目标在真实世界中自主推进',
      en: 'Let business goals advance autonomously in the real world',
    },
    promise: {
      zh: 'Workflow 自主工作流编排',
      en: 'Workflow autonomous orchestration',
    },
    body: {
      zh: 'A3S Workflow 先用对象、关系、规则、目标与约束构建业务本体，再把认知模型编译为可执行计划。Cloud Operations 与 A3S Flow 提供唯一的可恢复长期编排，计划中的计算步骤再复用 A3S Runtime Task 或 Service。',
      en: 'A3S Workflow models the business domain through objects, relationships, rules, goals, and constraints, then compiles that cognitive model into an executable plan. Cloud Operations and A3S Flow provide the sole recoverable long-running orchestration path, while executable steps reuse A3S Runtime Tasks or Services.',
    },
    capabilities: [
      {
        title: {
          zh: '认知驱动的本体工程',
          en: 'Cognitive ontology engineering',
        },
        body: {
          zh: '让业务对象、关系和规则成为可计算语义，不再把流程固化成脆弱脚本。',
          en: 'Turn business objects, relationships, and rules into computable semantics instead of brittle scripts.',
        },
      },
      {
        title: { zh: '目标驱动的自主协同', en: 'Goal-directed coordination' },
        body: {
          zh: '按目标动态拆解任务，在 Agent、工具、人员与系统之间分派工作。',
          en: 'Decompose goals dynamically and assign work across Agents, tools, people, and systems.',
        },
      },
      {
        title: { zh: '可恢复的长期运行', en: 'Recoverable long-running work' },
        body: {
          zh: '用状态、检查点、重试和证据支撑跨小时、跨天的可恢复流程。',
          en: 'Use state, checkpoints, retries, and evidence to support recoverable long-running work.',
        },
      },
    ],
    gateCodes: ['W0', 'C0', 'A1', 'MCP0', 'I0'],
    roadmapNote: {
      zh: 'W0 仍处于规划状态；Workflow 语义、Flow 编排和 Runtime 执行步骤将复用现有控制链路，在 W0 Gate 通过前不声明可用。',
      en: 'W0 remains planned. Workflow semantics, Flow orchestration, and Runtime execution steps will reuse the existing control path; no availability is claimed before the W0 gates pass.',
    },
  },
  {
    id: 'agent-factory',
    index: '03',
    basedOn: 'A3S Runtime + A3S Box',
    navLabel: { zh: '智能体工厂', en: 'Agent Factory' },
    title: {
      zh: '让不同技术栈的 Agent 进入同一条生产线',
      en: 'Bring heterogeneous Agents onto one production line',
    },
    promise: {
      zh: 'Agent Factory 异构智能体工厂',
      en: 'Agent Factory for heterogeneous Agents',
    },
    body: {
      zh: 'Agent Factory 可托管采用不同模型、框架、语言与 Harness 的任意异构 Agent，并把代码、Skill、MCP、依赖和安全策略固定为不可变 Release，沿统一资产链路完成评测、发布、部署、运行与审计。A3S Code 是原生 Harness 之一，而非唯一可托管 Harness。',
      en: 'Agent Factory hosts heterogeneous Agents built with different models, frameworks, languages, and harnesses. It pins code, Skills, MCP servers, dependencies, and security policy into immutable releases, then evaluates, publishes, deploys, runs, and audits them through one asset path. A3S Code is one native harness, not the only harness that can be hosted.',
    },
    capabilities: [
      {
        title: {
          zh: '任意异构 Agent 托管',
          en: 'Host any heterogeneous Agent',
        },
        body: {
          zh: '通过标准交付与运行契约接入不同语言、框架、模型、自定义 Harness、Skill 与 MCP 服务。',
          en: 'Connect different languages, frameworks, models, custom harnesses, Skills, and MCP services through standard delivery and runtime contracts.',
        },
      },
      {
        title: { zh: '资产化生产', en: 'Asset-based production' },
        body: {
          zh: '统一装配、评测、版本固定、发布和 Workload 部署，把能力变成企业资产。',
          en: 'Standardize assembly, evaluation, pinning, publishing, and Workload deployment as enterprise assets.',
        },
      },
      {
        title: { zh: '统一托管与证据契约', en: 'Unified hosting and evidence' },
        body: {
          zh: '不同 Harness 保留各自实现，同时统一身份、会话、审批、语义事件与运行证据。',
          en: 'Let each harness retain its implementation while unifying identity, conversations, approvals, semantic events, and run evidence.',
        },
      },
    ],
    gateCodes: ['A0', 'A1', 'MCP0', 'I0', 'EV0'],
    roadmapNote: {
      zh: 'A3S Code 原生 Provider 正在实现；异构 Harness 合约属于 A1.3，评测与自进化属于 EV0，未通过的子能力继续明确标记为不可用。',
      en: 'The native A3S Code provider is in progress. The heterogeneous Harness contract belongs to A1.3, while evaluation and self-evolution belong to EV0; unverified sub-capabilities remain explicitly unavailable.',
    },
  },
];
