import {
  ArrowRight,
  Boxes,
  Cloud,
  Code2,
  GitBranch,
  Layers3,
  Monitor,
  Network,
  PanelsTopLeft,
  Search,
  ShieldCheck,
  Workflow,
  type LucideIcon,
} from 'lucide-react';
import { useEffect } from 'react';
import { ArchitectureOverview } from '../architecture/architecture-diagram';
import { LanguageSwitcher, useI18n } from '../../lib/i18n';
import { ProductPillars } from './product-pillars';

const AUTHORITY_PATH: ReadonlyArray<{
  label: string;
  detail: string;
  icon: LucideIcon;
}> = [
  { label: 'A3S OS', detail: 'Intent, identity, and policy', icon: Cloud },
  { label: 'Operations + A3S Flow', detail: 'Durable orchestration', icon: Workflow },
  { label: 'Outbound-only Node Agent', detail: 'Typed command delivery', icon: Network },
  { label: 'A3S Runtime + Box', detail: 'Execution and isolation', icon: Boxes },
  { label: 'A3S Code Harness', detail: 'Sole Agent run owner', icon: Code2 },
];

const WEB_CAPABILITIES: ReadonlyArray<{
  title: { zh: string; en: string };
  detail: { zh: string; en: string };
  icon: LucideIcon;
}> = [
  {
    title: { zh: '统一工作空间', en: 'Unified workspace' },
    detail: { zh: '环境、Operation、Workload 与路由状态集中呈现。', en: 'See environments, Operations, Workloads, and routes together.' },
    icon: PanelsTopLeft,
  },
  {
    title: { zh: 'Workload 运行', en: 'Workload operations' },
    detail: { zh: '查看收敛、版本、实时日志、停止、取消与回滚。', en: 'Inspect convergence, versions, live logs, stop, cancel, and rollback.' },
    icon: Boxes,
  },
  {
    title: { zh: 'Agent 工作台', en: 'Agent workspace' },
    detail: { zh: '管理会话、审批、检查点和唯一 Harness 证据。', en: 'Manage sessions, approvals, checkpoints, and sole-Harness evidence.' },
    icon: Code2,
  },
  {
    title: { zh: '交付与证据', en: 'Delivery and evidence' },
    detail: { zh: '追踪源码版本、构建、制品、SBOM 与签名证据。', en: 'Trace source revisions, builds, artifacts, SBOMs, and signed evidence.' },
    icon: GitBranch,
  },
  {
    title: { zh: 'Edge 与安全', en: 'Edge and security' },
    detail: { zh: '统一查看域名、TLS、Gateway 和安全运行信号。', en: 'Inspect domains, TLS, Gateway state, and security signals.' },
    icon: ShieldCheck,
  },
  {
    title: { zh: '搜索与架构', en: 'Search and architecture' },
    detail: { zh: '跨模块定位资源，查看并导出完整模块架构。', en: 'Find resources across modules and inspect or export the module map.' },
    icon: Search,
  },
];

