import { withBase } from '@rspress/core/runtime';
import {
  ArrowDown,
  ArrowsClockwise,
  Brain,
  Broadcast,
  Browser,
  Buildings,
  Code,
  Cube,
  DesktopTower,
  Engine,
  Factory,
  Fingerprint,
  FlowArrow,
  PlugsConnected,
  Pulse,
  Receipt,
  SealCheck,
  ShieldCheck,
  Tag,
} from '@phosphor-icons/react';
import type { HomeLanguage, ProductId } from '../data/product';

type ChartProps = {
  language: HomeLanguage;
};

const chartCopy = {
  workflow: {
    title: {
      zh: '从业务本体到执行协同',
      en: 'From business ontology to coordinated execution',
    },
    subtitle: {
      zh: '本体关系、编排状态与执行主体的语义投影',
      en: 'Semantic projection of ontology, orchestration state, and execution actors',
    },
  },
  factory: {
    title: {
      zh: '异构 Agent 资产化与托管链路',
      en: 'Heterogeneous Agent asset and hosting path',
    },
    subtitle: {
      zh: '保留各自 Harness 实现，以统一 Release、身份与证据契约进入生产',
      en: 'Retain each harness implementation while entering production through one release, identity, and evidence contract',
    },
  },
  gateway: {
    title: {
      zh: '统一网关，治理所有智能流量',
      en: 'One gateway governs every intelligent workload',
    },
    subtitle: {
      zh: '统一接入、身份、协议、策略、路由与端云流量治理',
      en: 'Unified ingress, identity, protocols, policy, routing, and cloud-edge traffic governance',
    },
  },
} as const;

const pick = (copy: { zh: string; en: string }, language: HomeLanguage) =>
  copy[language];

export function HeroVisual({ language }: ChartProps) {
  const zh = language === 'zh';
  const ingress = [
    { icon: Browser, label: 'A3S Work' },
    { icon: Code, label: 'A3S CLI' },
    {
      icon: PlugsConnected,
      label: zh ? '任意 Agent / MCP' : 'Any Agent / MCP',
    },
  ];
  const capabilities = [
    {
      code: 'GATEWAY',
      icon: Broadcast,
      title: zh ? '统一流量治理' : 'Unified traffic governance',
      body: zh
        ? '接入 · 身份 · 策略 · 路由'
        : 'Ingress · Identity · Policy · Routing',
    },
    {
      code: 'ORCHESTRATE',
      icon: Brain,
      title: zh ? '认知与智能编排' : 'Cognitive orchestration',
      body: zh
        ? '本体 · 规划 · 状态 · 协同'
        : 'Ontology · Planning · State · Coordination',
    },
    {
      code: 'RUNTIME',
      icon: Engine,
      title: zh ? '多形态服务运行' : 'Multi-modal service runtime',
      body: 'Workflow · Agent · Function as a Service',
    },
    {
      code: 'EXECUTE',
      icon: ShieldCheck,
      title: zh ? '端云安全执行' : 'Secure cloud-edge execution',
      body: zh
        ? '端云路由 · Box 隔离 · CPU / GPU 调度'
        : 'Cloud-edge routing · Box isolation · CPU / GPU scheduling',
    },
  ];

  return (
    <figure
      className="cloud-hero-scene cloud-motion-scene"
      aria-label={
        zh
          ? 'A3S OS 系统能力执行动画'
          : 'A3S OS system capability execution animation'
      }
    >
      <div className="cloud-hero-console">
        <header>
          <span>
            <img alt="" src={withBase('/a3s-os-logo.png')} />
            <span>
              <strong>A3S OS</strong>
              <small>
                {zh ? '企业智能运行空间' : 'ENTERPRISE INTELLIGENCE SPACE'}
              </small>
            </span>
          </span>
          <em>
            <i /> {zh ? '持续运行' : 'LIVE SYSTEM'}
          </em>
        </header>
        <div className="cloud-hero-field" aria-hidden="true">
          <div className="cloud-hero-ingress">
            <small>{zh ? '并列调用入口' : 'PARALLEL ENTRY POINTS'}</small>
            <div>
              {ingress.map(({ icon: Icon, label }) => (
                <span key={label}>
                  <Icon size={17} weight="duotone" />
                  <b>{label}</b>
                </span>
              ))}
            </div>
          </div>
          <div className="cloud-hero-capability-pipeline">
            <i className="cloud-hero-capability-signal" />
            {capabilities.map(({ body, code, icon: Icon, title }, index) => (
              <article className={`is-step-${index + 1}`} key={code}>
                <span>
                  <Icon size={21} weight="duotone" />
                </span>
                <div>
                  <strong>{title}</strong>
                  <small>{body}</small>
                </div>
                <em>{code}</em>
              </article>
            ))}
          </div>
        </div>
        <footer className="cloud-hero-feedback">
          <span>
            <Pulse size={20} weight="duotone" />
            <span>
              <strong>AnySentry</strong>
              <small>
                {zh ? '全链路可观测与轨迹回流' : 'Full-path observability'}
              </small>
            </span>
          </span>
          <span>
            <ArrowsClockwise size={20} weight="duotone" />
            <span>
              <strong>{zh ? '双螺旋自进化' : 'Dual-helix evolution'}</strong>
              <small>Agentic RL · Harness Learning</small>
            </span>
          </span>
        </footer>
      </div>
    </figure>
  );
}

