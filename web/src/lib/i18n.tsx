import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from 'react';

export type Language = 'zh-CN' | 'en';

const LANGUAGE_KEY = 'a3s-cloud.language';

const ZH_CN: Record<string, string> = {
  English: '英文',
  Language: '语言',
  'Self-hosted control plane': '自托管控制平面',
  'Control plane': '控制平面',
  'A3S-native operations': 'A3S 原生运维',
  'Operate Agents on infrastructure you own.': '在自有基础设施上运行 Agent。',
  'Deploy applications and run Agents through one durable control plane for delivery, execution, routing, and authoritative evidence.':
    '通过统一、持久化的控制平面交付应用并运行 Agent，覆盖交付、执行、路由与权威证据。',
  'Platform trust boundaries': '平台信任边界',
  'Scoped identity': '范围化身份',
  'Operator-owned nodes': '自主管理节点',
  'Durable audit trail': '持久审计轨迹',
  'One control path': '统一控制路径',
  'Cloud orchestrates. Existing A3S authorities execute.': 'Cloud 负责编排，既有 A3S 权威组件负责执行。',
  'Live architecture': '实时架构',
  'Intent, identity, and policy': '意图、身份与策略',
  'Operations + A3S Flow': 'Operations + A3S Flow',
  'Durable orchestration': '持久化编排',
  'Outbound-only Node Agent': '仅出站 Node Agent',
  'Typed command delivery': '类型化命令投递',
  'Execution and isolation': '执行与隔离',
  'Agent execution providers': 'Agent 执行 Provider',
  'One provider-neutral contract': '统一的 Provider 中立合约',
  'Sign in to A3S OS': '登录 A3S OS',
  'Open A3S Web': '进入 A3S Web',
  'The credential remains in this browser tab.': '凭据仅保留在当前浏览器标签页。',
  'Organization API token': '组织 API Token',
  'Sent only as a Bearer credential to the configured Cloud API.':
    '仅作为 Bearer 凭据发送到已配置的 Cloud API。',
  'Verifying...': '正在验证...',
  'Desired state': '期望状态',
  'PostgreSQL authority': 'PostgreSQL 权威状态源',
  'Durable operations': '持久化操作',
  'Flow-backed recovery': '由 Flow 驱动恢复',
  'Outbound nodes': '仅出站节点',
  'No inbound management ports': '无需开放入站管理端口',
  'Managed reachability': '托管可达性',
  'Gateway policy and evidence': 'Gateway 策略与证据',
  'Enter an organization API token.': '请输入组织 API Token。',
  'This token has no visible organization.': '该 Token 无权访问任何组织。',
  'Cloud could not verify this token.': 'Cloud 无法验证该 Token。',
  Overview: '概览',
  'Workspace health': '工作区健康度',
  Workloads: '工作负载',
  'Runtime convergence': '运行时收敛',
  Agents: 'Agent',
  'Conversations and runs': '会话与运行',
  Delivery: '交付',
  'Builds and evidence': '构建与证据',
  Edge: '边缘',
  'Routes and TLS': '路由与 TLS',
  Architecture: '架构',
  'Platform module map': '平台模块图',
  'active operations': '个活跃操作',
  workloads: '个工作负载',
  conversations: '个会话',
  'build runs': '个构建运行',
  routes: '条路由',
  'Environment sections': '环境功能区',
  'Environment workspace': '环境工作区',
  'Cloud context': 'Cloud 上下文',
  Organization: '组织',
  Project: '项目',
  Environment: '环境',
  'None yet': '暂无',
  Live: '实时',
  Reconnecting: '正在重连',
  Connecting: '正在连接',
  Idle: '空闲',
  'Close operations': '关闭操作面板',
  'Open operations': '打开操作面板',
  'Sign out': '退出登录',
  Retry: '重试',
  'Choose an organization, project, and environment first.': '请先选择组织、项目和环境。',
  'Cloud state could not be loaded.': '无法加载 Cloud 状态。',
  '{name} workspace': '{name} 工作区',
  'Choose a project and environment to inspect its desired state.': '请选择项目和环境以查看其期望状态。',
  Operations: '操作',
  '{count} active': '{count} 个活跃',
  '{count} observed': '已观测 {count} 个',
  Authoritative: '权威状态源',
  Infrastructure: '基础设施',
  'Current control and execution ownership': '当前控制与执行权属',
  Runtime: '运行时',
  'Operation authority': '操作权威组件',
  '{active}/{total} active': '{active}/{total} 活跃',
  'A3S assets': 'A3S 资产',
  'Immutable Agent, MCP, and Skill releases': '不可变的 Agent、MCP 与 Skill 发布',
  'Published Agent releases deploy through immutable Workload bindings. Yanked releases remain available to pinned deployments.':
    '已发布的 Agent 通过不可变工作负载绑定进行部署；已撤回版本仍可供固定版本的部署使用。',
  '{count} asset': '{count} 个资产',
  '{count} assets': '{count} 个资产',
  '{count} published': '{count} 个已发布',
  '{count} draft': '{count} 个草稿',
  '{count} yanked': '{count} 个已撤回',
  'Desired state is converged': '期望状态已收敛',
  'Convergence is in progress': '正在收敛',
  'No active operation is changing the selected environment.': '当前没有活跃操作正在变更所选环境。',
  '{count} durable operation currently active.': '当前有 {count} 个持久化操作正在运行。',
  '{count} durable operations currently active.': '当前有 {count} 个持久化操作正在运行。',
  'Active operations': '活跃操作',
  'Build runs': '构建运行',
  'Active routes': '活跃路由',
  'Current operations': '当前操作',
  'Latest durable workflow state for this organization': '该组织最新的持久化工作流状态',
  '{count} total': '共 {count} 个',
  'No operations recorded': '暂无操作记录',
  'Accepted mutations and their terminal evidence will appear here.':
    '已接受的变更及其终态证据将显示在这里。',
  'Authority and runtime path': '权威边界与运行路径',
  'One control route from accepted intent to execution evidence': '从已接受意图到执行证据的统一控制路径',
  'A3S OS control': 'A3S OS 控制层',
  'Durable orchestration and recovery': '持久化编排与恢复',
  'Placement, revisions, and convergence': '调度、修订与收敛',
  'Leases, Claims, commands, and receipts': '租约、Claim、命令与回执',
  'Task, Service, build, and isolation': 'Task、Service、构建与隔离',
  'Applied request-path policy': '已应用的请求路径策略',
  'A3S OS architecture': 'A3S OS 架构',
  'Export PNG': '导出 PNG',
  'Exporting...': '正在导出...',
  'PNG is generated from this live HTML diagram.': 'PNG 由当前 HTML 架构图实时生成。',
  'Rendering the architecture PNG...': '正在渲染架构 PNG...',
  'Architecture PNG exported.': '架构 PNG 已导出。',
  'Architecture PNG export failed.': '架构 PNG 导出失败。',
  'Scrollable A3S OS architecture diagram': '可滚动的 A3S OS 架构图',
  'Module architecture': '模块架构',
  'Complete module architecture': '完整模块架构',
  'Roadmap snapshot': '路线图快照',
  'Control and execution authority': '控制与执行权威边界',
  'Users and application scenarios': '用户与应用场景',
  'Agent applications': 'Agent 应用',
  'Application services': '应用服务',
  Developers: '开发者',
  'Platform operators': '平台运维人员',
  Automation: '自动化系统',
  'Enterprise automation': '企业自动化',
  'Enterprise integration': '企业集成',
  'External application products': '对外应用产品层',
  'Built on': '基于',
  'Unified access and experience': '统一访问与体验',
  'Web console': 'Web 控制台',
  'Management MCP': '管理 MCP',
  'Cloud orchestration and control': 'Cloud 编排与控制',
  'Commands / Queries': '命令 / 查询',
  'PostgreSQL desired state': 'PostgreSQL 期望状态',
  'Workloads / Fleet': 'Workloads / Fleet',
  'Workloads scheduling': '工作负载调度',
  'Cloud business modules': 'Cloud 业务模块',
  'Complete Cloud product portfolio': '完整 Cloud 产品能力',
  'Roadmap state legend': '路线图状态图例',
  'Platform and resources': '平台与资源',
  'Identity / Tenants': '身份 / 租户',
  'Projects / Environments': '项目 / 环境',
  'Assets / Artifacts': '资产 / 制品',
  'Search / Audit': '搜索 / 审计',
  'Plugins (planned)': '插件（规划中）',
  'Delivery and services': '交付与服务',
  'Sources / Builds': '源码 / 构建',
  'Generic Executions': '通用执行',
  'Workloads / Deployments': '工作负载 / 部署',
  Secrets: '密钥',
  'Edge / Gateway': '边缘 / Gateway',
  'Data / Inference (planned)': '数据 / 推理（规划中）',
  'Agent platform': 'Agent 平台',
  'Agent Release': 'Agent 发布',
  'Conversations / Executions': '会话 / 执行',
  'Semantic event stream': '语义事件流',
  'Skill / MCP bindings': 'Skill / MCP 绑定',
  'Approvals / checkpoints (planned)': '审批 / 检查点（规划中）',
  'Node and runtime plane': '节点与运行时平面',
  'Node convergence and execution plane': '节点收敛与执行平面',
  'Fleet node_commands': 'Fleet 节点命令',
  'Leases / Claims / receipts': '租约 / Claim / 回执',
  'A3S Runtime Task / Service': 'A3S Runtime Task / Service',
  'Runtime payloads': '运行时载荷',
  'Runtime services and payload ownership': '运行时服务与载荷权属',
  'Application / MCP': '应用 / MCP',
  'Applications / Hosted MCP': '应用 / 托管 MCP',
  'A3S Code Core / Native Agent execution provider': 'A3S Code Core / 原生 Agent 执行 Provider',
  'One Cloud lifecycle and provider conformance contract': '统一的 Cloud 生命周期与 Provider 一致性合约',
  'A3S Power (planned)': 'A3S Power（规划中）',
  'A3S Power / Inference planned': 'A3S Power / 推理计划中',
  'Infrastructure and trust boundaries': '基础设施与信任边界',
  'The complete 19-gate portfolio shares one control path from A3S OS intent to Runtime, Gateway, and one provider-neutral Agent execution contract.':
    '完整的 19 个产品 Gate 共用一条从 A3S OS 意图到 Runtime、Gateway 与统一 Provider 中立 Agent 执行合约的控制路径。',
  'Immutable objects + fenced mutable volumes': '不可变对象 + 带 Fencing 的可变卷',
  'Compatibility lock': '兼容性锁定',
  'Desired and observed state': '期望状态与观测状态',
  'Select an environment': '请选择环境',
  'No workloads in this environment': '该环境暂无工作负载',
  'Create a digest-bound Service deployment to start convergence.': '创建绑定摘要的 Service 部署以开始收敛。',
  'No desired revision': '无期望修订',
  Desired: '期望',
  Observed: '观测',
  Operation: '操作',
  'No health': '无健康状态',
  'No evidence': '无证据',
  'Generation {generation}': '第 {generation} 代',
  None: '无',
  Convergence: '收敛',
  'Deployment state': '部署状态',
  'Awaiting workload': '等待工作负载',
  'Requesting...': '正在请求...',
  Cancel: '取消',
  'Stopping...': '正在停止...',
  Stop: '停止',
  'Deployment convergence stages': '部署收敛阶段',
  'Desired revision': '期望修订',
  'Active revision': '活跃修订',
  'Observed generation': '观测代次',
  'Runtime / health': '运行时 / 健康状态',
  'Release binding': '发布绑定',
  'Not reported': '未上报',
  'Not observed': '未观测',
  'A deployment appears here only after its committed operation is observable.':
    '只有在已提交操作可观测后，部署才会显示在这里。',
  'Runtime apply': '运行时应用',
  'Health proof': '健康证明',
  Complete: '完成',
  'Not requested': '未请求',
  Pending: '等待中',
  'Stop requested': '已请求停止',
  'The active revision remains selected until Runtime reports stopped or absent.':
    '在 Runtime 上报已停止或不存在之前，活跃修订仍保持选中。',
  'Workload stopped': '工作负载已停止',
  'Runtime stop evidence was persisted and no active revision remains selected.':
    'Runtime 停止证据已持久化，当前无活跃修订。',
  'Cancellation requested': '已请求取消',
  'The operation is checking whether a Runtime child must be stopped.':
    '操作正在检查是否需要停止 Runtime 子任务。',
  'Runtime cleanup pending': '等待 Runtime 清理',
  'The operation remains non-terminal until stopped or absent Runtime evidence is persisted.':
    '在已停止或不存在的 Runtime 证据持久化之前，操作保持非终态。',
  'Cleanup could not be proven': '无法证明清理完成',
  'Operator action is required because the Runtime child may still exist.':
    'Runtime 子任务可能仍然存在，需要运维人员处理。',
  'Cancellation complete': '取消完成',
  'No active Runtime child remains for this deployment.': '该部署已无活跃 Runtime 子任务。',
  'Ordinary Workload': '普通工作负载',
  'Immutable history': '不可变历史',
  'Deployment timeline': '部署时间线',
  'No deployment projection': '暂无部署投影',
  'Committed generations appear here with their observed operation state.':
    '已提交代次及其观测操作状态将显示在这里。',
  Current: '当前',
  'Rollback from {source}': '从 {source} 回滚',
  'Source {source} · build {build}': '源码 {source} · 构建 {build}',
  'Agent release {release} / build {build}': 'Agent 发布 {release} / 构建 {build}',
  'MCP release {release} / profile {profile}': 'MCP 发布 {release} / 配置 {profile}',
  'Skills {skills}': 'Skills {skills}',
  Requested: '请求时间',
  Activated: '激活时间',
  Node: '节点',
  'Not scheduled': '未调度',
  'Authoritative edge projection': '权威边缘投影',
  'Routes and certificates': '路由与证书',
  'Route and certificate state': '路由与证书状态',
  'No route projection': '暂无路由投影',
  'Reachability appears only after Cloud owns a route for this workload.':
    '只有 Cloud 为该工作负载持有路由后，可达性才会显示。',
  'Gateway node': 'Gateway 节点',
  'Gateway revision': 'Gateway 修订',
  'Not acknowledged': '未确认',
  Snapshot: '快照',
  'Not published': '未发布',
  'No managed certificate bound': '未绑定托管证书',
  'This route projection does not reference a Gateway certificate.': '该路由投影未引用 Gateway 证书。',
  'Certificate projection unavailable': '证书投影不可用',
  'Referenced certificate {id} is absent from this snapshot.': '当前快照中不存在引用的证书 {id}。',
  'Fingerprint {fingerprint} · expires {expires}': '指纹 {fingerprint} · 到期时间 {expires}',
  'Not issued': '未签发',
  'Agent inputs': 'Agent 输入',
  'Skill bindings': 'Skill 绑定',
  'Each change creates a new immutable Agent workload revision. Skill bundles are mounted read-only and are never scheduled as separate services.':
    '每次变更都会创建新的不可变 Agent 工作负载修订。Skill 包以只读方式挂载，不会作为独立服务调度。',
  'Create a new revision without this Skill': '创建不包含该 Skill 的新修订',
  'Unbinding...': '正在解绑...',
  Unbind: '解绑',
  'No Skill release is bound to this revision.': '该修订未绑定任何 Skill 发布。',
  'Skill Asset': 'Skill 资产',
  'No active Skill Assets': '暂无活跃 Skill 资产',
  'Published release': '已发布版本',
  'No published releases': '暂无已发布版本',
  'Binding...': '正在绑定...',
  'Already bound': '已绑定',
  'Bind release': '绑定发布',
  'Skill bindings unlock after the desired Agent revision is the active deployment':
    '期望 Agent 修订成为活跃部署后，才可变更 Skill 绑定',
  'Release-bound workloads update through their Asset release lifecycle':
    '绑定发布的工作负载通过其资产发布生命周期更新',
  'Commit a complete immutable replacement': '提交完整的不可变替换',
  'Update and rollback unlock after the desired revision is the active deployment':
    '期望修订成为活跃部署后，才可更新和回滚',
  Update: '更新',
  'No older successfully activated revision is eligible': '没有符合条件的历史成功激活修订',
  'Clone an older activated revision into a new generation': '将历史活跃修订克隆为新代次',
  'Roll back': '回滚',
  'Immutable replacement': '不可变替换',
  'Update {name}': '更新 {name}',
  'Edit the complete requested template for generation {generation}. Secret values are never projected here; bindings contain references only.':
    '编辑第 {generation} 代的完整请求模板。这里不会投影 Secret 值，绑定仅包含引用。',
  'Complete Service template': '完整 Service 模板',
  'The template is not valid JSON.': '模板不是有效的 JSON。',
  'The template must include artifact, process, secrets, resources, ports, and health.':
    '模板必须包含 artifact、process、secrets、resources、ports 和 health。',
  'The authoritative desired revision changed while this editor was open. Close and reopen it before submitting.':
    '编辑器打开期间权威期望修订已发生变化，请关闭后重新打开再提交。',
  'Field-level changes': '字段级变更',
  'No template fields have changed.': '模板字段没有变化。',
  'A single idempotency key is retained while this dialog stays open.':
    '该对话框保持打开期间会复用同一个幂等键。',
  'Committing...': '正在提交...',
  'Commit replacement': '提交替换',
  'Manual rollback': '手动回滚',
  'Roll back {name}': '回滚 {name}',
  'Select an older successfully activated revision. Cloud clones its exact resolved template into a new generation and uses the normal health, cutover, and retirement path.':
    '选择一个历史成功激活修订。Cloud 会将其精确解析模板克隆为新代次，并沿用标准健康检查、切换与退役路径。',
  'Rollback source revision': '回滚源修订',
  'Activated {time}': '激活于 {time}',
  'The source revision ID is recorded on the durable operation.': '源修订 ID 会记录在持久化操作中。',
  'Roll back to generation {generation}': '回滚到第 {generation} 代',
  'Close dialog': '关闭对话框',
  'Immutable source to OCI': '从不可变源码到 OCI',
  'No build runs': '暂无构建运行',
  'Accepted source revisions and their authoritative build state will appear here.':
    '已接受的源码修订及其权威构建状态将显示在这里。',
  'Build {id} · Attempt {attempt}': '构建 {id} · 第 {attempt} 次尝试',
  'source {source} · {time}': '源码 {source} · {time}',
  'Source digest': '源码摘要',
  'Preparing input': '正在准备输入',
  Platform: '平台',
  Artifact: '制品',
  'Verified evidence': '已验证证据',
  Provenance: '来源证明',
  'Signing key': '签名密钥',
  'Inspecting run': '正在查看',
  'Inspect run': '查看运行',
  Cancelling: '正在取消',
  'Cancel build': '取消构建',
  Retrying: '正在重试',
  'Retry build': '重试构建',
  'Supply-chain integrity': '供应链完整性',
  'Build evidence': '构建证据',
  Verified: '已验证',
  'Select a build run': '请选择构建运行',
  'Choose a BuildRun to inspect its signed SBOM and provenance state.':
    '选择一个 BuildRun 以查看其已签名 SBOM 与来源证明状态。',
  'Attestation in progress': '正在生成证明',
  'No evidence available': '暂无可用证据',
  'Cloud is generating and signing the immutable evidence document.': 'Cloud 正在生成并签名不可变证据文档。',
  'This BuildRun has not produced verified supply-chain evidence.':
    '该 BuildRun 尚未生成已验证的供应链证据。',
  'key version {version}': '密钥版本 {version}',
  'SBOM digest': 'SBOM 摘要',
  'Provenance digest': '来源证明摘要',
  'Signing key ID': '签名密钥 ID',
  'Evidence schema': '证据 Schema',
  'Loading evidence': '正在加载证据',
  'View evidence JSON': '查看证据 JSON',
  'Download JSON': '下载 JSON',
  'Evidence JSON': '证据 JSON',
  'Bounded preview': '有限预览',
  'Complete document': '完整文档',
  'preview truncated; download the complete document.': '预览已截断；请下载完整文档。',
  'Build evidence response did not match the selected BuildRun.': '构建证据响应与所选 BuildRun 不匹配。',
  'Build evidence could not be loaded.': '无法加载构建证据。',
  'Durable timeline': '持久化时间线',
  'Clear {count} terminal': '清除 {count} 个终态操作',
  'No visible operations': '暂无可见操作',
  'Active work and new authoritative terminal results will appear here.':
    '活跃工作及新的权威终态结果将显示在这里。',
  'rollback source {source}': '回滚源 {source}',
  'source {source}': '源码 {source}',
  'build {build}': '构建 {build}',
  'seq {sequence} · {time}': '序号 {sequence} · {time}',
  'Log stream filter': '日志流筛选',
  'Ordered log records': '有序日志记录',
  'Showing the latest {count} ordered records at most': '最多显示最新 {count} 条有序记录',
  'Connected. Waiting for ordered log records.': '已连接，正在等待有序日志记录。',
  'Connecting to the authoritative log stream.': '正在连接权威日志流。',
  '{reason} · {count} records': '{reason} · {count} 条记录',
  'unknown time': '未知时间',
  'Live workload logs': '实时工作负载日志',
  'Bounded live delivery': '有界实时投递',
  'Workload logs': '工作负载日志',
  'No active revision': '无活跃修订',
  'Logs become available after a revision is scheduled.': '修订完成调度后即可查看日志。',
  'Build log availability': '构建日志可用性',
  'A3S Box contract pending': '等待 A3S Box 契约',
  'Build logs': '构建日志',
  'Build {id} · {status}': '构建 {id} · {status}',
  'No selected build': '未选择构建',
  'Build logs are unavailable until A3S Box exposes an authoritative durable log contract.':
    '在 A3S Box 提供权威持久化日志契约前，构建日志暂不可用。',
  'Select a build run to inspect log availability.': '请选择构建运行以查看日志可用性。',
  'Search authorized resources': '搜索已授权资源',
  'Choose an organization to search': '选择组织后搜索',
  'Search authorized Cloud resources': '搜索已授权 Cloud 资源',
  Searching: '正在搜索',
  'No authorized resources found.': '未找到已授权资源。',
  'Authorized search is unavailable.': '授权资源搜索暂不可用。',
  'Durable context': '持久化上下文',
  'Agent conversations': 'Agent 会话',
  'Agent execution workbench': 'Agent 执行工作台',
  'Creating...': '正在创建...',
  'New conversation': '新建会话',
  'Create a conversation to start an immutable Agent release.': '创建会话以运行不可变的 Agent 发布。',
  '{count} events': '{count} 个事件',
  'Exact release binding': '精确发布绑定',
  Executions: '执行',
  'Published Agent release': '已发布 Agent 版本',
  'Choose a release': '请选择版本',
  'Starting...': '正在启动...',
  'Start execution': '启动执行',
  'No executions recorded for this conversation.': '该会话暂无执行记录。',
  'release {release}': '发布 {release}',
  'Monotonic semantic history': '单调递增语义历史',
  'Execution events': '执行事件',
  'Select a conversation to follow its semantic event stream.': '选择会话以跟踪其语义事件流。',
  '{time} · {count} bytes': '{time} · {count} 字节',
  'Agent event stream authorization failed': 'Agent 事件流授权失败',
  'Agent event stream closed': 'Agent 事件流已关闭',
  'Agent event stream failed': 'Agent 事件流失败',
  'Route active': '路由生效',
  Rejected: '已拒绝',
  Unavailable: '不可用',
  Publishing: '发布中',
  'Prior revision active': '上一修订仍活跃',
  '{count} acknowledged': '{count} 条已确认',
  'just now': '刚刚',
  '{count}m ago': '{count} 分钟前',
  'Not recorded': '未记录',
};

