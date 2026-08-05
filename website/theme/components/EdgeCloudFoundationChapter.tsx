import {
  ArrowLeft,
  ArrowRight,
  ArrowsLeftRight,
  Brain,
  Broadcast,
  Browser,
  ChartLineUp,
  Check,
  Code,
  Cube,
  Database,
  DesktopTower,
  FlowArrow,
  Key,
  Lightning,
  Network,
  Pulse,
  ShieldCheck,
  Stack,
  TerminalWindow,
  type IconProps,
} from '@phosphor-icons/react';
import type { ComponentType } from 'react';
import type { HomeLanguage, LocalizedText } from '../data/product';

type FoundationCapability = {
  title: LocalizedText;
  icon: ComponentType<IconProps>;
};

const edgeCapabilities: FoundationCapability[] = [
  {
    title: { zh: '敏感数据本地驻留', en: 'Sensitive data stays local' },
    icon: Database,
  },
  {
    title: { zh: '本地工具与命令', en: 'Local tools and commands' },
    icon: Code,
  },
  {
    title: { zh: '低延迟业务交互', en: 'Low-latency interaction' },
    icon: Lightning,
  },
  {
    title: { zh: 'A3S Box 安全隔离', en: 'A3S Box isolation' },
    icon: Cube,
  },
];

const cloudCapabilities: FoundationCapability[] = [
  {
    title: { zh: '统一身份与会话风控', en: 'Identity and session controls' },
    icon: Key,
  },
  {
    title: { zh: '高并发批量任务', en: 'Concurrent batch workloads' },
    icon: Stack,
  },
  {
    title: {
      zh: '统一审批、调度与配额',
      en: 'Approval, scheduling, and quota',
    },
    icon: FlowArrow,
  },
  {
    title: { zh: '弹性隔离执行', en: 'Elastic isolated execution' },
    icon: ShieldCheck,
  },
];

const strengths = [
  {
    title: { zh: '数据不出域', en: 'Data residency' },
    body: {
      zh: '敏感数据就近处理，遵循企业边界',
      en: 'Process sensitive data near its source',
    },
    icon: Database,
  },
  {
    title: { zh: '端云协作', en: 'Cloud-edge collaboration' },
    body: {
      zh: '端侧节点与云端任务灵活切换',
      en: 'Coordinate edge nodes and cloud workloads',
    },
    icon: ArrowsLeftRight,
  },
  {
    title: { zh: '高效稳定', en: 'Efficient and resilient' },
    body: {
      zh: '本地低延迟，云端弹性承载',
      en: 'Local responsiveness with cloud elasticity',
    },
    icon: ChartLineUp,
  },
  {
    title: { zh: '统一管控', en: 'Unified governance' },
    body: {
      zh: '策略、证据与运行状态统一收敛',
      en: 'Converge policy, evidence, and runtime state',
    },
    icon: ShieldCheck,
  },
] as const;

const localize = (copy: LocalizedText, language: HomeLanguage) =>
  copy[language];

