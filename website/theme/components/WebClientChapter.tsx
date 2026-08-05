import {
  ArrowsClockwise,
  Browser,
  Cube,
  Globe,
  MagnifyingGlass,
  Network,
  Package,
  Pulse,
  Robot,
} from '@phosphor-icons/react';
import type { HomeLanguage, LocalizedText } from '../data/product';

const capabilities = [
  {
    title: { zh: '全局概览', en: 'Workspace overview' },
    body: {
      zh: '环境健康、活动 Operation、Workload、构建与路由状态集中呈现。',
      en: 'See environment health, active Operations, Workloads, builds, and routes together.',
    },
    icon: Pulse,
  },
  {
    title: { zh: 'Workload 运行', en: 'Workload operations' },
    body: {
      zh: '查看收敛、版本、部署时间线、实时日志、停止、取消与回滚。',
      en: 'Inspect convergence, versions, deployment timelines, live logs, stop, cancel, and rollback.',
    },
    icon: Cube,
  },
  {
    title: { zh: 'Agent 工作台', en: 'Agent workspace' },
    body: {
      zh: '管理会话、执行、审批、语义事件、检查点和唯一 Harness 证据。',
      en: 'Manage conversations, runs, approvals, semantic events, checkpoints, and sole-Harness evidence.',
    },
    icon: Robot,
  },
  {
    title: { zh: '交付与证据', en: 'Delivery and evidence' },
    body: {
      zh: '追踪 Source Revision、BuildRun、制品、SBOM、签名证据与发布结果。',
      en: 'Trace source revisions, BuildRuns, artifacts, SBOMs, signed evidence, and release outcomes.',
    },
    icon: Package,
  },
  {
    title: { zh: 'Edge 与安全', en: 'Edge and security' },
    body: {
      zh: '统一查看域名、TLS、路由、Gateway 状态和安全运行信号。',
      en: 'Inspect domains, TLS, routes, Gateway state, and security runtime signals in one place.',
    },
    icon: Globe,
  },
  {
    title: { zh: '架构与全局搜索', en: 'Architecture and global search' },
    body: {
      zh: '跨资源定位对象，查看完整模块图，并将实时 HTML 架构导出为 PNG。',
      en: 'Find resources across modules, inspect the full module map, and export the live HTML architecture as PNG.',
    },
    icon: Network,
  },
] as const;

const localize = (copy: LocalizedText, language: HomeLanguage) =>
  copy[language];

export function WebClientChapter({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <section className="cloud-web-client" id="web-client">
      <header className="cloud-web-client-heading" data-reveal>
        <span className="cloud-product-index">04</span>
        <div>
          <small>A3S WEB</small>
          <h2>
            {zh
              ? '一个客户端，贯通三大产品的每一次工作'
              : 'One client for every action across all three products'}
          </h2>
          <p>
            {zh
              ? 'A3S Web 是 A3S OS 的统一客户端，不另建控制机制。它复用同一组命令、查询、授权与证据，让业务人员、Agent 工程师和平台运维在同一个可搜索、可操作、可审计的工作空间中协同。'
              : 'A3S Web is the unified client for A3S OS and does not introduce another control mechanism. It reuses the same commands, queries, authorization, and evidence so business users, Agent engineers, and platform operators can collaborate in one searchable, operable, and auditable workspace.'}
          </p>
        </div>
      </header>

      <div className="cloud-web-client-layout">
        <div className="cloud-web-client-capabilities" data-reveal>
          {capabilities.map(({ body, icon: Icon, title }) => (
            <article key={title.en}>
              <span>
                <Icon aria-hidden="true" size={22} weight="duotone" />
              </span>
              <div>
                <strong>{localize(title, language)}</strong>
                <p>{localize(body, language)}</p>
              </div>
            </article>
          ))}
        </div>

        <figure
          className="cloud-web-window"
          data-reveal
          aria-labelledby="web-window-title"
        >
          <figcaption id="web-window-title">
            <div>
              <Browser aria-hidden="true" size={18} weight="duotone" />
              <strong>A3S Web</strong>
            </div>
            <span>
              {zh ? '统一运行工作区' : 'UNIFIED OPERATIONS WORKSPACE'}
            </span>
          </figcaption>
          <div className="cloud-web-window-toolbar" aria-hidden="true">
            <span>
              <MagnifyingGlass size={15} />
              {zh
                ? '搜索资源、执行与证据'
                : 'Search resources, runs, and evidence'}
            </span>
            <i />
            <i />
          </div>
          <div className="cloud-web-window-body" aria-hidden="true">
            <nav>
              {[
                [Pulse, zh ? '概览' : 'Overview'],
                [Cube, 'Workloads'],
                [Robot, 'Agents'],
                [Package, zh ? '交付' : 'Delivery'],
                [Globe, 'Edge'],
                [Network, zh ? '架构' : 'Architecture'],
              ].map(([Icon, label], index) => {
                const NavIcon = Icon as typeof Pulse;
                return (
                  <span
                    className={index === 0 ? 'is-active' : undefined}
                    key={String(label)}
                  >
                    <NavIcon size={15} />
                    {String(label)}
                  </span>
                );
              })}
            </nav>
            <div className="cloud-web-dashboard">
              <header>
                <div>
                  <small>{zh ? '当前环境' : 'CURRENT ENVIRONMENT'}</small>
                  <strong>{zh ? '生产工作区' : 'Production workspace'}</strong>
                </div>
                <span>
                  <ArrowsClockwise size={14} />
                  {zh ? '实时同步' : 'Live sync'}
                </span>
              </header>
              <div className="cloud-web-metrics">
                {[
                  [zh ? '运行单元' : 'Workloads', '12'],
                  [zh ? '活动操作' : 'Operations', '03'],
                  [zh ? '构建任务' : 'Build runs', '08'],
                  [zh ? '在线路由' : 'Routes', '06'],
                ].map(([label, value]) => (
                  <div key={label}>
                    <small>{label}</small>
                    <strong>{value}</strong>
                  </div>
                ))}
              </div>
              <div className="cloud-web-timeline">
                <strong>{zh ? '运行时间线' : 'Runtime timeline'}</strong>
                {[72, 56, 84, 64].map((width, index) => (
                  <span key={width}>
                    <i style={{ width: `${width}%` }} />
                    <b>0{index + 1}</b>
                  </span>
                ))}
              </div>
            </div>
          </div>
          <p>
            {zh
              ? 'CLIENT SURFACE · 实时资源投影 · 同一 API 合约'
              : 'CLIENT SURFACE · LIVE RESOURCE PROJECTION · ONE API CONTRACT'}
          </p>
        </figure>
      </div>
    </section>
  );
}