const LABELS_ZH_CN: Record<string, string> = {
  all: '全部',
  active: '活跃',
  accepted: '已接受',
  applying: '应用中',
  attesting: '生成证明中',
  cancelled: '已取消',
  cancelling: '取消中',
  cleanup_pending: '等待清理',
  completed: '已完成',
  connecting: '正在连接',
  failed: '失败',
  healthy: '健康',
  idle: '空闲',
  live: '实时',
  orphaned: '孤立',
  pending: '等待中',
  prepared: '准备完成',
  preparing: '准备中',
  provisioning: '配置中',
  published: '已发布',
  publishing: '发布中',
  queued: '排队中',
  resolving: '解析中',
  retrying: '正在重连',
  retiring: '退役中',
  running: '运行中',
  scheduled: '已调度',
  stopped: '已停止',
  stopping: '停止中',
  suspended: '已暂停',
  succeeded: '成功',
  verifying: '验证中',
  validating: '校验中',
  unavailable: '不可用',
  unhealthy: '不健康',
  unknown: '未知',
  rejected: '已拒绝',
  issued: '已签发',
  ready: '就绪',
  revoked: '已撤销',
  verified: '已验证',
  archived: '已归档',
  draft: '草稿',
  yanked: '已撤回',
  workload: '工作负载',
  deployment: '部署',
  build_run: '构建运行',
  source_revision: '源码修订',
  route: '路由',
  domain_claim: '域名声明',
  gateway_scope: 'Gateway 范围',
  operation: '操作',
  execution_requested: '已请求执行',
  model_output: '模型输出',
  execution_failed: '执行失败',
  execution_completed: '执行完成',
  execution_cancelled: '执行已取消',
};