export function ProductChart({ id, language }: ChartProps & { id: ProductId }) {
  if (id === 'workflow') return <WorkflowChart language={language} />;
  if (id === 'agent-factory') return <AgentFactoryChart language={language} />;
  return <GatewayChart language={language} />;
}

function WorkflowChart({ language }: ChartProps) {
  const ontology =
    language === 'zh'
      ? ['对象', '关系', '规则', '目标']
      : ['Objects', 'Relations', 'Rules', 'Goals'];
  const engine =
    language === 'zh'
      ? ['语义解析', '自主规划', '状态持续']
      : ['Interpret', 'Plan', 'Persist'];
  const actors =
    language === 'zh'
      ? ['Agent', '工具', '人员', '业务系统']
      : ['Agents', 'Tools', 'People', 'Systems'];
  const paths = [
    'M118 76 C214 76 214 92 322 92 C430 92 430 76 602 76',
    'M118 136 C220 136 228 92 322 92 C436 92 448 136 602 136',
    'M118 196 C222 196 232 174 322 174 C430 174 448 196 602 196',
    'M118 256 C226 256 238 256 322 256 C430 256 452 256 602 256',
    'M118 76 C208 76 236 174 322 174 C442 174 470 256 602 256',
    'M118 196 C218 196 244 256 322 256 C438 256 462 136 602 136',
  ];

  return (
    <figure
      className="cloud-editorial-chart cloud-workflow-chart cloud-motion-scene"
      aria-labelledby="workflow-chart-title"
    >
      <figcaption>
        <strong id="workflow-chart-title">
          {pick(chartCopy.workflow.title, language)}
        </strong>
        <span>{pick(chartCopy.workflow.subtitle, language)}</span>
      </figcaption>
      <div className="cloud-chart-canvas" aria-hidden="true">
        <svg viewBox="48 0 624 300">
          <defs>
            <marker
              id="cloud-workflow-arrow"
              markerHeight="8"
              markerWidth="8"
              orient="auto"
              refX="7"
              refY="4"
              viewBox="0 0 8 8"
            >
              <path className="cloud-chart-arrow" d="M0 0 L8 4 L0 8 Z" />
            </marker>
          </defs>
          <g className="cloud-chart-guides">
            <line x1="118" y1="46" x2="118" y2="276" />
            <line x1="322" y1="46" x2="322" y2="276" />
            <line x1="602" y1="46" x2="602" y2="276" />
          </g>
          <g className="cloud-chart-column-labels">
            <text x="118" y="22">
              01 ONTOLOGY
            </text>
            <text x="322" y="22">
              02 ORCHESTRATE
            </text>
            <text x="602" y="22">
              03 EXECUTE
            </text>
          </g>
          <g className="cloud-chart-threads">
            {paths.map((path, index) => (
              <path
                d={path}
                key={path}
                markerEnd={index < 4 ? 'url(#cloud-workflow-arrow)' : undefined}
              />
            ))}
          </g>
          <g className="cloud-chart-flows">
            {paths.map((path) => (
              <path d={path} key={`flow-${path}`} />
            ))}
          </g>
          {ontology.map((label, index) => (
            <g
              className={`cloud-chart-node is-ontology is-step-${index + 1}`}
              key={label}
              transform={`translate(118 ${76 + index * 60})`}
            >
              <circle r="7" />
              <text x="-14" y="4" textAnchor="end">
                {label}
              </text>
            </g>
          ))}
          {engine.map((label, index) => (
            <g
              className={`cloud-chart-node is-core is-step-${index + 1}`}
              key={label}
              transform={`translate(322 ${92 + index * 82})`}
            >
              <circle r="11" />
              <text x="14" y="4">
                {label}
              </text>
            </g>
          ))}
          {actors.map((label, index) => (
            <g
              className={`cloud-chart-node is-actor is-step-${index + 1}`}
              key={label}
              transform={`translate(602 ${76 + index * 60})`}
            >
              <circle r="7" />
              <text x="14" y="4">
                {label}
              </text>
            </g>
          ))}
        </svg>
      </div>
    </figure>
  );
}

