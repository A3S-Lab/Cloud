import { withBase } from '@rspress/core/runtime';
import {
  ArrowRight,
  ArrowsClockwise,
  Brain,
  CloudCheck,
  GithubLogo,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';
import { InteractionLayer } from './InteractionLayer';
import { PlatformArchitecture } from './PlatformArchitecture';
import { ProductChapter } from './ProductChapter';
import { ProductSystemChart } from './ProductCharts';
import { WebClientChapter } from './WebClientChapter';
import { productChapters, type HomeLanguage } from '../data/product';

const LANGUAGE_KEY = 'a3s-os.website-language';

function MarkdownHome() {
  return (
    <main>
      <h1>A3S OS 企业级 AI 操作系统</h1>
      <p>认知驱动的本体工程、自主进化与大规模高可用服务平台。</p>
      <h2>Workflow 自主工作流编排</h2>
      <p>
        用本体工程描述业务世界，再把对象、关系、规则、目标和约束编译为可恢复的长期流程。
      </p>
      <h2>Agent Factory 异构智能体工厂</h2>
      <p>
        把
        Agent、Skill、MCP、模型固定版本和安全策略装配成可版本、可部署、可审计的数字资产。
      </p>
      <h2>安全监控中台</h2>
      <p>
        把 Gateway、Runtime、Box、Fleet、身份和 AnySentry
        信号收敛成统一安全响应闭环。
      </p>
      <h2>A3S Web 客户端</h2>
      <p>
        通过一个可搜索、可操作、可审计的工作空间管理三大产品和完整运行证据。
      </p>
      <h2>模块架构</h2>
      <p>
        三大产品共享同一控制面、节点通道、运行时、Agent Harness 和信任边界。
      </p>
    </main>
  );
}

export function HomeLayout() {
  const [language, setLanguage] = useState<HomeLanguage>('zh');
  const zh = language === 'zh';
  const route = (path: string) => withBase(path);

  useEffect(() => {
    const stored = window.localStorage.getItem(LANGUAGE_KEY);
    if (stored === 'en') setLanguage('en');
  }, []);

  useEffect(() => {
    window.localStorage.setItem(LANGUAGE_KEY, language);
    document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en';
  }, [language]);

  if (import.meta.env.SSG_MD) return <MarkdownHome />;

  return (
    <main className="cloud-home">
      <InteractionLayer />

      <section className="cloud-hero">
        <div className="cloud-hero-copy">
          <div className="cloud-hero-meta">
            <span>
              A3S OS ·{' '}
              {zh ? '企业级 AI 操作系统' : 'ENTERPRISE AI OPERATING SYSTEM'}
            </span>
            <div
              className="cloud-language-switch"
              aria-label={zh ? '语言切换' : 'Language switch'}
            >
              <button
                className={zh ? 'is-active' : undefined}
                type="button"
                aria-pressed={zh}
                onClick={() => setLanguage('zh')}
              >
                中文
              </button>
              <button
                className={!zh ? 'is-active' : undefined}
                type="button"
                aria-pressed={!zh}
                onClick={() => setLanguage('en')}
              >
                EN
              </button>
            </div>
          </div>
          <h1>
            {zh ? '企业级 AI，' : 'Enterprise AI,'}
            <span>{zh ? '需要一套操作系统。' : 'operated as one system.'}</span>
          </h1>
          <p>
            {zh
              ? 'A3S OS 将认知驱动的本体工程、自主进化机制与大规模高可用服务底座收敛到同一平台，让业务意图、智能体资产和安全治理真正进入生产系统。'
              : 'A3S OS brings cognitive ontology engineering, autonomous evolution, and a large-scale highly available service foundation into one platform, moving business intent, Agent assets, and security governance into production.'}
          </p>
          <div className="cloud-hero-actions">
            <a className="cloud-button is-primary" href="#workflow">
              {zh ? '了解三大产品' : 'Explore the products'}
              <ArrowRight aria-hidden="true" weight="bold" />
            </a>
            <a className="cloud-button is-secondary" href="#architecture">
              {zh ? '查看模块架构' : 'View architecture'}
              <ArrowRight aria-hidden="true" weight="bold" />
            </a>
          </div>
        </div>
        <div className="cloud-hero-visual" data-reveal>
          <ProductSystemChart language={language} />
        </div>
      </section>

      <aside
        className="cloud-assurance-bar"
        aria-label={zh ? '核心亮点' : 'Core strengths'}
      >
        <strong>{zh ? '三大核心亮点' : 'THREE CORE STRENGTHS'}</strong>
        <ul>
          <li>
            <b>
              <Brain aria-hidden="true" weight="duotone" />
            </b>
            <div>
              <strong>
                {zh ? '认知驱动的本体工程' : 'Cognitive ontology engineering'}
              </strong>
              <small>
                {zh
                  ? '让业务语义成为可计算的系统模型'
                  : 'Make business semantics computable'}
              </small>
            </div>
          </li>
          <li>
            <b>
              <ArrowsClockwise aria-hidden="true" weight="duotone" />
            </b>
            <div>
              <strong>
                {zh ? '可治理的自主进化' : 'Governed autonomous evolution'}
              </strong>
              <small>
                {zh
                  ? '用运行证据持续优化策略与版本'
                  : 'Improve policies and versions with evidence'}
              </small>
            </div>
          </li>
          <li>
            <b>
              <CloudCheck aria-hidden="true" weight="duotone" />
            </b>
            <div>
              <strong>
                {zh
                  ? '大规模高可用服务平台'
                  : 'Large-scale highly available platform'}
              </strong>
              <small>
                {zh
                  ? '面向生产环境的调度、恢复与治理'
                  : 'Production scheduling, recovery, and governance'}
              </small>
            </div>
          </li>
        </ul>
      </aside>

      <section className="cloud-product-intro" data-reveal>
        <span>{zh ? '产品体系' : 'PRODUCT SYSTEM'}</span>
        <h2>
          {zh
            ? '三大产品，共同完成企业 AI 的生产闭环'
            : 'Three products complete the enterprise AI production loop'}
        </h2>
        <p>
          {zh
            ? '自主工作流编排把本体认知转化为持续运行的计划；异构智能体工厂把不同技术栈的能力转化为可交付资产；安全监控中台让每次执行保持可见、可控、可追溯。三者共享同一套状态、执行与证据底座。'
            : 'Autonomous workflow orchestration turns ontology-based cognition into durable plans. The heterogeneous Agent Factory turns different technology stacks into deliverable assets. Security Operations keeps every execution visible, controlled, and traceable. All three share one state, execution, and evidence foundation.'}
        </p>
      </section>

      {productChapters.map((product) => (
        <ProductChapter
          language={language}
          product={product}
          key={product.id}
        />
      ))}

      <WebClientChapter language={language} />
      <PlatformArchitecture language={language} />

      <section className="cloud-final-cta">
        <div>
          <span>{zh ? '从意图到证据' : 'FROM INTENT TO EVIDENCE'}</span>
          <h2>
            {zh
              ? '用一套系统，运营企业 AI。'
              : 'Operate enterprise AI as one system.'}
          </h2>
          <p>
            {zh
              ? '从仓库中的真实合约、路线 Gate 和模块边界开始理解 A3S OS。'
              : 'Start with the real contracts, roadmap gates, and module boundaries in the repository.'}
          </p>
        </div>
        <div>
          <a
            className="cloud-button is-primary"
            href="https://github.com/A3S-Lab/Cloud"
          >
            <GithubLogo aria-hidden="true" weight="fill" />
            GitHub
          </a>
          <a
            className="cloud-button is-secondary"
            href={route('/architecture/')}
          >
            {zh ? '交互式架构' : 'Interactive architecture'}
            <ArrowRight aria-hidden="true" weight="bold" />
          </a>
        </div>
      </section>

      <footer className="cloud-footer">
        <a href={route('/')}>
          <img alt="" src={route('/a3s-cloud-mark.svg')} />
          A3S OS
        </a>
        <span>
          {zh ? '企业级 AI 操作系统' : 'Enterprise AI operating system'}
        </span>
        <div>
          <a href="#workflow">Workflow</a>
          <a href="#agent-factory">Agent Factory</a>
          <a href="#security-operations">{zh ? '安全监控' : 'Security'}</a>
          <a href={route('/docs/')}>{zh ? '版本文档' : 'Versioned docs'}</a>
          <a href="https://github.com/A3S-Lab/Cloud">GitHub</a>
        </div>
      </footer>
    </main>
  );
}
