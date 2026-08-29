import {
  ArrowRight,
  Boxes,
  Cloud,
  Code2,
  Layers3,
  Network,
  PanelsTopLeft,
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
  { label: 'Agent execution providers', detail: 'One provider-neutral contract', icon: Code2 },
];

export function ProjectHome() {
  const { language, t } = useI18n();
  const zh = language === 'zh-CN';
  const navItems = [
    { href: '#products', label: zh ? '三大产品' : 'Products' },
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
              ? '以认知驱动的本体工程、治理式自主进化和大规模高可用服务平台，承载统一网关、自主工作流编排与异构智能体工厂；安全监控仍贯穿整个网关治理链路。'
              : 'Cognitive ontology engineering, governed autonomous evolution, and a highly available service platform power Unified Gateway, autonomous workflow orchestration, and a heterogeneous Agent Factory, with security operations spanning the gateway governance path.'}
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
                <p>
                  {zh
                    ? 'A3S OS 编排，既有 A3S 模块执行。'
                    : 'A3S OS orchestrates. Existing A3S authorities execute.'}
                </p>
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
        <Fact icon={Code2} value={1} label={zh ? '统一 Agent 合约' : 'One Agent contract'} />
        <Fact icon={Workflow} value={1} label={zh ? '共享运行底座' : 'Shared runtime foundation'} />
      </section>

      <ProductPillars />
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
            ? 'Cloud 控制台复用同一命令、授权与证据合约，不建立第二套控制机制。'
            : 'The Cloud console reuses the same command, authorization, and evidence contracts without a second control mechanism.'}
        </p>
        <a href='#top'>{zh ? '返回顶部' : 'Back to top'}</a>
      </footer>
    </main>
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
