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
    basedOn: 'A3S Gateway',
    title: {
      zh: '让所有智能流量通过一个可信入口',
      en: 'Move every intelligent workload through one trusted gateway',
    },
    promise: {
      zh: 'A3S Gateway 统一网关',
      en: 'A3S Gateway unified gateway',
    },
    body: {
      zh: 'A3S Gateway 统一治理 Workflow、Agent、MCP、模型 API 与业务服务，承载接入、身份、协议、策略、路由和实时流量治理。它连接 A3S Work、端侧节点与云端服务；AnySentry 则从基础设施贯穿上层应用，并将全链路数据轨迹回流到治理链路。',
      en: 'A3S Gateway governs Workflows, Agents, MCP, model APIs, and business services through one ingress, identity, protocol, policy, routing, and live-traffic plane. It connects A3S Work, edge nodes, and cloud services, while AnySentry observes the full path from infrastructure to applications and returns data trajectories to governance.',
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
    gateCodes: ['H0', 'C0', 'E0'],
    roadmapNote: {
      zh: '统一网关产品持续收敛端云接入、身份、策略、路由与证据链路；未通过路线 Gate 的能力仍明确标记为不可用。',
      en: 'The unified gateway product continues to converge cloud-edge ingress, identity, policy, routing, and evidence; capabilities outside verified roadmap gates remain explicitly unavailable.',
    },
  },
  {
    id: 'workflow',
    index: '02',
    basedOn: 'A3S Workflow',
    title: {
      zh: '让业务目标在真实世界中自主推进',
      en: 'Let business goals advance autonomously in the real world',
    },
    promise: {
      zh: 'Workflow 自主工作流编排',
      en: 'Workflow autonomous orchestration',
    },
    body: {
      zh: 'A3S Workflow 先用对象、关系、规则、目标与约束构建业务本体，再把认知模型编译为可执行计划。系统持续协调 Agent、工具、人员与业务系统，并由 A3S Runtime 通过 WaaS 提供可恢复的长期运行能力。',
      en: 'A3S Workflow models the business domain through objects, relationships, rules, goals, and constraints, then compiles that cognitive model into an executable plan. It coordinates Agents, tools, people, and services while A3S Runtime provides recoverable long-running execution through WaaS.',
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
    gateCodes: ['F0', 'C0', 'P0'],
    roadmapNote: {
      zh: '产品集成进行中，编排语义由 A3S Workflow 承载，长期任务通过 A3S Runtime 的 WaaS 运行合约交付。',
      en: 'Product integration is in progress; A3S Workflow owns orchestration semantics while long-running work is delivered through the A3S Runtime WaaS contract.',
    },
  },
  {
    id: 'agent-factory',
    index: '03',
    basedOn: 'A3S Runtime + A3S Box',
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
    gateCodes: ['A0', 'A1', 'MCP0', 'I0'],
    roadmapNote: {
      zh: '异构资产、托管与执行主链路正在实现，MCP0 的部分子能力仍明确标记为不可用。',
      en: 'The heterogeneous asset, hosting, and execution path is in progress, with unavailable MCP0 sub-capabilities still labeled explicitly.',
    },
  },
];
