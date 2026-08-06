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
      zh: '以版本化关系数据为权威，派生可重建的对象、关系、规则、目标与约束投影',
      en: 'Uses versioned relational authority to derive rebuildable projections of objects, relations, rules, goals, and constraints',
    },
    icon: Graph,
    emphasis: true,
    meta: 'Object · Relation · Rule',
  },
  {
    title: 'A3S Runtime',
    description: {
      zh: '只提供 Task / Service 标准生命周期；WaaS、AaaS 与 FaaS 由上层编译复用',
      en: 'Exposes only Task and Service lifecycles; WaaS, AaaS, and FaaS compile to those primitives',
    },
    icon: Engine,
    emphasis: true,
    meta: 'Task · Service',
  },
  {
    title: 'Asset Hosting',
    titleZh: '资产托管',
    description: {
      zh: '联合展示并固定各权威上下文发布的不可变企业 AI 资产引用',
      en: 'Federates and pins immutable enterprise AI Asset references published by each owning context',
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
      zh: '以同一存储平面承载不可变对象与带 Fencing 的可变卷',
      en: 'Carries immutable objects and fenced mutable volumes through one storage plane',
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
              ? '统一网关产品由 Cloud API 管理面与 A3S Gateway 实时数据面组成。Workflow、Agent、MCP 与模型形成统一服务平台；所有能力继续复用 Cloud 现有的来源交付、资产、调度、运行、存储、安全、审计与恢复机制。'
              : 'The unified gateway product combines the Cloud API management plane with the A3S Gateway live data plane. Workflow, Agent, MCP, and model services form the unified service platform while reusing Cloud’s existing source delivery, assets, scheduling, runtime, storage, security, audit, and recovery mechanisms.'}
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
                    ? '集成事实 · 证据引用 · 数据轨迹'
                    : 'INTEGRATION FACTS · EVIDENCE REFERENCES · TRAJECTORIES'
                }
              />
              <EventBusRail language={language} />
              <LayerConnector
                label={
                  zh
                    ? '资源需求 · 放置意图 · 执行回执'
                    : 'RESOURCE DEMAND · PLACEMENT INTENT · RECEIPTS'
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
              ? 'Cloud API 与 A3S Gateway 分别负责管理面和实时数据面；Workflow、Agent、MCP、模型与自进化复用同一 Flow、Workloads、Fleet、Runtime、Box、存储和审计链路。A3S Event 只发布已提交的集成事实，控制命令与回执仍由 Fleet 和节点日志负责。'
              : 'Cloud API and A3S Gateway own the management and live data planes respectively. Workflow, Agent, MCP, model, and evolution capabilities reuse one Flow, Workloads, Fleet, Runtime, Box, storage, and audit path. A3S Event publishes committed integration facts only; Fleet and the node journal retain command and receipt authority.'}
          </p>
        </footer>
      </div>
    </section>
  );
}

function EventBusRail({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';
  const channels = zh
    ? ['领域事件', '生命周期事实', '证据引用', '轨迹清单', '目录更新']
    : [
        'Domain events',
        'Lifecycle facts',
        'Evidence refs',
        'Trajectory manifests',
        'Catalog updates',
      ];

  return (
    <section className="cloud-event-bus-rail">
      <span>
        <Queue aria-hidden="true" size={24} weight="duotone" />
      </span>
      <div>
        <small>{zh ? '集成事实总线' : 'INTEGRATION FACT BUS'}</small>
        <strong>A3S Event</strong>
      </div>
      <p>
        {zh
          ? '通过事务 Outbox 发布已提交事实，不替代命令、回执与审计权威'
          : 'Publishes committed facts through the transactional Outbox without replacing command, receipt, or audit authority'}
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
    ? ['授权数据集', '可复现评测', '候选版本', '审批与回滚']
    : [
        'Authorized datasets',
        'Reproducible evaluation',
        'Candidates',
        'Approval and rollback',
      ];

  return (
    <aside
      className="cloud-evolution-rail"
      aria-label={zh ? '自进化系统' : 'Self-evolution system'}
    >
      <ArrowsClockwise aria-hidden="true" size={26} weight="duotone" />
      <div>
        <strong>{zh ? '自进化系统' : 'Self-Evolution'}</strong>
        <small>{zh ? '治理式演进模型' : 'GOVERNED EVOLUTION MODEL'}</small>
      </div>
      <p>
        {zh
          ? '基于授权证据生成候选版本，经评测、审批、灰度与回滚后进入现有发布链路'
          : 'Produces candidates from authorized evidence, then uses evaluation, approval, canary, and rollback through existing release paths'}
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
    ? ['管理 API', '统一身份', '协议策略', '智能路由', '实时流量', '端云证据']
    : [
        'Management API',
        'Identity',
        'Protocol policy',
        'Routing',
        'Live traffic',
        'Cloud-edge evidence',
      ];

  return (
    <section className="cloud-gateway-product-rail">
      <span>
        <Broadcast aria-hidden="true" size={25} weight="duotone" />
      </span>
      <div>
        <small>
          {zh ? '管理面 + 实时数据面' : 'MANAGEMENT + LIVE DATA PLANE'}
        </small>
        <strong>
          {zh ? 'Cloud API + A3S Gateway' : 'Cloud API + A3S Gateway'}
        </strong>
      </div>
      <p>
        {zh
          ? '统一治理 Workflow、Agent、MCP、模型 API 与业务服务，管理命令仍由 Cloud API 承载'
          : 'Governs Workflow, Agents, MCP, model APIs, and business services while management commands remain on Cloud API'}
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
          ? '贯穿基础设施与应用，并按授权导出证据；不直接驱动生产变更'
          : 'Observes infrastructure through applications and exports authorized evidence without directly driving production changes'}
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
            ? '可视化交互与命令行自动化共同进入 Cloud API 管理面'
            : 'Visual interaction and command-line automation share the Cloud API management plane'}
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