function AgentFactoryChart({ language }: ChartProps) {
  const inputs =
    language === 'zh'
      ? ['Agent', 'Harness', 'Skills / MCP', '策略']
      : ['Agent', 'Harness', 'Skills / MCP', 'Policy'];
  const stages =
    language === 'zh'
      ? ['适配合约', '不可变 Release', 'Workload', '运行证据']
      : ['Adapt', 'Immutable release', 'Workload', 'Evidence'];
  const stageIcons = [PlugsConnected, SealCheck, Cube, Receipt];

  return (
    <figure
      className="cloud-editorial-chart cloud-factory-chart cloud-motion-scene"
      aria-labelledby="factory-chart-title"
    >
      <figcaption>
        <strong id="factory-chart-title">
          {pick(chartCopy.factory.title, language)}
        </strong>
        <span>{pick(chartCopy.factory.subtitle, language)}</span>
      </figcaption>
      <div className="cloud-factory-scene" aria-hidden="true">
        <div className="cloud-factory-input-stack">
          <small>
            {language === 'zh' ? '异构输入' : 'HETEROGENEOUS INPUTS'}
          </small>
          {inputs.map((label, index) => (
            <span className={`is-tone-${(index % 4) + 1}`} key={label}>
              <i />
              {label}
            </span>
          ))}
        </div>
        <div className="cloud-factory-assembly">
          <div className="cloud-factory-stage-line">
            {stages.map((label, index) => {
              const Icon = stageIcons[index];
              return (
                <article className={`is-stage-${index + 1}`} key={label}>
                  <span>
                    <Icon size={24} weight="duotone" />
                  </span>
                  <strong>{label}</strong>
                  <small>{String(index + 1).padStart(2, '0')}</small>
                </article>
              );
            })}
          </div>
          <div className="cloud-factory-contract">
            <Code size={24} weight="duotone" />
            <span>
              <strong>Agent Hosting Contract</strong>
              <small>
                {language === 'zh'
                  ? '任意 Agent · 任意 Harness · 统一身份与证据'
                  : 'Any Agent · Any harness · Unified identity and evidence'}
              </small>
            </span>
          </div>
          <div className="cloud-factory-state">
            <span>
              <Tag size={17} weight="duotone" />
              {language === 'zh' ? '版本已固定' : 'Version pinned'}
            </span>
            <span>
              <Fingerprint size={17} weight="duotone" />
              {language === 'zh' ? '身份已绑定' : 'Identity bound'}
            </span>
            <span>
              <ShieldCheck size={17} weight="duotone" />
              {language === 'zh' ? '策略已装载' : 'Policy attached'}
            </span>
          </div>
        </div>
      </div>
    </figure>
  );
}

