import { withBase } from '@rspress/core/runtime';
import {
  ArrowRight,
  ArrowsClockwise,
  Brain,
  CloudCheck,
  GithubLogo,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';
import { EdgeCloudFoundationChapter } from './EdgeCloudFoundationChapter';
import { EdgeAgentChapter } from './EdgeAgentChapter';
import { IndustrySolutionsChapter } from './IndustrySolutionsChapter';
import { InteractionLayer } from './InteractionLayer';
import { PlatformArchitecture } from './PlatformArchitecture';
import { ProductChapter } from './ProductChapter';
import { HeroVisual } from './ProductCharts';
import { productChapters, type HomeLanguage } from '../data/product';

const LANGUAGE_KEY = 'a3s-os.website-language';

function MarkdownHome() {
  return (
    <main>
      <h1>A3S OS 企业级 AI 操作系统</h1>
      <p>
        A3S OS 基于云原生微服务架构，打造「可管、可控、可协作、可审计」的企业级
        AI Native 操作系统，提供全栈国产化的端云一体智能体和工作流安全执行平台。
      </p>
      <h2>A3S Gateway 统一网关</h2>
      <p>
        统一治理 Workflow、Agent、MCP、模型 API
        与业务服务的接入、身份、策略、路由和端云流量。
      </p>
      <h2>Workflow 自主工作流编排</h2>
      <p>
        用本体工程描述业务世界，再把对象、关系、规则、目标和约束编译为可恢复的长期流程。
      </p>
      <h2>Agent Factory 异构智能体工厂</h2>
      <p>
        把
        Agent、Skill、MCP、模型固定版本和安全策略装配成可版本、可部署、可审计的数字资产。
      </p>
      <h2>A3S Work 端侧智能体</h2>
      <p>
        通过一个可搜索、可操作、可审计的工作空间管理三大产品和完整运行证据。
      </p>
      <h2>模块架构</h2>
      <p>
        三大产品共享同一控制面、节点通道、运行时与信任边界，并通过统一托管契约接入异构
        Agent 与 Harness。
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

  useEffect(() => {
    const normalizedPath = (path: string) =>
      path.replace(/index\.html$/, '').replace(/\/+$/, '/');

    const scrollToHash = (behavior: ScrollBehavior) => {
      const hash = window.location.hash.slice(1);
      if (!hash) return;

      const target = document.getElementById(decodeURIComponent(hash));
      target?.scrollIntoView({ behavior, block: 'start' });
    };

    const onAnchorClick = (event: MouseEvent) => {
      if (
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey
      ) {
        return;
      }

      const source = event.target;
      if (!(source instanceof Element)) return;

      const anchor = source.closest<HTMLAnchorElement>('a[href*="#"]');
      if (!anchor || anchor.target || anchor.hasAttribute('download')) return;

      const destination = new URL(anchor.href, window.location.href);
      if (
        destination.origin !== window.location.origin ||
        !destination.hash ||
        normalizedPath(destination.pathname) !==
          normalizedPath(window.location.pathname)
      ) {
        return;
      }

      const target = document.getElementById(
        decodeURIComponent(destination.hash.slice(1)),
      );
      if (!target) return;

      event.preventDefault();
      event.stopPropagation();
      const currentPath = window.location.pathname.replace(/index\.html$/, '');
      window.history.pushState(
        null,
        '',
        `${currentPath}${window.location.search}${destination.hash}`,
      );
      target.scrollIntoView({ behavior: 'smooth', block: 'start' });
    };

    const onHistoryChange = () => scrollToHash('auto');
    const frame = window.requestAnimationFrame(onHistoryChange);

    document.addEventListener('click', onAnchorClick, true);
    window.addEventListener('hashchange', onHistoryChange);
    window.addEventListener('popstate', onHistoryChange);

    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener('click', onAnchorClick, true);
      window.removeEventListener('hashchange', onHistoryChange);
      window.removeEventListener('popstate', onHistoryChange);
    };
  }, []);

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
            <span className="cloud-hero-brand">A3S OS</span>
            <span>
              {zh
                ? '企业级 AI 操作系统'
                : 'The operating system for enterprise AI'}
            </span>
          </h1>
          <p>
            {zh
              ? '基于云原生微服务架构，打造「可管、可控、可协作、可审计」的企业级 AI Native 操作系统，提供全栈国产化的端云一体智能体和工作流安全执行平台。'
              : 'Built on cloud-native microservices, A3S OS is a manageable, controllable, collaborative, and auditable AI-native operating system for secure cloud-to-edge Agent and workflow execution.'}
          </p>
          <div className="cloud-hero-actions">
            <a className="cloud-button is-primary" href="#unified-gateway">
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
          <HeroVisual language={language} />
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
                  ? '模型 Agentic RL 与 Harness 自学习双螺旋'
                  : 'Model Agentic RL and harness self-learning'}
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

      <EdgeCloudFoundationChapter language={language} />

      <section className="cloud-product-intro" data-reveal>
        <pre className="cloud-ascii-scene is-product-system" aria-hidden="true">
          {`A3S GATEWAY
     |
WORKFLOW -- AGENT -- MCP -- MODEL
     |
 A3S RUNTIME
     |
 CPU / GPU`}
        </pre>
        <span>{zh ? '产品体系' : 'PRODUCT SYSTEM'}</span>
        <h2>
          {zh
            ? '三大产品，共同完成企业 AI 的生产闭环'
            : 'Three products complete the enterprise AI production loop'}
        </h2>
        <p>
          {zh
            ? 'A3S Gateway 统一治理所有智能流量；自主工作流编排把本体认知转化为持续运行的计划；异构智能体工厂把不同技术栈的能力转化为可交付资产。AnySentry 从基础设施到上层应用提供全链路可观测性和数据轨迹回流。'
            : 'A3S Gateway governs every intelligent workload through one gateway. Autonomous workflow orchestration turns ontology-based cognition into durable plans, and the heterogeneous Agent Factory turns different technology stacks into deliverable assets. AnySentry provides full-path observability and data-trajectory return from infrastructure to applications.'}
        </p>
      </section>

      {productChapters.map((product) => (
        <ProductChapter
          language={language}
          product={product}
          key={product.id}
        />
      ))}

      <EdgeAgentChapter language={language} />
      <IndustrySolutionsChapter language={language} />
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
          <img alt="" height="28" src={route('/a3s-os-logo.png')} width="28" />
          A3S OS
        </a>
        <span>
          {zh ? '企业级 AI 操作系统' : 'Enterprise AI operating system'}
        </span>
        <div>
          <a href="#unified-gateway">{zh ? '统一网关' : 'Gateway'}</a>
          <a href="#workflow">{zh ? '工作流编排' : 'Workflow'}</a>
          <a href="#agent-factory">{zh ? '智能体工厂' : 'Agent Factory'}</a>
          <a href={route('/docs/')}>{zh ? '版本文档' : 'Versioned docs'}</a>
          <a href="https://github.com/A3S-Lab/Cloud">GitHub</a>
        </div>
      </footer>
    </main>
  );
}
