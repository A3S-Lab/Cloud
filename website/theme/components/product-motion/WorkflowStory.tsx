import {
  ArrowsClockwise,
  Brain,
  Check,
  CirclesThreePlus,
  Database,
  DotsThree,
  Flag,
  GitBranch,
  MagnifyingGlass,
  Minus,
  Play,
  Plus,
  Robot,
  SlidersHorizontal,
  User,
  Wrench,
} from '@phosphor-icons/react';
import type { HomeLanguage } from '../../data/product';

export function WorkflowStory({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <figure
      className="cloud-editorial-chart cloud-product-demo cloud-workflow-story cloud-motion-scene"
      aria-labelledby="workflow-story-title"
    >
      <figcaption>
        <strong id="workflow-story-title">
          {zh
            ? '在可视画布中编排可恢复工作流'
            : 'Orchestrate recoverable workflows on a visual canvas'}
        </strong>
        <span>
          {zh
            ? '本体检索、Agent、条件分支与人工审批共享状态，A3S Runtime 从检查点继续执行'
            : 'Ontology retrieval, Agents, branches, and human approval share state while A3S Runtime resumes from checkpoints'}
        </span>
      </figcaption>

      <div className="cloud-flow-app" aria-hidden="true">
        <header className="cloud-flow-appbar">
          <span className="cloud-flow-breadcrumb">
            <strong>{zh ? '退款审批' : 'Refund approval'}</strong>
            <small>Production · v18</small>
          </span>
          <span className="cloud-flow-save">
            <i /> {zh ? '已保存' : 'Saved'}
          </span>
          <span className="cloud-flow-run-button">
            <Play size={13} weight="fill" /> {zh ? '运行' : 'Run'}
          </span>
          <DotsThree size={17} weight="bold" />
        </header>

        <div className="cloud-flow-app-body">
          <aside className="cloud-flow-library">
            <header>
              <strong>{zh ? '节点' : 'Nodes'}</strong>
              <MagnifyingGlass size={13} />
            </header>
            <div>
              <span>
                <Flag size={15} weight="duotone" />
                <b>{zh ? '开始' : 'Start'}</b>
              </span>
              <span>
                <Database size={15} weight="duotone" />
                <b>{zh ? '知识' : 'Knowledge'}</b>
              </span>
              <span>
                <Robot size={15} weight="duotone" />
                <b>Agent</b>
              </span>
              <span>
                <GitBranch size={15} weight="duotone" />
                <b>{zh ? '条件' : 'Branch'}</b>
              </span>
              <span>
                <User size={15} weight="duotone" />
                <b>{zh ? '审批' : 'Human'}</b>
              </span>
              <span>
                <Wrench size={15} weight="duotone" />
                <b>{zh ? '工具' : 'Tool'}</b>
              </span>
            </div>
            <footer>
              <Plus size={13} weight="bold" /> {zh ? '更多节点' : 'More'}
            </footer>
          </aside>

          <main className="cloud-flow-canvas">
            <div className="cloud-flow-canvas-toolbar">
              <span>
                <CirclesThreePlus size={14} weight="duotone" />{' '}
                {zh ? '自动布局' : 'Auto layout'}
              </span>
              <span>
                <Minus size={12} />
                <b>82%</b>
                <Plus size={12} />
              </span>
            </div>
            <div className="cloud-flow-canvas-stage">
              <svg
                className="cloud-flow-edges"
                viewBox="0 0 400 390"
                preserveAspectRatio="none"
              >
                <path className="is-edge-1" d="M102 70 C120 70 122 70 142 70" />
                <path className="is-edge-2" d="M250 70 C262 70 264 70 275 70" />
                <path
                  className="is-edge-3"
                  d="M325 111 C325 137 253 128 253 160"
                />
                <path
                  className="is-edge-4"
                  d="M252 205 C263 205 266 205 275 205"
                />
                <path
                  className="is-edge-muted"
                  d="M197 205 C112 205 112 318 142 318"
                />
                <path
                  className="is-edge-5"
                  d="M327 249 C327 276 253 277 253 286"
                />
                <path
                  className="is-edge-6"
                  d="M252 327 C263 327 267 327 278 327"
                />
              </svg>

              <article className="cloud-flow-node is-start">
                <i className="cloud-flow-node-icon">
                  <Flag size={15} weight="duotone" />
                </i>
                <span>
                  <small>{zh ? '触发器' : 'TRIGGER'}</small>
                  <strong>{zh ? '开始' : 'Start'}</strong>
                </span>
                <i className="cloud-flow-port is-output" />
              </article>

              <article className="cloud-flow-node is-ontology">
                <i className="cloud-flow-port is-input" />
                <i className="cloud-flow-node-icon">
                  <Database size={15} weight="duotone" />
                </i>
                <span>
                  <small>{zh ? '本体知识图谱' : 'ONTOLOGY'}</small>
                  <strong>
                    {zh ? '检索订单上下文' : 'Retrieve order context'}
                  </strong>
                </span>
                <em>4 facts</em>
                <i className="cloud-flow-port is-output" />
              </article>

              <article className="cloud-flow-node is-agent">
                <i className="cloud-flow-port is-input" />
                <i className="cloud-flow-node-icon">
                  <Robot size={15} weight="duotone" />
                </i>
                <span>
                  <small>AGENT</small>
                  <strong>{zh ? '风险评估' : 'Risk assessment'}</strong>
                </span>
                <em>Qwen3-32B</em>
                <i className="cloud-flow-port is-output-bottom" />
              </article>

              <article className="cloud-flow-node is-condition">
                <i className="cloud-flow-port is-input-top" />
                <i className="cloud-flow-node-icon">
                  <GitBranch size={15} weight="duotone" />
                </i>
                <span>
                  <small>{zh ? '条件分支' : 'CONDITION'}</small>
                  <strong>amount &gt; 10,000</strong>
                </span>
                <em>TRUE</em>
                <i className="cloud-flow-port is-output" />
                <i className="cloud-flow-port is-output-bottom" />
              </article>

              <article className="cloud-flow-node is-approval">
                <i className="cloud-flow-port is-input" />
                <i className="cloud-flow-node-icon">
                  <User size={15} weight="duotone" />
                </i>
                <span>
                  <small>{zh ? '人工审批' : 'HUMAN APPROVAL'}</small>
                  <strong>{zh ? '财务负责人' : 'Finance owner'}</strong>
                </span>
                <em>
                  <b>{zh ? '等待中' : 'WAITING'}</b>
                  <b>
                    <Check size={10} weight="bold" />{' '}
                    {zh ? '已批准' : 'APPROVED'}
                  </b>
                </em>
                <i className="cloud-flow-port is-output-bottom" />
              </article>

              <article className="cloud-flow-node is-resume">
                <i className="cloud-flow-port is-input-top" />
                <i className="cloud-flow-node-icon">
                  <ArrowsClockwise size={15} weight="duotone" />
                </i>
                <span>
                  <small>A3S RUNTIME</small>
                  <strong>{zh ? '从检查点恢复' : 'Resume checkpoint'}</strong>
                </span>
                <em>CP-03</em>
                <i className="cloud-flow-port is-output" />
              </article>

              <article className="cloud-flow-node is-finish">
                <i className="cloud-flow-port is-input" />
                <i className="cloud-flow-node-icon">
                  <Check size={15} weight="bold" />
                </i>
                <span>
                  <small>{zh ? '输出' : 'OUTPUT'}</small>
                  <strong>{zh ? '退款完成' : 'Refund issued'}</strong>
                </span>
              </article>

              <span className="cloud-flow-run-cursor">
                <Play size={10} weight="fill" />
              </span>
            </div>
          </main>

          <aside className="cloud-flow-inspector">
            <header>
              <span>
                <Robot size={15} weight="duotone" />
                <strong>{zh ? '风险评估' : 'Risk assessment'}</strong>
              </span>
              <DotsThree size={15} weight="bold" />
            </header>
            <section>
              <label>{zh ? '模型' : 'Model'}</label>
              <span>
                Qwen3-32B <b>⌄</b>
              </span>
            </section>
            <section>
              <label>{zh ? '输入变量' : 'Input variables'}</label>
              <code>{'{{ ontology.order }}'}</code>
              <code>{'{{ start.requester }}'}</code>
            </section>
            <section>
              <label>{zh ? '运行策略' : 'Run policy'}</label>
              <span>
                <SlidersHorizontal size={12} /> retry 3 · timeout 90s
              </span>
            </section>
            <section className="cloud-flow-inspector-output">
              <label>{zh ? '最近输出' : 'Latest output'}</label>
              <strong>risk_level: A</strong>
              <small>confidence: 0.96</small>
            </section>
            <footer>
              <Brain size={13} weight="duotone" />{' '}
              {zh ? '本体上下文已注入' : 'Ontology context attached'}
            </footer>
          </aside>
        </div>
      </div>
    </figure>
  );
}
