import { Code, Factory, FlowArrow, ShieldCheck } from '@phosphor-icons/react';
import type { HomeLanguage, ProductId } from '../data/product';

type ChartProps = {
  language: HomeLanguage;
};

const chartCopy = {
  hero: {
    title: {
      zh: '三大产品，一套企业 AI 操作系统',
      en: 'Three products, one enterprise AI operating system',
    },
    subtitle: {
      zh: '应用产品共享控制、执行与证据边界',
      en: 'Application products share control, execution, and evidence boundaries',
    },
  },
  workflow: {
    title: {
      zh: '从业务本体到执行协同',
      en: 'From business ontology to coordinated execution',
    },
    subtitle: {
      zh: '每一条线代表一类真实编排关系，不用脚本假装业务语义',
      en: 'Each line represents a real orchestration relationship, without reducing business semantics to scripts',
    },
  },
  factory: {
    title: {
      zh: '一个 Agent 版本如何成为可运行资产',
      en: 'How one Agent version becomes a runnable asset',
    },
    subtitle: {
      zh: '装配输入固定为 Release，执行身份从发布一直延续到证据',
      en: 'Assembly inputs are pinned into a release, preserving execution identity through evidence',
    },
  },
  security: {
    title: {
      zh: '分散信号，收敛成一个处置闭环',
      en: 'Distributed signals converge into one response loop',
    },
    subtitle: {
      zh: '信号来源、策略判断和处置结果都保留可追踪关系',
      en: 'Signal sources, policy decisions, and response outcomes remain traceable',
    },
  },
} as const;

const pick = (copy: { zh: string; en: string }, language: HomeLanguage) =>
  copy[language];

export function ProductSystemChart({ language }: ChartProps) {
  const products = [
    {
      icon: FlowArrow,
      label: language === 'zh' ? '自主工作流编排' : 'Workflow',
      meta: 'A3S Workflow',
    },
    {
      icon: Factory,
      label: language === 'zh' ? '异构智能体工厂' : 'Agent Factory',
      meta: 'A3S Code',
    },
    {
      icon: ShieldCheck,
      label: language === 'zh' ? '安全监控中台' : 'Security Ops',
      meta: 'Gateway / Sentry',
    },
  ];

  return (
    <figure
      className="cloud-editorial-chart cloud-system-chart"
      aria-labelledby="system-chart-title"
    >
      <figcaption>
        <strong id="system-chart-title">
          {pick(chartCopy.hero.title, language)}
        </strong>
        <span>{pick(chartCopy.hero.subtitle, language)}</span>
      </figcaption>
      <div className="cloud-system-products" aria-hidden="true">
        {products.map(({ icon: Icon, label, meta }) => (
          <div key={label}>
            <Icon size={21} weight="duotone" />
            <strong>{label}</strong>
            <small>{meta}</small>
          </div>
        ))}
      </div>
      <div className="cloud-system-spine" aria-hidden="true">
        <i />
        <span>A3S OS</span>
        <i />
      </div>
      <div className="cloud-system-foundation" aria-hidden="true">
        <span>A3S Gateway</span>
        <span>A3S Runtime</span>
        <span>A3S Box</span>
        <span>A3S Power</span>
        <span>Code Hosting</span>
        <span>Hardware Scheduling</span>
      </div>
      <p>
        {language === 'zh'
          ? '产品关系图 · 共享运行底座 · 仓库架构'
          : 'PRODUCT MAP · SHARED RUNTIME · REPOSITORY ARCHITECTURE'}
      </p>
    </figure>
  );
}

export function ProductChart({ id, language }: ChartProps & { id: ProductId }) {
  if (id === 'workflow') return <WorkflowChart language={language} />;
  if (id === 'agent-factory') return <AgentFactoryChart language={language} />;
  return <SecurityChart language={language} />;
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
      className="cloud-editorial-chart cloud-workflow-chart"
      aria-labelledby="workflow-chart-title"
    >
      <figcaption>
        <strong id="workflow-chart-title">
          {pick(chartCopy.workflow.title, language)}
        </strong>
        <span>{pick(chartCopy.workflow.subtitle, language)}</span>
      </figcaption>
      <div className="cloud-chart-canvas" aria-hidden="true">
        <svg viewBox="0 0 720 330">
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
            {paths.map((path) => (
              <path d={path} key={path} />
            ))}
          </g>
          {ontology.map((label, index) => (
            <g
              className="cloud-chart-node"
              key={label}
              transform={`translate(118 ${76 + index * 60})`}
            >
              <circle r="5" />
              <text x="-14" y="4" textAnchor="end">
                {label}
              </text>
            </g>
          ))}
          {engine.map((label, index) => (
            <g
              className="cloud-chart-node is-core"
              key={label}
              transform={`translate(322 ${92 + index * 82})`}
            >
              <circle r="8" />
              <text x="14" y="4">
                {label}
              </text>
            </g>
          ))}
          {actors.map((label, index) => (
            <g
              className="cloud-chart-node"
              key={label}
              transform={`translate(602 ${76 + index * 60})`}
            >
              <circle r="5" />
              <text x="14" y="4">
                {label}
              </text>
            </g>
          ))}
        </svg>
      </div>
      <p>
        {language === 'zh'
          ? 'THREAD MAP · 本体关系投影 · A3S WORKFLOW'
          : 'THREAD MAP · ONTOLOGY PROJECTION · A3S WORKFLOW'}
      </p>
    </figure>
  );
}

