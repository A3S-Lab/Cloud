import { withBase } from '@rspress/core/runtime';
import {
  ArrowRight,
  Browser,
  Broadcast,
  Code,
  Cpu,
  Cube,
  Database,
  Factory,
  FlowArrow,
  GitBranch,
  Key,
  Lightning,
  ShieldCheck,
  Stack,
  UsersThree,
  type IconProps,
} from '@phosphor-icons/react';
import type { ComponentType } from 'react';
import type { HomeLanguage, LocalizedText } from '../data/product';

type ArchitectureItem = {
  title: string;
  description: LocalizedText;
  icon: ComponentType<IconProps>;
  emphasis?: boolean;
};

const products: ArchitectureItem[] = [
  {
    title: 'Workflow',
    description: {
      zh: '认知与本体工程驱动的自主编排',
      en: 'Cognitive, ontology-driven orchestration',
    },
    icon: FlowArrow,
    emphasis: true,
  },
  {
    title: 'Agent Factory',
    description: {
      zh: '异构智能体资产生产与交付',
      en: 'Heterogeneous Agent production and delivery',
    },
    icon: Factory,
    emphasis: true,
  },
  {
    title: 'Security Operations',
    description: {
      zh: '统一安全监控与响应',
      en: 'Unified security monitoring and response',
    },
    icon: ShieldCheck,
    emphasis: true,
  },
];

const client: ArchitectureItem[] = [
  {
    title: 'A3S Web',
    description: {
      zh: '面向三大产品的统一可视化客户端、工作空间与证据入口',
      en: 'Unified visual client, workspace, and evidence surface for all three products',
    },
    icon: Browser,
    emphasis: true,
  },
];

const platformServices: ArchitectureItem[] = [
  {
    title: 'A3S Workflow Engine',
    description: {
      zh: '本体语义、目标分解与自主编排',
      en: 'Ontology semantics, goal decomposition, and autonomous orchestration',
    },
    icon: FlowArrow,
  },
  {
    title: 'Operations + A3S Flow',
    description: {
      zh: '统一操作状态、重试与生命周期收敛',
      en: 'Unified operation state, retries, and lifecycle convergence',
    },
    icon: Stack,
  },
  {
    title: 'A3S Code Harness',
    description: {
      zh: '唯一 Agent 运行所有者与会话证据边界',
      en: 'Sole Agent run owner and session evidence boundary',
    },
    icon: Code,
    emphasis: true,
  },
  {
    title: 'A3S Sentry + AnySentry',
    description: {
      zh: '安全信号、策略判断与响应处置',
      en: 'Security signals, policy decisions, and response actions',
    },
    icon: ShieldCheck,
  },
];

const infrastructure: ArchitectureItem[] = [
  {
    title: 'A3S Gateway',
    description: {
      zh: '安全接入、实时流量与路由治理',
      en: 'Secure ingress, live traffic, and route governance',
    },
    icon: Broadcast,
  },
  {
    title: 'A3S Runtime',
    description: {
      zh: 'Task 与 Service 的统一生命周期',
      en: 'Unified Task and Service lifecycle',
    },
    icon: Cpu,
  },
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
    title: 'Code Hosting',
    description: {
      zh: '租户隔离的 Git 仓库与不可变源码版本',
      en: 'Tenant-scoped Git repositories and immutable source revisions',
    },
    icon: GitBranch,
  },
  {
    title: 'Hardware Scheduling',
    description: {
      zh: 'Workloads 与 Fleet 统一管理 CPU、GPU、放置和资源声明',
      en: 'Workloads and Fleet own CPU, GPU, placement, and resource claims',
    },
    icon: Stack,
  },
  {
    title: 'Data + Artifacts',
    description: {
      zh: 'PostgreSQL、A3S ORM、对象存储与 OCI 制品',
      en: 'PostgreSQL, A3S ORM, object storage, and OCI artifacts',
    },
    icon: Database,
  },
  {
    title: 'Identity + Evidence',
    description: {
      zh: 'A3S ACL、mTLS、日志、回执与审计证据',
      en: 'A3S ACL, mTLS, logs, receipts, and audit evidence',
    },
    icon: Key,
  },
];

