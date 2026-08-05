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
    component: 'Agent Hosting',
    title: { zh: 'Agent 安全执行', en: 'Secure Agent execution' },
    body: {
      zh: '任意异构 Agent 通过自己的 Harness 执行，本地工具、文件与网络访问由统一策略授权和审计。',
      en: 'Each heterogeneous Agent runs through its own harness while local tools, files, and network access share one policy and audit boundary.',
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
    component: 'A3S Work',
    title: { zh: '统一智能工作台', en: 'Unified AI workspace' },
    body: {
      zh: '工作流编排、智能体运营、统一网关与 AnySentry 运行视图集中呈现，减少跨系统切换。',
      en: 'Orchestrate workflows, operate Agents, and inspect A3S Gateway and AnySentry state in one A3S Work workspace.',
    },
    icon: Browser,
  },
  {
    component: 'Runtime + Box',
    title: { zh: '端侧运行与隔离', en: 'Edge runtime and isolation' },
    body: {
      zh: 'Runtime 以 WaaS、AaaS、FaaS 承载工作流、智能体与无状态服务，并按安全要求进入 Box 隔离环境。',
      en: 'Runtime hosts workflows, Agents, and stateless services through WaaS, AaaS, and FaaS, then executes them inside explicit Box isolation.',
    },
    icon: Cube,
  },
  {
    component: 'A3S Use',
    title: { zh: '资产包热插拔', en: 'Hot-pluggable Asset Packages' },
    body: {
      zh: '通过 A3S Use 包管理器按需安装资产托管平台中的 Workflow、Agent、Model、MCP、Skill、OKF 与 Tool 资产包。',
      en: 'Use A3S Use Package Manager to install hosted Workflow, Agent, Model, MCP, Skill, OKF, and Tool Packages on demand.',
    },
    icon: Package,
  },
];

const foundationItems = [
  { title: { zh: '异构 Harness', en: 'Heterogeneous harnesses' }, icon: Code },
  { title: { zh: '出站 mTLS', en: 'Outbound mTLS' }, icon: Key },
  { title: { zh: '数据本地驻留', en: 'Local data' }, icon: Database },
  {
    title: { zh: '显式安全隔离', en: 'Explicit isolation' },
    icon: ShieldCheck,
  },
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
        <span>A3S WORK · EDGE AGENT</span>
        <h2>{zh ? 'A3S Work 端侧智能体' : 'A3S Work Edge Agent'}</h2>
        <h3>
          {zh
            ? '把企业 AI 安全带到每一个业务终端'
            : 'Bring enterprise AI safely to every business endpoint'}
        </h3>
        <p>
          {zh
            ? 'A3S Work 将工作流、异构智能体、统一网关、AnySentry 观测与端侧执行汇入一个工作空间，并通过 A3S Use 包管理器按需热插拔安装资产托管平台的资产包。敏感数据就近留存，平台通过统一身份、策略和证据完成治理。'
            : 'A3S Work unifies all three products, heterogeneous Agent execution, and AnySentry insight in one edge workspace. A3S Use Package Manager hot-plugs hosted Asset Packages on demand, while identity, policy, and evidence remain governed by one platform contract.'}
        </p>
      </header>

      <div className="cloud-edge-web-capabilities" data-reveal>
        {capabilities.map(({ body, component, icon: Icon, title }, index) => (
          <article key={component}>
            <figure className={`is-tone-${index + 1}`} aria-hidden="true">
              <i className="is-signal-a" />
              <i className="is-signal-b" />
              <i className="is-signal-c" />
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
            <strong>
              {zh ? '端侧能力，统一治理' : 'One governed edge runtime'}
            </strong>
            <small>
              {zh
                ? 'A3S Work、Node Agent 与异构执行组件共用身份、策略和证据'
                : 'A3S Work, Node Agent, and heterogeneous runtimes share identity, policy, and evidence'}
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
