import { Boxes, CheckCircle2, Factory, GitBranch, Network, Workflow, type LucideIcon } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import { statusBadgeState } from '../../lib/status-badge';
import { CAPABILITY_STATES, localize, PRODUCT_PILLARS, type ProductPillar } from './project-catalog';

const PILLAR_ICONS: Record<ProductPillar['id'], LucideIcon> = {
  'unified-gateway': Network,
  workflow: Workflow,
  'agent-factory': Factory,
};

const FOUNDATION_MODULES = [
  'A3S OS',
  'A3S Workflow',
  'A3S Code',
  'A3S Gateway / Sentry',
  'A3S Runtime / Box',
  'PostgreSQL / OCI / ACL / mTLS',
];

export function ProductPillars() {
  const { language } = useI18n();
  const zh = language === 'zh-CN';

  return (
    <section id='products' className='home-section product-pillars' aria-labelledby='product-pillars-title'>
      <header className='home-section-heading'>
        <h2 id='product-pillars-title'>
          {zh ? '三大产品，把 AI 变成可运营系统' : 'Three products turn AI into an operable system'}
        </h2>
        <p>
          {zh
            ? '统一网关负责可信接入与安全治理，自主工作流编排负责业务意图，异构智能体工厂负责规模化交付。A3S OS 将三者收敛到同一套平台。'
            : 'Unified Gateway owns trusted access and security governance, autonomous workflow orchestration owns business intent, and the heterogeneous Agent Factory owns repeatable delivery. A3S OS unifies all three.'}
        </p>
      </header>

      <div className='product-pillar-grid'>
        {PRODUCT_PILLARS.map((pillar) => (
          <ProductPillarCard pillar={pillar} key={pillar.id} />
        ))}
      </div>

      <section className='product-foundation' aria-labelledby='product-foundation-title'>
        <div>
          <Boxes size={22} aria-hidden='true' />
          <h3 id='product-foundation-title'>
            {zh ? '同一套企业级运行底座' : 'One enterprise runtime foundation'}
          </h3>
          <p>
            {zh
              ? '三大产品不各建控制器、调度器、Runtime、Agent 生命周期或证据库，而是共享 A3S OS 权威状态、执行链路与集成事实。'
              : 'The three products do not build separate controllers, schedulers, runtimes, Agent lifecycles, or evidence stores. They share A3S OS authority, execution paths, and integration facts.'}
          </p>
        </div>
        <ul>
          {FOUNDATION_MODULES.map((module) => (
            <li key={module}>{module}</li>
          ))}
        </ul>
      </section>
    </section>
  );
}

function ProductPillarCard({ pillar }: { pillar: ProductPillar }) {
  const { language } = useI18n();
  const zh = language === 'zh-CN';
  const Icon = PILLAR_ICONS[pillar.id];

  return (
    <article className={`card product-pillar product-pillar-${pillar.id}`}>
      <header>
        <span className='product-pillar-icon' aria-hidden='true'>
          <Icon size={22} />
        </span>
        <div>
          <small>{zh ? '技术底座' : 'Built on'}</small>
          <strong>{pillar.basedOn}</strong>
        </div>
        <span
          className={`status-badge capability-state capability-state-${pillar.state}`}
          data-state={statusBadgeState(pillar.state)}
          data-size='sm'
        >
          {localize(CAPABILITY_STATES[pillar.state].label, language)}
        </span>
      </header>
      <h3>{localize(pillar.title, language)}</h3>
      <h4>{localize(pillar.promise, language)}</h4>
      <p>{localize(pillar.description, language)}</p>
      <ul className='product-pillar-capabilities'>
        {pillar.capabilities.map((capability) => (
          <li key={capability.en}>
            <CheckCircle2 size={15} aria-hidden='true' />
            {localize(capability, language)}
          </li>
        ))}
      </ul>
      <footer>
        <GitBranch size={15} aria-hidden='true' />
        <span>{zh ? '关联路线图' : 'Roadmap gates'}</span>
        {pillar.gateCodes.map((code) => (
          <code key={code}>{code}</code>
        ))}
      </footer>
    </article>
  );
}