export function ProjectHome() {
  const { language, t } = useI18n();
  const zh = language === 'zh-CN';
  const navItems = [
    { href: '#products', label: zh ? '三大产品' : 'Products' },
    { href: '#web-client', label: 'A3S Web' },
    { href: '#architecture', label: zh ? '模块架构' : 'Architecture' },
  ];

  useEffect(() => {
    const targetId = window.location.hash.slice(1);
    if (!targetId) return;
    const frame = window.requestAnimationFrame(() => document.getElementById(targetId)?.scrollIntoView());
    return () => window.cancelAnimationFrame(frame);
  }, []);

  return (
    <main id='top' className='product-home'>
      <header className='product-topbar'>
        <a className='brand-lockup' href='#top' aria-label={zh ? 'A3S OS 首页' : 'A3S OS home'}>
          <span className='brand-mark' aria-hidden='true'>
            A3
          </span>
          <span>A3S OS</span>
        </a>
        <nav aria-label={zh ? '首页导航' : 'Homepage navigation'}>
          {navItems.map((item) => (
            <a href={item.href} key={item.href}>
              {item.label}
            </a>
          ))}
        </nav>
        <div className='product-topbar-actions'>
          <LanguageSwitcher />
        </div>
      </header>

      <section className='product-hero' aria-labelledby='home-title'>
        <div className='product-hero-story'>
          <h1 id='home-title'>
            <span>A3S OS</span>
            {zh ? '企业级 AI 操作系统' : 'The enterprise AI operating system'}
          </h1>
          <p>
            {zh
              ? '以认知驱动的本体工程、自主进化和大规模高可用服务平台，承载自主工作流编排、异构智能体工厂与安全监控中台。'
              : 'Cognitive ontology engineering, governed autonomous evolution, and a highly available service platform power autonomous workflow orchestration, a heterogeneous Agent Factory, and security operations.'}
          </p>
          <div className='product-hero-actions'>
            <a className='hero-primary-action' href='#products'>
              {zh ? '了解三大产品' : 'Explore products'}
              <ArrowRight size={17} aria-hidden='true' />
            </a>
            <a className='hero-secondary-action' href='#architecture'>
              <Layers3 size={17} aria-hidden='true' />
              {zh ? '查看模块架构' : 'View architecture'}
            </a>
          </div>
        </div>

        <div className='product-hero-system'>
          <section className='signin-authority-card' aria-labelledby='authority-path-title'>
            <div className='signin-authority-heading'>
              <div>
                <h2 id='authority-path-title'>{zh ? '一套系统运行链路' : 'One system execution path'}</h2>
                <p>{zh ? 'A3S OS 编排，既有 A3S 模块执行。' : 'A3S OS orchestrates. Existing A3S authorities execute.'}</p>
              </div>
              <span>{zh ? '唯一执行链路' : 'Sole execution path'}</span>
            </div>
            <ol className='signin-authority-path'>
              {AUTHORITY_PATH.map(({ label, detail, icon: Icon }) => (
                <li key={label}>
                  <span className='authority-icon' aria-hidden='true'>
                    <Icon size={19} />
                  </span>
                  <span>
                    <strong>{t(label)}</strong>
                    <small>{t(detail)}</small>
                  </span>
                </li>
              ))}
            </ol>
          </section>
        </div>
      </section>

      <section className='product-facts' aria-label={zh ? '项目事实' : 'Project facts'}>
        <Fact icon={PanelsTopLeft} value={3} label={zh ? '应用层产品' : 'Application products'} />
        <Fact icon={Monitor} value={1} label='A3S Web' />
        <Fact icon={Code2} value={1} label={zh ? '唯一 Code Harness' : 'Sole Code Harness'} />
        <Fact icon={Workflow} value={1} label={zh ? '共享运行底座' : 'Shared runtime foundation'} />
      </section>

      <ProductPillars />
      <WebClientOverview language={zh ? 'zh' : 'en'} />
      <ArchitectureOverview />

      <footer className='product-footer'>
        <div className='brand-lockup'>
          <span className='brand-mark' aria-hidden='true'>
            A3
          </span>
          <span>A3S OS</span>
        </div>
        <p>
          {zh
            ? 'A3S Web 复用同一命令、授权与证据合约，不建立第二套控制机制。'
            : 'A3S Web reuses the same command, authorization, and evidence contracts without a second control mechanism.'}
        </p>
        <a href='#top'>{zh ? '返回顶部' : 'Back to top'}</a>
      </footer>
    </main>
  );
}

function WebClientOverview({ language }: { language: 'zh' | 'en' }) {
  const zh = language === 'zh';

  return (
    <section id='web-client' className='home-section web-client-overview' aria-labelledby='web-client-title'>
      <header className='home-section-heading'>
        <span>04 · A3S WEB</span>
        <h2 id='web-client-title'>{zh ? '一个客户端，贯通三大产品的每一次工作' : 'One client for every action across all three products'}</h2>
        <p>
          {zh
            ? 'A3S Web 是 A3S OS 的统一客户端。它将复杂运行状态翻译为可搜索、可操作、可审计的企业工作空间。'
            : 'A3S Web is the unified client for A3S OS, translating complex runtime state into a searchable, operable, and auditable enterprise workspace.'}
        </p>
      </header>
      <div className='web-client-overview-layout'>
        <div className='web-client-capability-grid'>
          {WEB_CAPABILITIES.map(({ detail, icon: Icon, title }) => (
            <article key={title.en}>
              <span aria-hidden='true'><Icon size={19} /></span>
              <div>
                <strong>{title[language]}</strong>
                <small>{detail[language]}</small>
              </div>
            </article>
          ))}
        </div>
        <figure className='web-client-thread'>
          <figcaption>
            <Monitor size={18} aria-hidden='true' />
            <strong>A3S Web</strong>
            <small>{zh ? '统一客户端视图' : 'UNIFIED CLIENT VIEW'}</small>
          </figcaption>
          <ol>
            {[zh ? '发现资源' : 'Discover', zh ? '执行操作' : 'Operate', zh ? '跟踪状态' : 'Trace', zh ? '验证证据' : 'Verify'].map((label, index) => (
              <li key={label}><b>0{index + 1}</b><span>{label}</span></li>
            ))}
          </ol>
          <p>{zh ? '同一身份 · 同一命令 · 同一证据链' : 'ONE IDENTITY · ONE COMMAND PATH · ONE EVIDENCE CHAIN'}</p>
        </figure>
      </div>
    </section>
  );
}

function Fact({ icon: Icon, value, label }: { icon: LucideIcon; value: number; label: string }) {
  return (
    <article>
      <span aria-hidden='true'>
        <Icon size={20} />
      </span>
      <strong>{value}</strong>
      <small>{label}</small>
    </article>
  );
}
