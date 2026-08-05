import {
  Browser,
  Code,
  Cube,
  Database,
  DesktopTower,
  FlowArrow,
  Key,
  Network,
  Package,
  ShieldCheck,
  Stack,
  type IconProps,
} from '@phosphor-icons/react';
import type { ComponentType } from 'react';
import type { HomeLanguage, LocalizedText } from '../data/product';

type EdgeWebCapability = {
  component: string;
  title: LocalizedText;
  body: LocalizedText;
  icon: ComponentType<IconProps>;
};

const capabilities: EdgeWebCapability[] = [
  {
    component: 'A3S Code',
    title: { zh: 'Agent 安全执行', en: 'Secure Agent execution' },
    body: {
      zh: 'Agent 任务经 Code Harness 执行，对本地工具、文件与网络访问实施统一授权和审计。',
      en: 'Every Agent run enters Code Harness, with local tools, files, and network access governed by one security boundary.',
    },
    icon: ShieldCheck,
  },
  {
    component: 'Node Agent',
    title: { zh: '终端与边缘纳管', en: 'Endpoint and edge management' },
    body: {
      zh: '员工终端、开发机与边缘节点通过出站 mTLS 安全接入，企业网络无需开放入站管理端口。',
      en: 'Desktops, developer machines, and edge nodes connect over outbound mTLS without inbound management ports.',
    },
    icon: DesktopTower,
  },
  {
    component: 'A3S Web',
    title: { zh: '统一智能工作台', en: 'Unified AI workspace' },
    body: {
      zh: '工作流编排、智能体运营、安全审批和运行状态集中呈现，减少跨系统切换。',
      en: 'Orchestrate workflows, operate Agents, handle approvals, and inspect security state in one Web workspace.',
    },
    icon: Browser,
  },
  {
    component: 'Runtime + Box',
    title: { zh: '端侧运行与隔离', en: 'Edge runtime and isolation' },
    body: {
      zh: '任务与服务由 Runtime 管理生命周期，并按安全要求进入 Box 隔离环境。',
      en: 'Tasks and Services reuse Runtime and Box with explicit isolation and no second execution mechanism.',
    },
    icon: Cube,
  },
  {
    component: 'Agent Assets',
    title: { zh: 'Skill 与证据治理', en: 'Skill and evidence governance' },
    body: {
      zh: '集中管理 Skill、MCP、版本、权限、运行回执与审计记录，支持审核和追溯。',
      en: 'Govern Skills, MCP, versions, permissions, receipts, and audit evidence as managed enterprise assets.',
    },
    icon: Package,
  },
];

const foundationItems = [
  { title: { zh: '唯一 Harness', en: 'One Harness' }, icon: Code },
  { title: { zh: '出站 mTLS', en: 'Outbound mTLS' }, icon: Key },
  { title: { zh: '数据本地驻留', en: 'Local data' }, icon: Database },
  { title: { zh: '显式安全隔离', en: 'Explicit isolation' }, icon: ShieldCheck },
  { title: { zh: '私有网络部署', en: 'Private network' }, icon: Network },
  { title: { zh: '全链路可审计', en: 'End-to-end audit' }, icon: Stack },
] as const;

const localize = (copy: LocalizedText, language: HomeLanguage) =>
  copy[language];

export function WebClientChapter({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <section className="cloud-edge-web" id="edge-agent">
      <span className="cloud-section-anchor" id="web-client" />
      <header className="cloud-edge-web-heading" data-reveal>
        <span>A3S WEB · EDGE AGENT</span>
        <h2>{zh ? 'A3S Web 端侧智能体' : 'A3S Web Edge Agent'}</h2>
        <h3>
          {zh
            ? '把企业 AI 安全带到每一个业务终端'
            : 'Bring enterprise AI safely to every business endpoint'}
        </h3>
        <p>
          {zh
            ? 'A3S Web 将工作流、智能体、安全监控与端侧执行汇入一个工作空间。员工在终端侧处理业务，敏感数据就近留存；平台通过统一身份、策略和证据完成治理。'
            : 'A3S Web is the edge Agent workspace of A3S OS, not a separate client or second control plane. It unifies access to all three products, secure local execution, and enterprise asset governance under one identity, policy, and evidence contract.'}
        </p>
      </header>

      <div className="cloud-edge-web-capabilities" data-reveal>
        {capabilities.map(({ body, component, icon: Icon, title }, index) => (
          <article key={component}>
            <figure className={`is-tone-${index + 1}`} aria-hidden="true">
              <span>
                <Icon size={56} weight="duotone" />
              </span>
            </figure>
            <small>{component}</small>
            <h4>{localize(title, language)}</h4>
            <p>{localize(body, language)}</p>
          </article>
        ))}
      </div>

      <div className="cloud-edge-web-foundation" data-reveal>
        <header>
          <span>
            <FlowArrow aria-hidden="true" size={25} weight="duotone" />
          </span>
          <div>
            <strong>{zh ? '端侧能力，统一治理' : 'One governed edge runtime'}</strong>
            <small>
              {zh
                ? 'A3S Web、Node Agent 与执行组件共用身份、策略和证据'
                : 'A3S Web, Node Agent, and the runtime share identity, policy, and evidence'}
            </small>
          </div>
        </header>
        <div>
          {foundationItems.map(({ icon: Icon, title }) => (
            <span key={title.en}>
              <Icon aria-hidden="true" size={19} weight="duotone" />
              {localize(title, language)}
            </span>
          ))}
        </div>
      </div>
    </section>
  );
}