const localize = (copy: LocalizedText, language: HomeLanguage) =>
  copy[language];

export function PlatformArchitecture({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <section className="cloud-platform-architecture" id="architecture">
      <header className="cloud-architecture-heading" data-reveal>
        <div>
          <span>{zh ? '模块架构' : 'MODULE ARCHITECTURE'}</span>
          <h2>
            {zh
              ? '一张图看清 A3S OS 如何运行'
              : 'See how A3S OS operates in one map'}
          </h2>
          <p>
            {zh
              ? 'A3S Web 承载统一交互，三大产品共享平台服务与基础设施。每个模块只保留一个明确职责，不重复建设调度器、Runtime 或第二套 Agent Harness。'
              : 'A3S Web provides one interaction surface while the three products share platform services and infrastructure. Every module keeps one clear responsibility without duplicate schedulers, runtimes, or Agent harnesses.'}
          </p>
        </div>
        <a href={withBase('/architecture/')}>
          {zh ? '打开交互式架构' : 'Open interactive architecture'}
          <ArrowRight aria-hidden="true" weight="bold" />
        </a>
      </header>

      <div className="cloud-platform-map" data-reveal>
        <header>
          <div>
            <span className="cloud-platform-mark">A3</span>
            <div>
              <strong>A3S OS</strong>
              <small>
                {zh ? '企业级 AI 操作系统' : 'ENTERPRISE AI OPERATING SYSTEM'}
              </small>
            </div>
          </div>
          <span>
            {zh
              ? '从产品意图到可验证执行'
              : 'FROM PRODUCT INTENT TO VERIFIED EXECUTION'}
          </span>
        </header>

        <ArchitectureLayer
          items={client}
          language={language}
          label={zh ? '客户端层' : 'CLIENT'}
          tone="client"
        />
        <LayerConnector
          label={zh ? '统一交互与证据视图' : 'UNIFIED INTERACTION AND EVIDENCE'}
        />
        <ArchitectureLayer
          items={products}
          language={language}
          label={zh ? '三大产品层' : 'THREE PRODUCTS'}
          tone="products"
        />
        <LayerConnector
          label={
            zh
              ? '产品意图与统一运行合约'
              : 'PRODUCT INTENT AND UNIFIED RUNTIME CONTRACT'
          }
        />
        <ArchitectureLayer
          items={platformServices}
          language={language}
          label={zh ? '平台服务层' : 'PLATFORM SERVICES'}
          tone="control"
        />
        <LayerConnector
          label={
            zh
              ? '状态、命令、资源与安全信号'
              : 'STATE, COMMANDS, RESOURCES, AND SECURITY SIGNALS'
          }
        />
        <ArchitectureLayer
          items={infrastructure}
          language={language}
          label={zh ? '基础设施层' : 'INFRASTRUCTURE'}
          tone="foundation"
        />

        <footer>
          <UsersThree aria-hidden="true" size={20} weight="duotone" />
          <p>
            {zh
              ? 'A3S OS 负责产品意图、权限与收敛；A3S Flow 负责持久任务；A3S Runtime 与 Box 负责执行；Workloads 与 Fleet 负责硬件资源调度；A3S Code Harness 只负责 Agent 运行。'
              : 'A3S OS owns product intent, authorization, and convergence. A3S Flow owns durable work. A3S Runtime and Box own execution. Workloads and Fleet own hardware scheduling. A3S Code Harness owns Agent runs only.'}
          </p>
        </footer>
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
        {items.map(({ description, emphasis, icon: Icon, title }) => (
          <article className={emphasis ? 'is-emphasis' : undefined} key={title}>
            <span>
              <Icon aria-hidden="true" size={23} weight="duotone" />
            </span>
            <div>
              <strong>{title}</strong>
              <small>{localize(description, language)}</small>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function LayerConnector({ label }: { label: string }) {
  return (
    <div className="cloud-layer-connector" aria-hidden="true">
      <i />
      <span>{label}</span>
      <i />
    </div>
  );
}
