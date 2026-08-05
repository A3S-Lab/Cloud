import {
  Bank,
  Briefcase,
  Buildings,
  Factory,
  FlowArrow,
  Gear,
  Lightning,
  Network,
  Robot,
  ShieldCheck,
  Stack,
  type IconProps,
} from '@phosphor-icons/react';
import type { ComponentType } from 'react';
import type { HomeLanguage, LocalizedText } from '../data/product';

type IndustrySolution = {
  eyebrow: LocalizedText;
  title: LocalizedText;
  body: LocalizedText;
  icon: ComponentType<IconProps>;
};

const solutions: IndustrySolution[] = [
  {
    eyebrow: { zh: '风控 · 客户运营 · 审计', en: 'RISK · OPERATIONS · AUDIT' },
    title: { zh: '金融机构', en: 'Financial services' },
    body: {
      zh: '将高风险 Agent 流程纳入统一审批、执行和追溯链路。',
      en: 'Move high-risk Agent workflows into an approved, replayable, and traceable production loop.',
    },
    icon: Bank,
  },
  {
    eyebrow: {
      zh: '城市治理 · 政务服务 · 决策',
      en: 'GOVERNANCE · SERVICE · DECISIONS',
    },
    title: { zh: '政务与公共服务', en: 'Government and public service' },
    body: {
      zh: '通过本体模型统一业务口径，支持跨部门流程协作和权责管理。',
      en: 'Unify business semantics with ontology engineering and coordinate cross-agency workflows within clear authority boundaries.',
    },
    icon: Buildings,
  },
  {
    eyebrow: {
      zh: '质检 · 运维 · 供应链',
      en: 'QUALITY · OPERATIONS · SUPPLY CHAIN',
    },
    title: { zh: '智能制造', en: 'Intelligent manufacturing' },
    body: {
      zh: '连接现场端侧智能体与云端调度，统一管理设备任务和异常处置。',
      en: 'Connect on-site edge Agents with cloud scheduling and govern equipment, tasks, and incident response as one runtime model.',
    },
    icon: Factory,
  },
  {
    eyebrow: {
      zh: '巡检 · 调度 · 合规',
      en: 'INSPECTION · SCHEDULING · COMPLIANCE',
    },
    title: { zh: '能源与基础设施', en: 'Energy and infrastructure' },
    body: {
      zh: '面向弱网和分布式现场保障任务连续性，支持端云调度和集中审计。',
      en: 'Maintain continuity across distributed and constrained sites while cloud-edge coordination supplies elastic scheduling and unified audit.',
    },
    icon: Lightning,
  },
  {
    eyebrow: {
      zh: '办公 · 知识 · 服务运营',
      en: 'WORK · KNOWLEDGE · SERVICE OPS',
    },
    title: { zh: '企业智能办公', en: 'Enterprise AI workplace' },
    body: {
      zh: '将审批、知识、客服和日常运营流程沉淀为可复用的 Workflow、Agent 与 Skill。',
      en: 'Turn approvals, knowledge, service, and daily operations into reusable Workflow, Agent, and Skill assets.',
    },
    icon: Briefcase,
  },
];

const sharedFoundation = [
  { title: { zh: '本体工程', en: 'Ontology' }, icon: Network },
  { title: { zh: '自主工作流', en: 'Workflow' }, icon: FlowArrow },
  { title: { zh: '异构智能体', en: 'Agents' }, icon: Robot },
  { title: { zh: '安全执行', en: 'Secure execution' }, icon: ShieldCheck },
  { title: { zh: '端云协同', en: 'Cloud-edge' }, icon: Stack },
  { title: { zh: '统一治理', en: 'Governance' }, icon: Gear },
] as const;

const localize = (copy: LocalizedText, language: HomeLanguage) =>
  copy[language];

export function IndustrySolutionsChapter({
  language,
}: {
  language: HomeLanguage;
}) {
  const zh = language === 'zh';

  return (
    <section className="cloud-industry-solutions" id="solutions">
      <header className="cloud-industry-heading" data-reveal>
        <span>{zh ? '行业场景' : 'INDUSTRY SOLUTIONS'}</span>
        <h2>{zh ? '行业场景解决方案' : 'Solutions for complex industries'}</h2>
        <h3>
          {zh
            ? '面向强监管行业与复杂流程，复用同一套 AI 操作系统底座'
            : 'Reuse one AI operating system across regulated industries and complex workflows'}
        </h3>
        <p>
          {zh
            ? '各行业共享本体模型、工作流、Agent 资产、安全执行和审计能力，并根据组织权限、数据边界和业务流程进行配置。'
            : 'A3S OS does not rebuild a control plane for each scenario. Industry solutions share ontology models, workflows, Agent assets, secure execution, and audit evidence, then compose them around each business boundary.'}
        </p>
      </header>

      <div className="cloud-industry-grid" data-reveal>
        {solutions.map(({ body, eyebrow, icon: Icon, title }) => (
          <article key={title.en}>
            <span className="cloud-industry-icon" aria-hidden="true">
              <Icon size={38} weight="regular" />
            </span>
            <small>{localize(eyebrow, language)}</small>
            <h4>{localize(title, language)}</h4>
            <p>{localize(body, language)}</p>
          </article>
        ))}
      </div>

      <div className="cloud-industry-foundation" data-reveal>
        <header>
          <span>
            <Network aria-hidden="true" size={25} weight="duotone" />
          </span>
          <div>
            <strong>
              {zh ? '一套底座，多行业复用' : 'One foundation, many industries'}
            </strong>
            <small>
              {zh
                ? '场景变化，核心运行与治理合约保持一致'
                : 'Adapt scenarios while preserving runtime and governance contracts'}
            </small>
          </div>
        </header>
        <div>
          {sharedFoundation.map(({ icon: Icon, title }) => (
            <span key={title.en}>
              <Icon aria-hidden="true" size={18} weight="duotone" />
              {localize(title, language)}
            </span>
          ))}
        </div>
      </div>
    </section>
  );
}