type TranslationValues = Record<string, string | number>;

interface I18nContextValue {
  language: Language;
  setLanguage: (language: Language) => void;
  t: (message: string, values?: TranslationValues) => string;
  label: (value: string) => string;
  formatRelative: (value: string) => string;
  formatTimestamp: (value: string | null) => string;
}

const fallbackContext: I18nContextValue = {
  language: 'en',
  setLanguage: () => undefined,
  t: interpolate,
  label: humanizeEnglish,
  formatRelative: (value) => formatRelativeFor('en', value),
  formatTimestamp: (value) => formatTimestampFor('en', value),
};

const I18nContext = createContext<I18nContextValue>(fallbackContext);

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [language, setLanguage] = useState<Language>(readStoredLanguage);

  useEffect(() => {
    document.documentElement.lang = language;
    try {
      localStorage.setItem(LANGUAGE_KEY, language);
    } catch {
      // Storage can be unavailable in hardened browser contexts.
    }
  }, [language]);

  const value = useMemo<I18nContextValue>(() => {
    const t = (message: string, values?: TranslationValues) =>
      interpolate(language === 'zh-CN' ? (ZH_CN[message] ?? message) : message, values);
    return {
      language,
      setLanguage,
      t,
      label: (input) =>
        language === 'zh-CN' ? (LABELS_ZH_CN[input] ?? humanizeEnglish(input)) : humanizeEnglish(input),
      formatRelative: (input) => formatRelativeFor(language, input),
      formatTimestamp: (input) => formatTimestampFor(language, input),
    };
  }, [language]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  return useContext(I18nContext);
}

