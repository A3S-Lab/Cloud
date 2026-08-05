import {
  Broadcast,
  Browser,
  Check,
  CloudArrowUp,
  DesktopTower,
  DotsThree,
  Engine,
  Globe,
  IdentificationCard,
  LockKey,
  MagnifyingGlass,
  Network,
  Path,
  PlugsConnected,
  Pulse,
  ShieldCheck,
  Stack,
  TerminalWindow,
} from '@phosphor-icons/react';
import type { HomeLanguage } from '../../data/product';

export function GatewayStory({ language }: { language: HomeLanguage }) {
  const zh = language === 'zh';

  return (
    <figure
      className="cloud-editorial-chart cloud-product-demo cloud-gateway-story cloud-motion-scene"
      aria-labelledby="gateway-story-title"
    >
      <figcaption>
        <strong id="gateway-story-title">
          {zh
            ? '在实时拓扑中治理每一次调用'
            : 'Govern every call through a live topology'}
        </strong>
        <span>
          {zh
            ? '调用来源、网关集群、身份策略、路由目标与 AnySentry 证据在同一张网络图中关联'
            : 'Call sources, gateway clusters, identity policy, route targets, and AnySentry evidence meet in one network view'}
        </span>
      </figcaption>

      <div className="cloud-gateway-console" aria-hidden="true">
        <header className="cloud-gateway-consolebar">
          <span className="cloud-gateway-console-title">
            <Network size={16} weight="duotone" />
            <strong>{zh ? '流量拓扑' : 'Traffic topology'}</strong>
            <small>production / cn-east</small>
          </span>
          <span className="cloud-gateway-live">
            <i /> {zh ? '实时' : 'LIVE'}
          </span>
          <span className="cloud-gateway-time">
            {zh ? '最近 5 分钟' : 'Last 5 min'}⌄
          </span>
          <DotsThree size={17} weight="bold" />
        </header>

        <div className="cloud-gateway-console-body">
          <aside className="cloud-gateway-navrail">
            <span className="is-active">
              <Network size={16} weight="duotone" />
            </span>
            <span>
              <Pulse size={16} weight="duotone" />
            </span>
            <span>
              <ShieldCheck size={16} weight="duotone" />
            </span>
            <span>
              <Stack size={16} weight="duotone" />
            </span>
            <footer>
              <MagnifyingGlass size={15} />
            </footer>
          </aside>

          <main className="cloud-topology-canvas">
            <div className="cloud-topology-toolbar">
              <span>
                <i className="is-healthy" /> {zh ? '健康' : 'Healthy'} 12
              </span>
              <span>
                <i className="is-traffic" /> 1.8k req/s
              </span>
              <span>
                <i className="is-evidence" /> {zh ? '证据流' : 'Evidence'} 100%
              </span>
            </div>
            <div className="cloud-topology-stage">
              <svg
                className="cloud-topology-links"
                viewBox="0 0 440 400"
                preserveAspectRatio="none"
              >
                <path
                  className="is-source-link"
                  d="M82 72 C106 72 104 128 120 128"
                />
                <path
                  className="is-source-link"
                  d="M82 164 C105 164 105 190 120 190"
                />
                <path
                  className="is-source-link"
                  d="M82 256 C105 256 105 252 120 252"
                />
                <path
                  className="is-backbone is-path-1"
                  d="M230 128 C244 128 245 69 260 69"
                />
                <path
                  className="is-backbone is-path-2"
                  d="M230 190 C245 190 245 151 260 151"
                />
                <path
                  className="is-backbone is-path-3"
                  d="M230 252 C246 252 246 233 260 233"
                />
                <path
                  className="is-policy-link"
                  d="M304 91 C304 105 304 114 304 127"
                />
                <path
                  className="is-policy-link"
                  d="M304 173 C304 187 304 196 304 209"
                />
                <path
                  className="is-policy-link"
                  d="M304 255 C304 269 304 279 304 291"
                />
                <path
                  className="is-target-link is-main-route"
                  d="M348 315 C377 315 348 115 362 115"
                />
                <path
                  className="is-target-link"
                  d="M348 315 C377 315 350 249 362 249"
                />
                <path
                  className="is-evidence-link"
                  d="M304 337 C304 365 343 365 362 365"
                />
              </svg>

              <span className="cloud-topology-group-label is-sources">
                {zh ? '调用来源' : 'CALL SOURCES'}
              </span>
              <article className="cloud-topology-endpoint is-work">
                <Browser size={17} weight="duotone" />
                <span>
                  <strong>A3S Work</strong>
                  <small>124 req/s</small>
                </span>
              </article>
              <article className="cloud-topology-endpoint is-api">
                <TerminalWindow size={17} weight="duotone" />
                <span>
                  <strong>Agent / MCP</strong>
                  <small>860 req/s</small>
                </span>
              </article>
              <article className="cloud-topology-endpoint is-edge">
                <DesktopTower size={17} weight="duotone" />
                <span>
                  <strong>{zh ? '端侧节点' : 'Edge nodes'}</strong>
                  <small>816 req/s</small>
                </span>
              </article>

              <section className="cloud-topology-gateway-cluster">
                <header>
                  <Broadcast size={15} weight="duotone" />
                  <strong>A3S Gateway</strong>
                  <small>3 / 3</small>
                </header>
                <div className="is-gw-1">
                  <i />
                  <span>
                    <b>gw-01</b>
                    <small>32%</small>
                  </span>
                </div>
                <div className="is-gw-2">
                  <i />
                  <span>
                    <b>gw-02</b>
                    <small>34%</small>
                  </span>
                </div>
                <div className="is-gw-3">
                  <i />
                  <span>
                    <b>gw-03</b>
                    <small>34%</small>
                  </span>
                </div>
              </section>

              <span className="cloud-topology-group-label is-policy">
                {zh ? '请求策略链' : 'REQUEST POLICY'}
              </span>
              <article className="cloud-topology-policy is-identity">
                <IdentificationCard size={16} weight="duotone" />
                <span>
                  <strong>{zh ? '身份' : 'Identity'}</strong>
                  <small>tenant · mTLS</small>
                </span>
                <Check size={12} weight="bold" />
              </article>
              <article className="cloud-topology-policy is-protocol">
                <PlugsConnected size={16} weight="duotone" />
                <span>
                  <strong>{zh ? '协议' : 'Protocol'}</strong>
                  <small>MCP · HTTP · A3S</small>
                </span>
                <Check size={12} weight="bold" />
              </article>
              <article className="cloud-topology-policy is-policy">
                <LockKey size={16} weight="duotone" />
                <span>
                  <strong>ACL · {zh ? '配额' : 'Quota'}</strong>
                  <small>allow · 240 rpm</small>
                </span>
                <Check size={12} weight="bold" />
              </article>
              <article className="cloud-topology-policy is-route">
                <Path size={16} weight="duotone" />
                <span>
                  <strong>{zh ? '路由策略' : 'Routing'}</strong>
                  <small>latency aware</small>
                </span>
                <Check size={12} weight="bold" />
              </article>

              <span className="cloud-topology-group-label is-targets">
                {zh ? '服务目标' : 'TARGETS'}
              </span>
              <article className="cloud-topology-target is-runtime">
                <Engine size={18} weight="duotone" />
                <span>
                  <strong>A3S Runtime</strong>
                  <small>stream 200 · 42ms</small>
                </span>
              </article>
              <article className="cloud-topology-target is-box">
                <CloudArrowUp size={18} weight="duotone" />
                <span>
                  <strong>A3S Box</strong>
                  <small>edge-cn-07 · 28ms</small>
                </span>
              </article>
              <article className="cloud-topology-target is-sentry">
                <Pulse size={18} weight="duotone" />
                <span>
                  <strong>AnySentry</strong>
                  <small>
                    {zh ? '轨迹与证据回流' : 'Trace & evidence return'}
                  </small>
                </span>
              </article>

              <i className="cloud-topology-packet is-request" />
              <i className="cloud-topology-packet is-evidence" />
            </div>
          </main>

          <aside className="cloud-gateway-inspector">
            <header>
              <span>
                <Globe size={15} weight="duotone" />
                <strong>req_01J8</strong>
              </span>
              <small>42 ms</small>
            </header>
            <section className="cloud-gateway-request-info">
              <label>{zh ? '当前调用' : 'Current call'}</label>
              <strong>POST /agents/run</strong>
              <code>tenant_acme</code>
            </section>
            <ol>
              <li className="is-hop-1">
                <i />
                <span>
                  <b>{zh ? '接入' : 'Ingress'}</b>
                  <small>gw-02 · 4ms</small>
                </span>
              </li>
              <li className="is-hop-2">
                <i />
                <span>
                  <b>{zh ? '身份' : 'Identity'}</b>
                  <small>verified · 7ms</small>
                </span>
              </li>
              <li className="is-hop-3">
                <i />
                <span>
                  <b>{zh ? '策略' : 'Policy'}</b>
                  <small>allowed · 3ms</small>
                </span>
              </li>
              <li className="is-hop-4">
                <i />
                <span>
                  <b>{zh ? '路由' : 'Route'}</b>
                  <small>runtime-07 · 28ms</small>
                </span>
              </li>
            </ol>
            <section className="cloud-gateway-trace">
              <Pulse size={14} weight="duotone" />
              <span>
                <strong>trace gw_01J8…</strong>
                <small>
                  {zh ? 'AnySentry 已接收' : 'Received by AnySentry'}
                </small>
              </span>
            </section>
            <footer>
              <ShieldCheck size={13} weight="duotone" />{' '}
              {zh ? '证据完整' : 'Evidence complete'}
            </footer>
          </aside>
        </div>
      </div>
    </figure>
  );
}
