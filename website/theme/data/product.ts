export type HomeLanguage = 'zh' | 'en';

export type LocalizedText = {
  zh: string;
  en: string;
};

export type ProductId = 'workflow' | 'agent-factory' | 'security-operations';

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
    id: 'workflow',
    index: '01',
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
      zh: 'A3S Workflow 先用对象、关系、规则、目标与约束构建业务本体，再把认知模型编译为可执行计划。系统持续协调 Agent、工具、人员与业务系统，并通过 A3S Flow 在中断后从已确认状态继续运行。',
      en: 'A3S Workflow models the business domain through objects, relationships, rules, goals, and constraints, then compiles that cognitive model into an executable plan. It coordinates Agents, tools, people, and services, while A3S Flow resumes work from confirmed state after interruption.',
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
      zh: '产品集成进行中，编排语义由 A3S Workflow 承载，长期任务复用 Operations 与 A3S Flow 的唯一持久化边界。',
      en: 'Product integration is in progress and reuses the sole durable boundary owned by Operations and A3S Flow.',
    },
  },
  {
    id: 'agent-factory',
    index: '02',
    basedOn: 'A3S Code',
    title: {
      zh: '让不同技术栈的 Agent 进入同一条生产线',
      en: 'Bring heterogeneous Agents onto one production line',
    },
    promise: {
      zh: 'Agent Factory 异构智能体工厂',
      en: 'Agent Factory for heterogeneous Agents',
    },
    body: {
      zh: 'A3S Code 将不同模型、框架、Skill、MCP 与安全策略装配成不可变 Release，再沿同一资产链路完成评测、发布、部署、运行和审计。技术栈可以异构，执行契约保持统一，A3S Code Harness 始终是唯一 Agent 运行所有者。',
      en: 'A3S Code assembles different models, frameworks, Skills, MCP servers, and security policies into immutable releases, then evaluates, publishes, deploys, runs, and audits them through one asset path. Technology stacks may differ, while the execution contract stays unified and A3S Code Harness remains the sole Agent run owner.',
    },
    capabilities: [
      {
        title: { zh: '异构能力适配', en: 'Heterogeneous adaptation' },
        body: {
          zh: '用统一契约接入不同 Agent 框架、模型、Skill 与 MCP 服务。',
          en: 'Connect different Agent frameworks, models, Skills, and MCP services through one contract.',
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
        title: { zh: '统一运行契约', en: 'Unified runtime contract' },
        body: {
          zh: '会话、审批、检查点、语义事件和运行证据沿唯一 Harness 执行身份沉淀。',
          en: 'Keep conversations, approvals, checkpoints, semantic events, and evidence on the sole Harness identity.',
        },
      },
    ],
    gateCodes: ['A0', 'A1', 'MCP0', 'I0'],
    roadmapNote: {
      zh: '异构资产与执行主链路正在实现，MCP0 的部分子能力仍明确标记为不可用。',
      en: 'The asset and execution path is in progress, with unavailable MCP0 sub-capabilities still labeled explicitly.',
    },
  },
  {
    id: 'security-operations',
    index: '03',
    basedOn: 'A3S Gateway + A3S Sentry + AnySentry',
    title: {
      zh: '把每一次运行变成可响应的安全信号',
      en: 'Turn every execution into an actionable security signal',
    },
    promise: {
      zh: '安全监控中台',
      en: 'Security monitoring and operations center',
    },
    body: {
      zh: '汇聚 A3S Gateway 流量、A3S Runtime 与 Box 运行状态、Fleet 命令、租户身份、A3S Sentry 与 AnySentry 告警，把分散信号收敛为统一事件、策略判断、处置动作与审计证据。',
      en: 'Unify A3S Gateway traffic, A3S Runtime and Box state, Fleet commands, tenant identity, A3S Sentry, and AnySentry alerts into one stream of events, policy decisions, response actions, and audit evidence.',
    },
    capabilities: [
      {
        title: { zh: '全链路观测', en: 'Full-path observability' },
        body: {
          zh: '把入口流量、控制命令、运行状态和语义事件放进同一时间线。',
          en: 'Place ingress traffic, control commands, runtime state, and semantic events on one timeline.',
        },
      },
      {
        title: { zh: '统一安全策略', en: 'Unified policy' },
        body: {
          zh: '复用租户、身份、ACL、mTLS 与运行隔离边界执行一致判断。',
          en: 'Apply consistent decisions across tenant, identity, ACL, mTLS, and runtime isolation boundaries.',
        },
      },
      {
        title: { zh: '闭环处置', en: 'Closed-loop response' },
        body: {
          zh: '从发现、研判到隔离、恢复和复盘，保留可验证的完整证据。',
          en: 'Retain verifiable evidence from detection and triage through isolation, recovery, and review.',
        },
      },
    ],
    gateCodes: ['C0', 'E0', 'H0'],
    roadmapNote: {
      zh: '安全与路由能力持续收敛，历史证据仍需完成 Box 基线复认证。',
      en: 'Security and routing capabilities continue to converge, while historical evidence still requires Box baseline re-certification.',
    },
  },
];