function AgentFactoryChart({ language }: ChartProps) {
  const inputs =
    language === 'zh'
      ? ['Agent', 'Skills', 'MCP', '策略']
      : ['Agent', 'Skills', 'MCP', 'Policy'];
  const stages =
    language === 'zh'
      ? ['固定版本', '不可变 Release', 'Workload', '运行证据']
      : ['Pin', 'Immutable release', 'Workload', 'Evidence'];

  return (
    <figure
      className="cloud-editorial-chart cloud-factory-chart"
      aria-labelledby="factory-chart-title"
    >
      <figcaption>
        <strong id="factory-chart-title">
          {pick(chartCopy.factory.title, language)}
        </strong>
        <span>{pick(chartCopy.factory.subtitle, language)}</span>
      </figcaption>
      <div className="cloud-chart-canvas" aria-hidden="true">
        <svg viewBox="0 0 720 330">
          <g className="cloud-factory-inputs">
            {inputs.map((label, index) => (
              <g key={label} transform={`translate(82 ${70 + index * 58})`}>
                <circle r="4" />
                <text x="13" y="4">
                  {label}
                </text>
                <path
                  d={`M52 0 C138 0 138 ${90 - index * 58} 220 ${90 - index * 58}`}
                />
              </g>
            ))}
          </g>
          <g className="cloud-factory-lineage">
            <line x1="302" y1="64" x2="302" y2="284" />
            {stages.map((label, index) => (
              <g
                key={label}
                transform={`translate(${302 + index * 112} ${160 + (index % 2 === 0 ? -28 : 28)})`}
              >
                <circle
                  className={index === 1 ? 'is-release' : undefined}
                  r={index === 1 ? 11 : 6}
                />
                <text y={index % 2 === 0 ? -18 : 25} textAnchor="middle">
                  {label}
                </text>
              </g>
            ))}
            <path d="M302 132 C350 132 362 188 414 188 C470 188 474 132 526 132 C584 132 588 188 638 188" />
          </g>
          <g className="cloud-factory-harness">
            <rect x="423" y="230" width="198" height="48" rx="8" />
            <text x="522" y="250" textAnchor="middle">
              A3S CODE HARNESS
            </text>
            <text x="522" y="266" textAnchor="middle">
              {language === 'zh' ? '唯一执行所有者' : 'SOLE RUN OWNER'}
            </text>
            <path d="M526 138 L526 230" />
          </g>
        </svg>
      </div>
      <p>
        {language === 'zh'
          ? 'RELEASE LINEAGE · 固定版本资产 · A3S CODE'
          : 'RELEASE LINEAGE · PINNED ASSETS · A3S CODE'}
      </p>
    </figure>
  );
}

function SecurityChart({ language }: ChartProps) {
  const sources =
    language === 'zh'
      ? [
          'Gateway 流量',
          'Runtime / Box',
          'Fleet 命令',
          '租户身份',
          'AnySentry 告警',
        ]
      : [
          'Gateway traffic',
          'Runtime / Box',
          'Fleet commands',
          'Tenant identity',
          'AnySentry alerts',
        ];
  const outcomes =
    language === 'zh'
      ? ['策略判断', '隔离与恢复', '审计证据']
      : ['Policy decision', 'Isolation and recovery', 'Audit evidence'];

  return (
    <figure
      className="cloud-editorial-chart cloud-security-chart"
      aria-labelledby="security-chart-title"
    >
      <figcaption>
        <strong id="security-chart-title">
          {pick(chartCopy.security.title, language)}
        </strong>
        <span>{pick(chartCopy.security.subtitle, language)}</span>
      </figcaption>
      <div className="cloud-security-map" aria-hidden="true">
        <svg viewBox="0 0 720 330">
          <g className="cloud-security-rays">
            {[60, 110, 160, 210, 260].map((y, index) => {
              const destinationY = [88, 88, 165, 242, 242][index];
              return (
                <path
                  d={`M128 ${y} C240 ${y} 250 165 342 165 C438 165 458 ${destinationY} 598 ${destinationY}`}
                  key={y}
                />
              );
            })}
          </g>
          <circle className="cloud-security-orbit" cx="342" cy="165" r="88" />
          <circle
            className="cloud-security-orbit is-inner"
            cx="342"
            cy="165"
            r="60"
          />
        </svg>
        <div className="cloud-security-sources">
          {sources.map((source) => (
            <span key={source}>{source}</span>
          ))}
        </div>
        <div className="cloud-security-core">
          <ShieldCheck size={28} weight="duotone" />
          <strong>{language === 'zh' ? '安全监控中台' : 'Security Ops'}</strong>
          <small>Gateway · Sentry · AnySentry</small>
        </div>
        <div className="cloud-security-outcomes">
          {outcomes.map((outcome) => (
            <span key={outcome}>{outcome}</span>
          ))}
        </div>
      </div>
      <p>
        {language === 'zh'
          ? 'RADIAL CONVERGENCE · 运行信号图 · 安全证据'
          : 'RADIAL CONVERGENCE · RUNTIME SIGNALS · SECURITY EVIDENCE'}
      </p>
    </figure>
  );
}

export function ProductIcon({ id }: { id: ProductId }) {
  if (id === 'workflow')
    return <FlowArrow aria-hidden="true" weight="duotone" />;
  if (id === 'agent-factory')
    return <Factory aria-hidden="true" weight="duotone" />;
  return <ShieldCheck aria-hidden="true" weight="duotone" />;
}

export function HarnessIcon() {
  return <Code aria-hidden="true" weight="duotone" />;
}