export function EdgeCloudFoundationChapter({
  language,
}: {
  language: HomeLanguage;
}) {
  const zh = language === 'zh';

  return (
    <section className="cloud-edge-foundation" id="edge-cloud-foundation">
      <header className="cloud-edge-foundation-heading" data-reveal>
        <h2>
          {zh
            ? '端云一体 Agent 安全底座'
            : 'A secure cloud-edge foundation for Agents'}
        </h2>
        <h3>
          {zh
            ? '端云协同调度，让 Agent 执行兼顾安全与效率'
            : 'Coordinate cloud and edge execution without trading safety for efficiency'}
        </h3>
        <p>
          {zh
            ? '端侧负责低延迟交互与敏感数据处理，云端承载高并发任务和统一调度。身份、策略和证据贯穿两端，任务可按场景、风险与负载选择执行位置。'
            : 'Connect edge nodes with elastic cloud execution. Keep sensitive data local, move concurrent work to cloud capacity, and govern both through one policy, identity, and evidence boundary.'}
        </p>
      </header>

      <div className="cloud-edge-foundation-map cloud-motion-scene" data-reveal>
        <ExecutionSide
          capabilities={edgeCapabilities}
          footer={
            zh
              ? '员工终端 / 开发机 / 边缘节点'
              : 'DESKTOPS / DEVICES / EDGE NODES'
          }
          language={language}
          subtitle={
            zh
              ? '贴近业务现场，数据驻留本地'
              : 'Close to work, with local data residency'
          }
          title={zh ? '端侧执行' : 'EDGE EXECUTION'}
          tone="edge"
        />

        <div className="cloud-edge-decision" aria-hidden="true">
          <span className="cloud-edge-arrow is-left">
            <ArrowLeft size={19} weight="bold" />
          </span>
          <div className="cloud-edge-decision-orbit">
            <span className="cloud-edge-decision-ring is-outer" />
            <span className="cloud-edge-decision-ring is-middle" />
            <span className="cloud-edge-decision-ring is-inner" />
            <span className="cloud-edge-decision-mark">
              <Brain size={28} weight="bold" />
            </span>
            <strong>{zh ? '智能决策' : 'POLICY DECISION'}</strong>
            <small>
              <b>{zh ? '按场景 · 按风险 · 按负载' : 'CONTEXT · RISK · LOAD'}</b>
              <span>
                {zh
                  ? '自动选择执行路径'
                  : 'Choose the execution path automatically'}
              </span>
            </small>
          </div>
          <span className="cloud-edge-arrow is-right">
            <ArrowRight size={19} weight="bold" />
          </span>
          <div className="cloud-edge-decision-steps">
            <span>{zh ? '策略中心' : 'Policy'}</span>
            <span>{zh ? '任务路由' : 'Routing'}</span>
            <span>{zh ? '动态评估' : 'Evaluation'}</span>
          </div>
        </div>

        <ExecutionSide
          capabilities={cloudCapabilities}
          footer={
            zh
              ? '企业数据中心 / 私有云 / 云边集群'
              : 'DATA CENTER / PRIVATE CLOUD / CLOUD-EDGE FLEET'
          }
          language={language}
          subtitle={
            zh
              ? '弹性承载任务，全链路审计追踪'
              : 'Elastic workloads with end-to-end auditability'
          }
          title={zh ? '云侧执行' : 'CLOUD EXECUTION'}
          tone="cloud"
        />
      </div>

      <div className="cloud-edge-strengths" data-reveal>
        {strengths.map(({ body, icon: Icon, title }) => (
          <article key={title.en}>
            <span>
              <Icon aria-hidden="true" size={22} weight="duotone" />
            </span>
            <div>
              <strong>{localize(title, language)}</strong>
              <small>{localize(body, language)}</small>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function ExecutionSide({
  capabilities,
  footer,
  language,
  subtitle,
  title,
  tone,
}: {
  capabilities: FoundationCapability[];
  footer: string;
  language: HomeLanguage;
  subtitle: string;
  title: string;
  tone: 'cloud' | 'edge';
}) {
  return (
    <article className={`cloud-execution-side is-${tone}`}>
      <header>
        <span>{title}</span>
        <strong>{subtitle}</strong>
      </header>
      <ul>
        {capabilities.map(({ icon: Icon, title: capabilityTitle }) => (
          <li key={capabilityTitle.en}>
            <Icon aria-hidden="true" size={17} weight="duotone" />
            {localize(capabilityTitle, language)}
          </li>
        ))}
      </ul>
      <ExecutionPreview language={language} tone={tone} />
      <footer>
        <Network aria-hidden="true" size={18} weight="duotone" />
        {footer}
      </footer>
    </article>
  );
}

function ExecutionPreview({
  language,
  tone,
}: {
  language: HomeLanguage;
  tone: 'cloud' | 'edge';
}) {
  const zh = language === 'zh';

  if (tone === 'edge') {
    return (
      <figure
        className="cloud-execution-asset cloud-execution-ui is-edge"
        aria-hidden="true"
      >
        <header>
          <span>
            <Browser size={15} weight="duotone" />
            <strong>A3S Work</strong>
          </span>
          <small>{zh ? '本地安全会话' : 'LOCAL SECURE SESSION'}</small>
          <em>
            <i /> LIVE
          </em>
        </header>
        <div className="cloud-edge-mini-stage">
          <article className="cloud-edge-mini-task">
            <span>
              <Database size={17} weight="duotone" />
            </span>
            <div>
              <small>{zh ? '任务' : 'TASK'}</small>
              <strong>{zh ? '核验客户资料' : 'Verify customer profile'}</strong>
            </div>
            <em>{zh ? '敏感' : 'SENSITIVE'}</em>
          </article>
          <span className="cloud-edge-mini-tunnel">
            <ShieldCheck size={14} weight="duotone" />
            <b>mTLS</b>
            <i />
          </span>
          <article className="cloud-edge-mini-box">
            <span>
              <Cube size={18} weight="duotone" />
            </span>
            <div>
              <strong>A3S Box bx-07</strong>
              <small>{zh ? '本地隔离运行' : 'Local isolated run'}</small>
            </div>
            <Check size={14} weight="bold" />
          </article>
          <section className="cloud-edge-mini-residency">
            <DesktopTower size={15} weight="duotone" />
            <span>
              <strong>
                {zh ? '数据保留在设备内' : 'Data remains on device'}
              </strong>
              <small>customer.db · local only</small>
            </span>
            <em>12 ms</em>
          </section>
          <i className="cloud-edge-mini-packet" />
        </div>
        <footer>
          <Pulse size={14} weight="duotone" />
          <span>{zh ? '仅回流脱敏证据' : 'Redacted evidence only'}</span>
          <code>trace edge_01J8</code>
        </footer>
      </figure>
    );
  }

  return (
    <figure
      className="cloud-execution-asset cloud-execution-ui is-cloud"
      aria-hidden="true"
    >
      <header>
        <span>
          <Broadcast size={15} weight="duotone" />
          <strong>A3S Gateway</strong>
        </span>
        <small>{zh ? '云侧弹性执行' : 'CLOUD ELASTIC RUN'}</small>
        <em>
          <i /> LIVE
        </em>
      </header>
      <div className="cloud-cloud-mini-stage">
        <article className="cloud-cloud-mini-ingress">
          <TerminalWindow size={17} weight="duotone" />
          <span>
            <small>{zh ? '批量任务' : 'BATCH JOB'}</small>
            <strong>evaluation × 128</strong>
          </span>
        </article>
        <section className="cloud-cloud-mini-policy">
          <Key size={15} weight="duotone" />
          <span>
            <b>{zh ? '身份与配额通过' : 'Identity & quota passed'}</b>
            <small>tenant_acme · allow</small>
          </span>
          <Check size={13} weight="bold" />
        </section>
        <div className="cloud-cloud-mini-runtime">
          <header>
            <Stack size={16} weight="duotone" />
            <strong>A3S Runtime</strong>
            <small>3 / 3</small>
          </header>
          <div>
            <span>
              <i /> run-01
            </span>
            <span>
              <i /> run-02
            </span>
            <span>
              <i /> run-03
            </span>
          </div>
        </div>
        <section className="cloud-cloud-mini-evidence">
          <Pulse size={15} weight="duotone" />
          <span>
            <strong>AnySentry</strong>
            <small>{zh ? '全链路证据回流' : 'Full-path evidence return'}</small>
          </span>
          <em>100%</em>
        </section>
        <i className="cloud-cloud-mini-packet" />
      </div>
      <footer>
        <ShieldCheck size={14} weight="duotone" />
        <span>{zh ? '云侧工作负载已隔离' : 'Cloud workload isolated'}</span>
        <code>trace cloud_01J9</code>
      </footer>
    </figure>
  );
}
