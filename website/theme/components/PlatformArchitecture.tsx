import { withBase } from '@rspress/core/runtime';
import {
  ArrowDown,
  ArrowRight,
  ArrowsClockwise,
  Brain,
  BracketsAngle,
  Browser,
  Broadcast,
  Cpu,
  Cube,
  Database,
  DownloadSimple,
  Engine,
  Fingerprint,
  Graph,
  GraphicsCard,
  Kanban,
  Lightning,
  Path,
  PlugsConnected,
  Queue,
  Robot,
  TerminalWindow,
  TreeStructure,
  UsersThree,
  Vault,
  Waveform,
  type IconProps,
} from '@phosphor-icons/react';
import type { ComponentType } from 'react';
import type { HomeLanguage, LocalizedText } from '../data/product';

type ArchitectureItem = {
  title: string;
  titleZh?: string;
  description: LocalizedText;
  icon: ComponentType<IconProps>;
  emphasis?: boolean;
  meta?: string;
};

const unifiedServices: ArchitectureItem[] = [
  {
    title: 'Workflow Service',
    titleZh: '工作流服务',
    description: {
      zh: '本体语义、目标分解与自主编排服务',
      en: 'Ontology semantics, goal decomposition, and autonomous orchestration',
    },
    icon: TreeStructure,
    emphasis: true,
    meta: 'Workflow as a Service',
  },
  {
    title: 'Agent Service',
    titleZh: '智能体服务',
    description: {
      zh: '托管任意异构 Agent 与 Harness',
      en: 'Hosts any heterogeneous Agent and harness',
    },
    icon: Robot,
    emphasis: true,
    meta: 'Agent as a Service',
  },
  {
    title: 'MCP Service',
    titleZh: 'MCP 服务',
    description: {
      zh: '统一发现、连接与治理 MCP 服务端',
      en: 'Discovers, connects, and governs MCP servers',
    },
    icon: PlugsConnected,
    meta: 'MCP Registry + Gateway',
  },
  {
    title: 'Model Service',
    titleZh: '模型服务',
    description: {
      zh: '模型路由、推理服务与能力治理',
      en: 'Model routing, inference services, and capability governance',
    },
    icon: Brain,
    meta: 'Model API + Inference',
  },
];

const coreCapabilities: ArchitectureItem[] = [
  {
    title: 'Intelligent Orchestration Algorithms',
    titleZh: '智能编排算法',
    description: {
      zh: '将业务目标、规则与约束编译为可恢复的执行计划',
      en: 'Compiles business goals, rules, and constraints into recoverable execution plans',
    },
    icon: Path,
    emphasis: true,
    meta: 'Goal · Plan · Run',
  },
  {
    title: 'Ontology Knowledge Graph',
    titleZh: '本体知识图谱',
    description: {
      zh: '统一表达业务对象、关系、规则、目标与约束',
      en: 'Represents business objects, relations, rules, goals, and constraints',
    },
    icon: Graph,
    emphasis: true,
    meta: 'Object · Relation · Rule',
  },
  {
    title: 'A3S Runtime',
    description: {
      zh: '承载工作流、智能体与无状态函数的标准运行合约',
      en: 'Provides standard runtime contracts for workflows, Agents, and stateless functions',
    },
    icon: Engine,
    emphasis: true,
    meta: 'WaaS · AaaS · FaaS',
  },
  {
    title: 'Asset Hosting',
    titleZh: '资产托管',
    description: {
      zh: '托管、版本固定、发布和分发企业 AI 资产包',
      en: 'Hosts, pins, publishes, and distributes enterprise AI Asset Packages',
    },
    icon: Vault,
    emphasis: true,
    meta: 'Workflow · Agent · Model · MCP · Skill · OKF · Tool',
  },
];