function GatewayChart({ language }: ChartProps) {
  const ingress = [
    { icon: Browser, label: 'A3S Work' },
    {
      icon: PlugsConnected,
      label: language === 'zh' ? 'Agent / MCP' : 'Agents / MCP',
    },
    { icon: Brain, label: language === 'zh' ? '模型 API' : 'Model APIs' },
    {
      icon: DesktopTower,
      label: language === 'zh' ? '边缘设备' : 'Edge devices',
    },
  ];
  const destinations = [
    { icon: FlowArrow, label: 'Workflow' },
    { icon: Cube, label: 'Workloads' },
    { icon: Engine, label: 'Runtime / Box' },
    {
      icon: Buildings,
      label: language === 'zh' ? '业务服务' : 'Business services',
    },
  ];
  const policies =
    language === 'zh'
      ? ['身份', '协议', '策略', '路由', '流量', '证据']
      : ['Identity', 'Protocol', 'Policy', 'Routing', 'Traffic', 'Evidence'];

  return (
    <figure
      className="cloud-editorial-chart cloud-gateway-chart cloud-motion-scene"
      aria-labelledby="gateway-chart-title"
    >
      <figcaption>
        <strong id="gateway-chart-title">
          {pick(chartCopy.gateway.title, language)}
        </strong>
        <span>{pick(chartCopy.gateway.subtitle, language)}</span>
      </figcaption>
      <div className="cloud-gateway-map" aria-hidden="true">
        <div className="cloud-gateway-endpoints is-ingress">
          <small>{language === 'zh' ? '调用入口' : 'ENTRY POINTS'}</small>
          <div>
            {ingress.map(({ icon: Icon, label }, index) => (
              <span className={`is-endpoint-${index + 1}`} key={label}>
                <Icon size={18} weight="duotone" />
                <b>{label}</b>
              </span>
            ))}
          </div>
        </div>
        <div className="cloud-gateway-flow">
          <span className="cloud-gateway-route is-inbound">
            <ArrowDown size={18} weight="bold" />
          </span>
          <div className="cloud-gateway-core">
            <Broadcast size={28} weight="duotone" />
            <span>
              <strong>A3S Gateway</strong>
              <small>
                {language === 'zh'
                  ? '统一接入、策略执行与智能路由'
                  : 'Unified ingress, policy enforcement, and intelligent routing'}
              </small>
            </span>
          </div>
          <span className="cloud-gateway-route is-outbound">
            <ArrowDown size={18} weight="bold" />
          </span>
        </div>
        <div className="cloud-gateway-endpoints is-destination">
          <small>{language === 'zh' ? '服务目标' : 'SERVICE TARGETS'}</small>
          <div>
            {destinations.map(({ icon: Icon, label }, index) => (
              <span className={`is-endpoint-${index + 1}`} key={label}>
                <Icon size={18} weight="duotone" />
                <b>{label}</b>
              </span>
            ))}
          </div>
        </div>
        <div className="cloud-gateway-policies">
          {policies.map((policy, index) => (
            <span className={`is-tone-${(index % 3) + 1}`} key={policy}>
              {policy}
            </span>
          ))}
        </div>
      </div>
    </figure>
  );
}

export function ProductIcon({ id }: { id: ProductId }) {
  if (id === 'workflow')
    return <FlowArrow aria-hidden="true" weight="duotone" />;
  if (id === 'agent-factory')
    return <Factory aria-hidden="true" weight="duotone" />;
  return <Broadcast aria-hidden="true" weight="duotone" />;
}

export function HarnessIcon() {
  return <Code aria-hidden="true" weight="duotone" />;
}
