import { withBase } from '@rspress/core/runtime';
import {
  ArrowSquareOut,
  Check,
  Code,
  CursorClick,
  DotsThree,
  FolderSimple,
  House,
  List,
  MagnifyingGlass,
  PaperPlaneTilt,
  Play,
  Pulse,
  Robot,
  ShieldCheck,
  Sparkle,
  SquaresFour,
  User,
  X,
} from '@phosphor-icons/react';
import type { HomeLanguage } from '../../data/product';

const callIcons = [List, MagnifyingGlass, Code, Play];

export function HeroVisual({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';
  const calls = [
    {
      action: 'list',
      request: 'scope: runtime',
      result: zh ? '42 个授权能力' : '42 allowed capabilities',
    },
    {
      action: 'search',
      request: 'query: deploy agent',
      result: 'runtime.agent.deploy',
    },
    {
      action: 'describe',
      request: 'operation: runtime.agent.deploy',
      result: 'package · release · shaped',
    },
    {
      action: 'execute',
      request: 'shaped: true',
      result: '.view · req_01J8X4',
    },
  ];

  return (
    <figure
      className="cloud-hero-scene cloud-work-demo cloud-motion-scene"
      aria-label={
        zh
          ? 'A3S Work 通过渐进式 API 调用 A3S OS，并由用户打开渐进式 UI'
          : 'A3S Work calls A3S OS through the Progressive API and lets the user open the Progressive UI'
      }
    >
      <div className="cloud-work-app">
        <header className="cloud-work-appbar">
          <span className="cloud-work-app-brand">
            <img alt="" src={withBase('/a3s-os-logo.png')} />
            <strong>A3S Work</strong>
          </span>
          <span className="cloud-work-app-search">
            <MagnifyingGlass size={14} />
            {zh ? '搜索任务、资产和能力' : 'Search tasks, assets, capabilities'}
            <kbd>⌘ K</kbd>
          </span>
          <span className="cloud-work-app-actions">
            <span className="cloud-work-online">
              <i /> A3S OS
            </span>
            <DotsThree size={18} weight="bold" />
          </span>
        </header>

        <div className="cloud-work-layout" aria-hidden="true">
          <aside className="cloud-work-sidebar">
            <span className="cloud-work-new-task">
              <Sparkle size={14} weight="fill" />
              {zh ? '新任务' : 'New task'}
            </span>
            <nav>
              <span className="is-active">
                <House size={15} weight="duotone" />
                {zh ? '工作台' : 'Workspace'}
              </span>
              <span>
                <SquaresFour size={15} weight="duotone" />
                {zh ? '应用' : 'Apps'}
              </span>
              <span>
                <FolderSimple size={15} weight="duotone" />
                {zh ? '资产' : 'Assets'}
              </span>
            </nav>
            <div className="cloud-work-recents">
              <small>{zh ? '最近任务' : 'RECENT'}</small>
              <span className="is-current">
                <i />
                <b>{zh ? '部署客服 Agent' : 'Deploy service Agent'}</b>
              </span>
              <span>
                <i />
                <b>{zh ? '分析工单趋势' : 'Analyze ticket trend'}</b>
              </span>
              <span>
                <i />
                <b>{zh ? '更新知识资产' : 'Update knowledge assets'}</b>
              </span>
            </div>
            <footer>
              <span>
                <User size={14} weight="duotone" />
              </span>
              <b>Roy · Platform</b>
            </footer>
          </aside>

          <section className="cloud-work-thread">
            <header>
              <span>
                <strong>
                  {zh ? '部署客服 Agent' : 'Deploy service Agent'}
                </strong>
                <small>
                  {zh ? '任务会话 · 已授权' : 'TASK SESSION · AUTHORIZED'}
                </small>
              </span>
              <DotsThree size={17} weight="bold" />
            </header>

            <div className="cloud-work-messages">
              <article className="cloud-work-user-bubble">
                <span>
                  <User size={14} weight="duotone" />
                </span>
                <p>
                  {zh
                    ? '部署客服 Agent 的最新资产包，并打开运行视图。'
                    : 'Deploy the latest service Agent package and open its run view.'}
                </p>
              </article>

              <article className="cloud-work-assistant">
                <span className="cloud-work-assistant-avatar">
                  <Sparkle size={15} weight="fill" />
                </span>
                <div>
                  <header>
                    <strong>A3S Work</strong>
                    <small>{zh ? '正在调用 A3S OS' : 'CALLING A3S OS'}</small>
                  </header>
                  <p>
                    {zh
                      ? '已按当前用户权限发现并执行部署能力。'
                      : 'Deployment capability discovered and executed within current user permissions.'}
                  </p>
                  <section className="cloud-work-capability-run">
                    <header>
                      <span>
                        <Code size={14} weight="duotone" /> Progressive API
                      </span>
                      <code>POST /api/v1/kernel/capabilities</code>
                    </header>
                    <ol>
                      {calls.map(({ action, request, result }, index) => {
                        const Icon = callIcons[index];
                        return (
                          <li className={`is-call-${index + 1}`} key={action}>
                            <span className="cloud-work-call-icon">
                              <Icon size={13} weight="duotone" />
                            </span>
                            <code>{action}</code>
                            <small>{request}</small>
                            <em>{result}</em>
                            <Check size={13} weight="bold" />
                          </li>
                        );
                      })}
                    </ol>
                  </section>
                </div>
              </article>

              <article className="cloud-work-view-ready">
                <span>
                  <Robot size={17} weight="duotone" />
                </span>
                <div>
                  <strong>
                    {zh ? '部署视图已生成' : 'Deployment view is ready'}
                  </strong>
                  <small>customer-care@1.8.0 · .view</small>
                </div>
                <span className="cloud-work-open-view">
                  <ArrowSquareOut size={14} weight="bold" /> Open view
                </span>
                <CursorClick
                  className="cloud-work-open-cursor"
                  size={19}
                  weight="fill"
                />
              </article>
            </div>

            <footer className="cloud-work-composer">
              <span>{zh ? '继续输入任务…' : 'Continue with a task…'}</span>
              <i>
                <PaperPlaneTilt size={14} weight="fill" />
              </i>
            </footer>
          </section>

          <div className="cloud-work-view-overlay">
            <aside className="cloud-work-view-modal">
              <header>
                <span>
                  <strong>{zh ? '部署运行视图' : 'Deployment run view'}</strong>
                  <small>Progressive UI · .view</small>
                </span>
                <span className="cloud-work-modal-actions">
                  <i>{zh ? '实时' : 'LIVE'}</i>
                  <b>
                    <X size={13} weight="bold" />
                  </b>
                </span>
              </header>
              <section className="cloud-work-agent-summary">
                <span>
                  <Robot size={22} weight="duotone" />
                </span>
                <div>
                  <strong>{zh ? '客服 Agent' : 'Service Agent'}</strong>
                  <small>customer-care@1.8.0</small>
                </div>
                <em>
                  <i /> {zh ? '运行中' : 'RUNNING'}
                </em>
              </section>
              <div className="cloud-work-deploy-progress">
                <header>
                  <strong>{zh ? '部署进度' : 'Deployment'}</strong>
                  <span>4 / 4</span>
                </header>
                <ol>
                  <li className="is-step-1">
                    <i>
                      <Check size={11} weight="bold" />
                    </i>
                    <span>
                      <b>{zh ? '解析资产包' : 'Resolve package'}</b>
                      <small>asset://customer-care@1.8.0</small>
                    </span>
                  </li>
                  <li className="is-step-2">
                    <i>
                      <Check size={11} weight="bold" />
                    </i>
                    <span>
                      <b>{zh ? '校验策略' : 'Verify policy'}</b>
                      <small>identity · ACL · signature</small>
                    </span>
                  </li>
                  <li className="is-step-3">
                    <i>
                      <Check size={11} weight="bold" />
                    </i>
                    <span>
                      <b>{zh ? '部署工作负载' : 'Deploy workload'}</b>
                      <small>A3S Runtime · Box bx-07</small>
                    </span>
                  </li>
                  <li className="is-step-4">
                    <i>
                      <Pulse size={11} weight="fill" />
                    </i>
                    <span>
                      <b>{zh ? '接入运行证据' : 'Attach evidence'}</b>
                      <small>AnySentry · trace live</small>
                    </span>
                  </li>
                </ol>
              </div>
              <section className="cloud-work-evidence-strip">
                <ShieldCheck size={16} weight="duotone" />
                <span>
                  <strong>req_01J8X4</strong>
                  <small>
                    {zh
                      ? '身份、轨迹与证据已绑定'
                      : 'Identity, trace, and evidence bound'}
                  </small>
                </span>
              </section>
              <footer>
                <span>
                  <i /> Runtime
                </span>
                <span>
                  <i /> Gateway
                </span>
                <span>
                  <i /> AnySentry
                </span>
              </footer>
            </aside>
          </div>
        </div>
      </div>
    </figure>
  );
}