const infrastructure: ArchitectureItem[] = [
  {
    title: 'A3S Box',
    description: {
      zh: '唯一执行与隔离 Provider',
      en: 'Sole execution and isolation provider',
    },
    icon: Cube,
  },
  {
    title: 'A3S Power',
    description: {
      zh: '模型无关推理与硬件可信证明',
      en: 'Model-neutral inference and hardware attestation',
    },
    icon: Lightning,
  },
  {
    title: 'Distributed File Storage',
    titleZh: '分布式文件存储',
    description: {
      zh: '统一持久化代码、数据与制品，并提供分布式访问能力',
      en: 'Persists code, data, and artifacts with distributed access',
    },
    icon: Database,
  },
  {
    title: 'Identity + Evidence',
    titleZh: '身份与证据',
    description: {
      zh: 'A3S ACL、mTLS、日志、回执与审计证据',
      en: 'A3S ACL, mTLS, logs, receipts, and audit evidence',
    },
    icon: Fingerprint,
  },
];

const hardwareClusters: ArchitectureItem[] = [
  {
    title: 'CPU Clusters',
    titleZh: 'CPU 集群',
    description: {
      zh: '承载控制任务、通用计算与大规模并发 Workload',
      en: 'Runs control tasks, general compute, and concurrent workloads',
    },
    icon: Cpu,
  },
  {
    title: 'GPU Clusters',
    titleZh: 'GPU 集群',
    description: {
      zh: '承载模型推理、Agentic RL 与加速计算',
      en: 'Runs model inference, Agentic RL, and accelerated compute',
    },
    icon: GraphicsCard,
  },
];

const localize = (copy: LocalizedText, language: HomeLanguage) =>
  copy[language];

export function PlatformArchitecture({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <section className="cloud-platform-architecture" id="architecture">
      <header className="cloud-architecture-heading" data-reveal>
        <div className="cloud-architecture-copy">
          <span>{zh ? '模块架构' : 'MODULE ARCHITECTURE'}</span>
          <h2>
            {zh ? 'A3S OS 分层产品架构' : 'A3S OS layered product architecture'}
          </h2>
          <p>
            {zh
              ? 'A3S Gateway 统一治理 A3S Work、A3S CLI、端侧节点与外部系统的智能流量。工作流、智能体、MCP 与模型组成统一服务平台；核心层提供智能编排算法、本体知识图谱、A3S Runtime 与资产托管。'
              : 'A3S Gateway governs intelligent traffic from A3S Work, A3S CLI, edge nodes, and external systems. Workflow, Agent, MCP, and model services form the unified service platform; the core layer provides intelligent orchestration algorithms, an ontology knowledge graph, A3S Runtime, and asset hosting.'}
          </p>
        </div>
        <div className="cloud-architecture-actions">
          <a href={withBase('/architecture/')}>
            {zh ? '交互式架构' : 'Interactive architecture'}
            <ArrowRight aria-hidden="true" weight="bold" />
          </a>
        </div>
      </header>

      <div className="cloud-platform-map" data-reveal>
        <ClientLayer language={language} />
        <GatewayProductRail language={language} />

        <div className="cloud-platform-shell">
          <div className="cloud-platform-body">
            <EvolutionRail language={language} />
            <div className="cloud-platform-layers">
              <LayerConnector
                label={
                  zh
                    ? '身份 · 协议 · 策略 · 路由'
                    : 'IDENTITY · PROTOCOLS · POLICY · ROUTING'
                }
              />
              <ArchitectureLayer
                items={unifiedServices}
                language={language}
                label={zh ? '统一服务平台' : 'UNIFIED SERVICE PLATFORM'}
                tone="services"
              />
              <LayerConnector
                label={
                  zh
                    ? '服务调用 · 编排语义 · 本体上下文'
                    : 'SERVICE INVOCATION · ORCHESTRATION · ONTOLOGY CONTEXT'
                }
              />
              <ArchitectureLayer
                items={coreCapabilities}
                language={language}
                label={zh ? '核心层' : 'CORE LAYER'}
                tone="core"
              />
              <LayerConnector
                label={
                  zh
                    ? '隔离 · 推理 · 数据 · 信任'
                    : 'ISOLATION · INFERENCE · DATA · TRUST'
                }
              />
              <ArchitectureLayer
                items={infrastructure}
                language={language}
                label={zh ? '基础设施服务层' : 'INFRASTRUCTURE SERVICES'}
                tone="foundation"
              />
              <LayerConnector
                label={
                  zh
                    ? '事件、命令、回执与数据轨迹'
                    : 'EVENTS, COMMANDS, RECEIPTS, AND DATA TRAJECTORIES'
                }
              />
              <EventBusRail language={language} />
              <LayerConnector
                label={
                  zh
                    ? '资源需求 · 调度事件 · 执行回执'
                    : 'RESOURCE DEMAND · SCHEDULING EVENTS · RECEIPTS'
                }
              />
              <SchedulingRail language={language} />
              <LayerConnector
                label={
                  zh
                    ? '容量 · 放置 · 资源声明'
                    : 'CAPACITY · PLACEMENT · RESOURCE CLAIMS'
                }
              />
              <ArchitectureLayer
                items={hardwareClusters}
                language={language}
                label={zh ? '硬件基础设施' : 'HARDWARE INFRASTRUCTURE'}
                tone="hardware"
              />
            </div>
            <ObservabilityRail language={language} />
          </div>
        </div>

        <footer>
          <div>
            <UsersThree aria-hidden="true" size={22} weight="duotone" />
            <strong>{zh ? '清晰职责边界' : 'CLEAR LAYER BOUNDARIES'}</strong>
          </div>
          <p>
            {zh
              ? 'A3S Gateway 负责统一流量治理；核心层提供智能编排算法、本体知识图谱、A3S Runtime 与资产托管；统一服务平台提供 Workflow、Agent、MCP 与模型服务；A3S Event 传递事件、命令与回执。'
              : 'A3S Gateway governs traffic. The core layer provides intelligent orchestration algorithms, an ontology knowledge graph, A3S Runtime, and asset hosting. The service platform exposes Workflow, Agent, MCP, and model services; A3S Event carries events, commands, and receipts.'}
          </p>
        </footer>
      </div>
    </section>
  );
}

