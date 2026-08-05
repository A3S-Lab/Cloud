import {
  Brain,
  Check,
  Code,
  Cube,
  Factory,
  Fingerprint,
  GearSix,
  Package,
  PlugsConnected,
  Robot,
  Scan,
  SealCheck,
  ShieldCheck,
  Wrench,
} from '@phosphor-icons/react';
import type { HomeLanguage } from '../../data/product';

export function AgentFactoryStory({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';
  const assets = [
    { icon: Code, label: 'Harness', detail: 'Python Agent' },
    { icon: Brain, label: zh ? '模型' : 'Model', detail: 'Qwen3-32B' },
    { icon: Wrench, label: 'Skill / Tool', detail: '12 attached' },
    { icon: PlugsConnected, label: 'MCP', detail: '3 servers' },
    {
      icon: ShieldCheck,
      label: zh ? '安全策略' : 'Security',
      detail: 'policy@6',
    },
  ];

  return (
    <figure
      className="cloud-editorial-chart cloud-product-demo cloud-factory-story cloud-motion-scene"
      aria-labelledby="factory-story-title"
    >
      <figcaption>
        <strong id="factory-story-title">
          {zh
            ? '把异构 Agent 装配为可部署产品'
            : 'Assemble heterogeneous Agents into deployable products'}
        </strong>
        <span>
          {zh
            ? 'Harness、模型、Skill、MCP 与安全策略在同一工位装配，并固定为带身份和证据契约的 Release'
            : 'Harness, model, Skills, MCP, and security policy assemble on one station and pin into a Release with identity and evidence contracts'}
        </span>
      </figcaption>

      <div className="cloud-factory-console" aria-hidden="true">
        <header className="cloud-factory-consolebar">
          <span>
            <Factory size={16} weight="duotone" />
            <strong>Agent Factory</strong>
            <small>{zh ? '装配工位 03' : 'ASSEMBLY BAY 03'}</small>
          </span>
          <span className="cloud-factory-order">AF-2048 · support-agent</span>
          <span className="cloud-factory-line-state">
            <i /> {zh ? '生产中' : 'BUILDING'}
          </span>
        </header>

        <div className="cloud-factory-console-body">
          <aside className="cloud-factory-parts-bin">
            <header>
              <Package size={14} weight="duotone" />
              <strong>{zh ? '装配清单' : 'Manifest'}</strong>
            </header>
            <div>
              {assets.map(({ detail, icon: Icon, label }, index) => (
                <article className={`is-part-${index + 1}`} key={label}>
                  <span>
                    <Icon size={15} weight="duotone" />
                  </span>
                  <div>
                    <strong>{label}</strong>
                    <small>{detail}</small>
                  </div>
                  <i>
                    <Check size={10} weight="bold" />
                  </i>
                </article>
              ))}
            </div>
            <footer>
              <Cube size={13} weight="duotone" />{' '}
              {zh ? '5 个资产已锁定' : '5 assets locked'}
            </footer>
          </aside>

          <main className="cloud-factory-bay">
            <div className="cloud-factory-bay-heading">
              <span>
                <i /> {zh ? '自动装配' : 'AUTO ASSEMBLY'}
              </span>
              <code>harness://python-agent</code>
            </div>

            <div className="cloud-factory-overhead">
              <span className="cloud-factory-trolley">
                <i className="cloud-factory-arm-segment" />
                <i className="cloud-factory-gripper">
                  <b />
                  <b />
                </i>
              </span>
            </div>

            <div className="cloud-agent-robot">
              <span className="cloud-robot-antenna">
                <i />
              </span>
              <section className="cloud-robot-head">
                <Brain size={19} weight="duotone" />
                <i />
                <i />
                <small>MODEL</small>
              </section>
              <section className="cloud-robot-core">
                <Code size={22} weight="duotone" />
                <strong>HARNESS</strong>
                <span className="cloud-robot-core-light" />
              </section>
              <span className="cloud-robot-arm is-left">
                <i />
                <b>
                  <PlugsConnected size={15} weight="duotone" />
                </b>
              </span>
              <span className="cloud-robot-arm is-right">
                <i />
                <b>
                  <Wrench size={15} weight="duotone" />
                </b>
              </span>
              <span className="cloud-robot-leg is-left">
                <i />
              </span>
              <span className="cloud-robot-leg is-right">
                <i />
              </span>
              <span className="cloud-robot-security">
                <ShieldCheck size={18} weight="fill" />
              </span>
              <span className="cloud-robot-identity">
                <Fingerprint size={15} weight="duotone" />
              </span>
            </div>

            <span className="cloud-factory-part-flight is-model">
              <Brain size={15} weight="duotone" />
            </span>
            <span className="cloud-factory-part-flight is-harness">
              <Code size={15} weight="duotone" />
            </span>
            <span className="cloud-factory-part-flight is-mcp">
              <PlugsConnected size={15} weight="duotone" />
            </span>
            <span className="cloud-factory-part-flight is-skills">
              <Wrench size={15} weight="duotone" />
            </span>
            <span className="cloud-factory-part-flight is-security">
              <ShieldCheck size={15} weight="duotone" />
            </span>

            <div className="cloud-factory-scanner">
              <span>
                <Scan size={14} weight="duotone" />{' '}
                {zh ? '评测与签名' : 'EVALUATE & SIGN'}
              </span>
              <i />
            </div>

            <div className="cloud-factory-conveyor">
              <i />
              <i />
              <i />
              <i />
              <i />
              <i />
              <span className="cloud-factory-release-box">
                <Cube size={19} weight="duotone" />
                <b>support-agent@1.8.0</b>
                <small>sha256:7ec9…a142</small>
              </span>
            </div>
          </main>

          <aside className="cloud-factory-release-panel">
            <header>
              <SealCheck size={16} weight="duotone" />
              <span>
                <strong>{zh ? '发布检查' : 'Release gates'}</strong>
                <small>immutable build</small>
              </span>
            </header>
            <ol>
              <li className="is-gate-1">
                <i>
                  <Check size={10} weight="bold" />
                </i>
                <span>
                  <b>{zh ? '资产解析' : 'Assets'}</b>
                  <small>5 / 5</small>
                </span>
              </li>
              <li className="is-gate-2">
                <i>
                  <Check size={10} weight="bold" />
                </i>
                <span>
                  <b>{zh ? '评测通过' : 'Evaluation'}</b>
                  <small>96.4 score</small>
                </span>
              </li>
              <li className="is-gate-3">
                <i>
                  <Check size={10} weight="bold" />
                </i>
                <span>
                  <b>{zh ? '版本固定' : 'Version pin'}</b>
                  <small>digest locked</small>
                </span>
              </li>
              <li className="is-gate-4">
                <i>
                  <Check size={10} weight="bold" />
                </i>
                <span>
                  <b>{zh ? '签名完成' : 'Signature'}</b>
                  <small>identity bound</small>
                </span>
              </li>
            </ol>
            <section className="cloud-factory-release-card">
              <span>
                <Robot size={19} weight="duotone" />
              </span>
              <strong>Release 1.8.0</strong>
              <small>{zh ? '不可变 · 可审计' : 'Immutable · auditable'}</small>
              <em>
                <i /> {zh ? '就绪' : 'READY'}
              </em>
            </section>
            <section className="cloud-factory-deploy-target">
              <GearSix size={15} weight="duotone" />
              <span>
                <strong>A3S Box bx-07</strong>
                <small>Workload · running</small>
              </span>
            </section>
            <footer>
              <Fingerprint size={13} weight="duotone" />{' '}
              {zh ? '身份与证据契约已绑定' : 'Identity & evidence bound'}
            </footer>
          </aside>
        </div>
      </div>
    </figure>
  );
}