export function LanguageSwitcher({ compact = false }: { compact?: boolean }) {
  const { language, setLanguage, t } = useI18n();
  return (
    <fieldset
      className={compact ? 'button-group language-switcher compact' : 'button-group language-switcher'}
    >
      <legend className='sr-only'>{t('Language')}</legend>
      <button
        className='btn'
        data-size='sm'
        data-variant='ghost'
        type='button'
        aria-pressed={language === 'zh-CN'}
        onClick={() => setLanguage('zh-CN')}
      >
        中文
      </button>
      <button
        className='btn'
        data-size='sm'
        data-variant='ghost'
        type='button'
        aria-pressed={language === 'en'}
        onClick={() => setLanguage('en')}
      >
        EN
      </button>
    </fieldset>
  );
}

function readStoredLanguage(): Language {
  try {
    return localStorage.getItem(LANGUAGE_KEY) === 'en' ? 'en' : 'zh-CN';
  } catch {
    return 'zh-CN';
  }
}

function interpolate(message: string, values?: TranslationValues): string {
  if (!values) return message;
  return message.replace(/\{(\w+)\}/g, (match, key: string) =>
    Object.hasOwn(values, key) ? String(values[key]) : match
  );
}

function humanizeEnglish(value: string): string {
  return value.replaceAll('_', ' ').replace(/^./, (character) => character.toUpperCase());
}

function formatRelativeFor(language: Language, value: string): string {
  const elapsed = Math.max(0, Date.now() - new Date(value).getTime());
  if (elapsed < 60_000) return language === 'zh-CN' ? ZH_CN['just now'] : 'just now';
  if (elapsed < 3_600_000) {
    const count = Math.floor(elapsed / 60_000);
    return language === 'zh-CN' ? interpolate(ZH_CN['{count}m ago'], { count }) : `${count}m ago`;
  }
  return new Intl.DateTimeFormat(language, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value));
}

function formatTimestampFor(language: Language, value: string | null): string {
  if (!value) return language === 'zh-CN' ? ZH_CN['Not recorded'] : 'Not recorded';
  return new Intl.DateTimeFormat(language, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(value));
}