function EventBusRail({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';
  const channels = zh
    ? ['领域事件', '控制命令', '执行回执', '轨迹回流', '审计事实']
    : ['Domain events', 'Commands', 'Receipts', 'Trajectories', 'Audit facts'];

  return (
    <section className="cloud-event-bus-rail">
      <span>
        <Queue aria-hidden="true" size={24} weight="duotone" />
      </span>
      <div>
        <small>{zh ? '事件与命令总线' : 'EVENT AND COMMAND BUS'}</small>
        <strong>A3S Event</strong>
      </div>
      <p>
        {zh
          ? '传递领域事件、控制命令、执行回执与审计事实'
          : 'Carries domain events, control commands, execution receipts, and audit facts'}
      </p>
      <ul>
        {channels.map((channel) => (
          <li key={channel}>{channel}</li>
        ))}
      </ul>
    </section>
  );
}

function SchedulingRail({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <section className="cloud-scheduling-rail">
      <span>
        <Kanban aria-hidden="true" size={24} weight="duotone" />
      </span>
      <div>
        <small>{zh ? '硬件资源调度' : 'HARDWARE RESOURCE SCHEDULING'}</small>
        <strong>
          {zh ? 'Workloads + Fleet 统一调度' : 'Workloads + Fleet scheduling'}
        </strong>
      </div>
      <p>
        {zh
          ? '统一管理容量、放置、资源声明与 CPU/GPU 集群分配'
          : 'Manages capacity, placement, resource claims, and CPU/GPU allocation'}
      </p>
      <ul>
        <li>{zh ? '容量管理' : 'Capacity'}</li>
        <li>{zh ? '工作负载放置' : 'Placement'}</li>
        <li>{zh ? '集群资源声明' : 'Resource claims'}</li>
      </ul>
    </section>
  );
}

function EvolutionRail({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';
  const feedback = zh
    ? ['数据轨迹', '评测奖励', '运行证据', '安全约束']
    : ['Trajectories', 'Rewards', 'Run evidence', 'Safety'];

  return (
    <aside
      className="cloud-evolution-rail"
      aria-label={zh ? '自进化系统' : 'Self-evolution system'}
    >
      <ArrowsClockwise aria-hidden="true" size={26} weight="duotone" />
      <div>
        <strong>{zh ? '自进化系统' : 'Self-Evolution'}</strong>
        <small>{zh ? '双螺旋自进化机制' : 'DUAL-HELIX EVOLUTION'}</small>
      </div>
      <p>
        {zh
          ? '消费 AnySentry 回流轨迹，在安全约束下协同演进'
          : 'Consumes AnySentry trajectories and co-evolves under safety constraints'}
      </p>
      <div className="cloud-evolution-helix" aria-hidden="true">
        <span className="is-model">
          <b>MODEL</b>
          <strong>Agentic RL</strong>
        </span>
        <svg viewBox="0 0 96 164">
          <path
            className="is-model"
            d="M22 2 C82 22 82 58 22 82 C-7 94 2 132 74 162"
          />
          <path
            className="is-harness"
            d="M74 2 C14 22 14 58 74 82 C103 94 94 132 22 162"
          />
          <g>
            <line x1="30" x2="66" y1="12" y2="12" />
            <line x1="20" x2="76" y1="40" y2="40" />
            <line x1="31" x2="65" y1="68" y2="68" />
            <line x1="31" x2="65" y1="96" y2="96" />
            <line x1="20" x2="76" y1="124" y2="124" />
            <line x1="30" x2="66" y1="152" y2="152" />
          </g>
        </svg>
        <span className="is-harness">
          <b>HARNESS</b>
          <strong>{zh ? '自学习' : 'Self-Learning'}</strong>
        </span>
      </div>
      <ul>
        {feedback.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </aside>
  );
}

function GatewayProductRail({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';
  const capabilities = zh
    ? ['统一接入', '身份策略', '协议适配', '智能路由', '实时流量', '端云治理']
    : [
        'Ingress',
        'Identity',
        'Protocols',
        'Routing',
        'Live traffic',
        'Cloud-edge',
      ];

  return (
    <section className="cloud-gateway-product-rail">
      <span>
        <Broadcast aria-hidden="true" size={25} weight="duotone" />
      </span>
      <div>
        <small>{zh ? '统一流量治理' : 'UNIFIED TRAFFIC GOVERNANCE'}</small>
        <strong>
          {zh ? 'A3S Gateway 统一网关' : 'A3S Gateway unified gateway'}
        </strong>
      </div>
      <p>
        {zh
          ? '统一接入 Workflow、Agent、MCP、模型 API 与业务服务'
          : 'Provides unified ingress for Workflow, Agents, MCP, model APIs, and business services'}
      </p>
      <ul>
        {capabilities.map((capability) => (
          <li key={capability}>{capability}</li>
        ))}
      </ul>
    </section>
  );
}

function ObservabilityRail({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';
  const signals = zh
    ? ['指标', '日志', '链路', '事件']
    : ['Metrics', 'Logs', 'Traces', 'Events'];

  return (
    <aside
      className="cloud-observability-rail"
      aria-label={
        zh ? 'AnySentry 可观测性平台' : 'AnySentry observability platform'
      }
    >
      <Waveform aria-hidden="true" size={26} weight="duotone" />
      <div>
        <strong>AnySentry</strong>
        <small>{zh ? '全栈可观测性平台' : 'FULL-STACK OBSERVABILITY'}</small>
      </div>
      <p>
        {zh
          ? '从基础设施贯穿上层应用，并回流数据轨迹'
          : 'Observes infrastructure through applications and returns data trajectories'}
      </p>
      <div className="cloud-observability-loop" aria-hidden="true">
        <span>
          <b>APPLICATION</b>
          <strong>
            {zh ? '指标 · 日志 · 链路' : 'Metrics · Logs · Traces'}
          </strong>
        </span>
        <svg viewBox="0 0 96 164">
          <path className="is-forward" d="M48 8 V156" />
          <path
            className="is-return"
            d="M48 150 C84 134 84 102 48 86 C12 70 12 38 48 20"
          />
          <circle cx="48" cy="22" r="6" />
          <circle cx="48" cy="82" r="6" />
          <circle cx="48" cy="142" r="6" />
          <line x1="25" x2="71" y1="52" y2="52" />
          <line x1="25" x2="71" y1="112" y2="112" />
        </svg>
        <span>
          <b>INFRASTRUCTURE</b>
          <strong>{zh ? '数据轨迹回流' : 'Trajectory Return'}</strong>
        </span>
      </div>
      <ul>
        {signals.map((signal) => (
          <li key={signal}>{signal}</li>
        ))}
      </ul>
    </aside>
  );
}

function ClientLayer({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <section className="cloud-architecture-client">
      <div className="cloud-entry-heading">
        <strong>{zh ? '平级调用入口' : 'PEER INVOCATION ENTRY'}</strong>
        <small>
          {zh
            ? '可视化交互与命令行自动化共同进入统一网关'
            : 'Visual interaction and command-line automation share one gateway'}
        </small>
      </div>
      <div className="cloud-entry-channels">
        <article className="is-work">
          <span>
            <Browser aria-hidden="true" size={27} weight="duotone" />
          </span>
          <div>
            <strong>A3S Work</strong>
            <small>
              {zh
                ? '可视化工作台、审批、运行态势与证据'
                : 'Visual workspace, approvals, live state, and evidence'}
            </small>
          </div>
          <em>
            <DownloadSimple aria-hidden="true" size={17} weight="duotone" />
            <span>
              <b>A3S Use</b>
              {zh ? '按需热插拔资产包' : 'On-demand Asset Packages'}
            </span>
          </em>
        </article>
        <article className="is-cli">
          <span>
            <TerminalWindow aria-hidden="true" size={27} weight="duotone" />
          </span>
          <div>
            <strong>A3S CLI</strong>
            <small>
              {zh
                ? '命令行调用、脚本自动化与 CI/CD 集成'
                : 'Command invocation, scripting, and CI/CD integration'}
            </small>
          </div>
          <em>
            <BracketsAngle aria-hidden="true" size={17} weight="duotone" />
            <span>
              <b>CLI / API</b>
              {zh
                ? '批处理 · 自动化 · 集成'
                : 'Batch · Automation · Integration'}
            </span>
          </em>
        </article>
      </div>
    </section>
  );
}

function ArchitectureLayer({
  items,
  label,
  language,
  tone,
}: {
  items: ArchitectureItem[];
  label: string;
  language: HomeLanguage;
  tone: string;
}) {
  return (
    <section className={`cloud-architecture-layer is-${tone}`}>
      <h3>{label}</h3>
      <div>
        {items.map(
          ({ description, emphasis, icon: Icon, meta, title, titleZh }) => (
            <article
              className={emphasis ? 'is-emphasis' : undefined}
              key={title}
            >
              <span>
                <Icon aria-hidden="true" size={23} weight="duotone" />
              </span>
              <div>
                <strong>
                  {language === 'zh' && titleZh ? titleZh : title}
                </strong>
                {meta && <em>{meta}</em>}
                <small>{localize(description, language)}</small>
              </div>
            </article>
          ),
        )}
      </div>
    </section>
  );
}

function LayerConnector({ label }: { label: string }) {
  return (
    <div className="cloud-layer-connector" aria-hidden="true">
      <i />
      <span>
        <ArrowDown size={14} weight="bold" />
        {label}
      </span>
      <i />
    </div>
  );
}
