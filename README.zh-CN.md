# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud 将 Agent、Workflow、Function、Durable Cell、推理与 Web 语义转化为 A3S Runtime 与 Box 上的受治理服务" />
</p>

<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=release" /></a>
  <img alt="Rust 1.88 或更高" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST 契约 1.85.0" src="https://img.shields.io/badge/REST_contract-1.85.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT 许可证" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#工作原理">架构</a> &middot;
  <a href="#服务结果">服务</a> &middot;
  <a href="#快速开始">快速开始</a> &middot;
  <a href="#平台能力">能力</a> &middot;
  <a href="#交付状态">交付</a> &middot;
  <a href="#文档">文档</a>
</p>

**A3S Cloud 是自托管、Agent 优先的开发者平台，将租户授权的产品意图转化为运营商自有 CPU/GPU 基础设施上的持久 AaaS、WaaS、FaaS、Durable Cell、模型推理与 Static Web 服务。** Cloud 拥有产品与期望状态真相；A3S Flow 协调持久工作；A3S Runtime 定义统一生命周期契约；A3S Box 执行它；A3S Gateway 是唯一公共边缘。

> [!IMPORTANT]
> 架构目标不是可用性声明。仅当某能力在 [ROADMAP.md](ROADMAP.md) 中将其真实提供商、失败、恢复、清理、升级与发布闸门标为 <code>Verified</code> 后，该能力才算发布。

> [!NOTE]
> A3S Cloud 不附带管理 Dashboard。它确实托管面向 Application 与 Agent 的不可变 React/Vue 及其他租户 Web 发布。这些站点与其他所有客户端使用相同的 Gateway 与 API。

> [!TIP]
> 自动 CI 与 Box 符合性仅对推送到 `release` 以及面向 `release` 的 pull request 运行。`main` 分支不会自动启动这些工作流；对临时验证运行请使用显式 workflow dispatch 条目。

## 工作原理

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud 权威图：从 A3S Gateway 经租户产品域、Identity、PostgreSQL、Flow、Workloads 与 Fleet、Runtime、Box、供给、存储、派发与可观测性" />
</p>

架构对每种服务遵循同一路径：

1. **准入（Admit）：** A3S Gateway 认证公共流量；Identity 解析确切 Installation、Organization、Project、Environment、Principal 与凭证范围。
2. **提交（Commit）：** Application 用例经 A3S ORM 向 PostgreSQL 原子写入期望状态、幂等、审计证据与 Outbox 事实。
3. **协调（Coordinate）：** A3S Flow 与 Operations 拥有持久等待、重试、重放、审批、补偿、取消与投递历史。
4. **放置与执行：** Workloads 与 Fleet 拥有单一 CPU/GPU 调度器、Claims、滚动、排空与 fencing；节点 agent 经 A3S Box 收敛 A3S Runtime <code>Task</code> 或 <code>Service</code> 单元。
5. **发布（Publish）：** Edge 编译完整带版本路由快照；A3S Gateway 应用并服务它们。Cloud 永不成为第二个请求字节代理。

Runtime CI/CD 使用同一权威图：构建一次、验证确切摘要、晋升降级同一不可变发布、经 Workloads/Fleet 部署、经 Edge/Gateway 切换流量，并回滚到更早已准入发布。产品域保留发布真相；Flow 保留流水线历史。

## 服务结果

六种产品结果共享两类执行，而非创建六套运行时栈：

- **AaaS — Agent as a Service。** Agents 拥有对话、执行、语义事件、审批、检查点、fork、Tool 证据、提供商绑定与恢复。有状态 Agent（如 A3S Code）作为温热、会话隔离的 Runtime <code>Service</code> 单元运行；有界批处理 Agent 可用 <code>Task</code>。
- **WaaS — Workflow as a Service。** Workflow 拥有本体、不可变定义与计划、WorkflowRun、HumanTask、类型化节点顺序与结果。A3S Flow 协调 Agent、Function、MCP、Inference、Cell、人工、Connector、Task 与 Service 节点；没有重复的 Workflow 运行时。
- **FaaS — Function as a Service。** Assets 拥有不可变 Function 发布/配置文件；其应用门面将每次调用委托给 Executions、Workloads 或 Connectors，而不拥有另一套生命周期。Function 作为 Runtime <code>Task</code>、无状态 <code>Service</code> 或外部 FaaS Connector 运行。无会话 MCP 与来自 A3S Code 的调用使用相同模式。`FN0.1` 冻结这些组件契约；在后续所有者与生产闸门通过前，FaaS 仍不可用。
- **Durable Cell。** Durable Cells 拥有应用修订、兼容性、保留与部署/存储关联。普通 Runtime <code>Service</code> 为人员与多个 Agent 提供命名、串行化、可休眠的状态空间，而不复制 Agent 或 Workflow 历史。
- **模型推理。** Inference 拥有模型修订、部署、角色拓扑、路由策略、用量与评估。它在共享 CPU/GPU 放置轨上支持独立副本与类型化多节点 prefill/decode 组。
- **Static Web。** Applications 与 Assets 拥有不可变 Web 发布。React、Vue 及其他已准入对象由 Gateway 直接服务，带缓存、CSP、SPA 回退、路由与回滚策略；SSR 是普通 <code>Service</code> 配置文件。

唯一通用执行类是 **Task** 与 **Service**。Agent、Function、MCP、推理、Cell、构建与 Cloud 系统行为通过不可变、消费者拥有的配置文件表达。A3S Runtime 拥有统一生命周期契约；A3S Box 提供商实现它。

## 快速开始

### 要求

- Rust 1.88 或更高
- PostgreSQL 17 或兼容的受支持发行版
- 用于 pin 外部源获取的 Git CLI
- 用于节点本地工作负载/构建执行的 A3S Box
- 用于路由服务的已 pin A3S Gateway 修订
- 生产 <code>all</code>、Worker 或 Relay 角色所需的 NATS JetStream
- 仅 TypeScript 客户端或 CLI 开发需要 Bun

### 启动开发 API

~~~bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_POSTGRES_MIGRATION_URL="$A3S_CLOUD_POSTGRES_URL"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
~~~

服务进程永不跑迁移。一次性 migrator 在 PostgreSQL 可达之后、API、Worker、Relay 或 <code>all</code> 之前运行；生产使用不同的迁移与服务主体。

~~~bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
~~~

直接端口访问是本地开发便利。生产仅通过 A3S Gateway 发布 API。

<details>
<summary><strong>引导第一个 Organization</strong></summary>

Cloud 仅存储 API token 摘要；调用方创建并保留凭证。下列请求还会创建已接受的基线平台角色策略，并将 <code>PlatformOwner</code> 绑定到同一引导 Principal。并发相同请求在重放前串行化；任何策略、审计或 Outbox 失败会回滚完整身份与权威根。

~~~bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: $A3S_CLOUD_BOOTSTRAP_TOKEN" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"$A3S_CLOUD_ADMIN_TOKEN\",\"expiresAt\":null}"
~~~

后续变更使用 <code>Authorization: Bearer ...</code> 与稳定 <code>idempotency-key</code>。

</details>

### 使用 CLI

~~~bash
bun install --cwd cli --frozen-lockfile

export A3S_CLOUD_TOKEN="$A3S_CLOUD_ADMIN_TOKEN"
export A3S_CLOUD_URL="http://127.0.0.1:8080/api/v1"

bun run --cwd cli src/main.ts diagnostics status --output=json
bun run --cwd cli src/main.ts organizations list --output=json
bun run --cwd cli src/main.ts operations list --output=json
~~~

凭证来自环境变量或标准输入，永不写入 CLI 上下文文件。

## 平台能力

### 构建、供给与晋升降级

- 托管 Git 权威加外部源修订、webhook、可复现 Box 构建、provenance、pull request 预览、不可变产物与保留摘要的晋升降级。
- 分离的 **Git**、**OCI Registry** 与 **A3S Use Registry** 权威。无一被过载去冒充另一供给类型。
- 受治理的逻辑 Model 与 Model Revision，加不可变模型权重 manifest/对象、外部 hub 解析（如 ModelScope）、许可证、信任策略与可重建节点缓存。
- 面向 Agent、Workflow、Function/MCP、Durable Cell、推理、Static Web 与 Cloud 系统服务发布的统一 Flow 后备 CI/CD 模型。
- 托管 Agent 将一份规范、Code 拥有的最终发布 manifest 绑定到确切 OCI 与已签名构建 provenance，再经普通 Workloads/Runtime 路径只读挂载其确定性归档。同一 manifest 拥有就绪路径、存活路径与优雅关停间隔；调用方不能在部署时削弱生命周期契约。

### 运行、扩缩与恢复

- 面向 CPU 池、GPU 池、加速器/拓扑约束、Claims、反亲和、gang 放置、维护、配额与抢占策略的统一异构调度器。
- 无状态水平扩缩与 scale-to-zero；有状态排空、单写 fencing、检查点/交接、恢复与局部性感知放置。
- 分布式推理含独立副本、多节点组、prefill/decode 解耦角色、显式 KV 传输，以及共享调度器而非第二套推理控制面。
- 可独立扩缩的 API、Worker、Relay、migrator、node-agent 与 Gateway 角色，带显式就绪、迁移、lease/leader、滚动与恢复契约。

### 存储、服务与观测

- 面向外部 HTTPS AWS S3 或 S3 兼容存储的统一类型化不可变对象客户端。Cloud 不捆绑 S3 服务器，也不将对象存储呈现为 POSIX/FUSE。可变卷、备份、恢复、保留与写 fencing 属于 Data/S0。
- A3S Gateway 拥有 TLS、认证、请求限制、路由、模型/Agent/Function/MCP 端点与租户 Web 投递。Cloud 服务保持私有。
- OpenTelemetry 关联日志、指标、追踪、SLO 与事件。不可变证据留在域所有者处；Apache Doris 是可选、可重建的分析与 SLO 投影。

### 治理租户与特权访问

- 统一不可变 Installation 身份，以及跨租户隔离、成员、Resource Grant、配额、凭证、审计、Outbox 与生命周期清理的统一判别 Installation/Organization/Project/Environment 范围契约。平台事实永不借用哨兵 Organization。
- 面向安装、机群、迁移、策略、事件与 break-glass 职责的独立系统管理员 RBAC 平面。系统角色永不静默授予租户数据或 Secret 访问。
- 租户支持需要活跃确切人工、已准入支持用途角色、短时不可续期授权、后代范围，以及一条封闭非敏感权限。每次允许钉住可重放的策略、凭证、绑定、授权、动作、资源与请求证据。
- 全新引导原子创建第一个 Organization、服务 Principal、所有者 Membership、API token、已接受基线平台角色策略，以及匹配的 <code>PlatformOwner</code> 绑定，共享审计、Outbox 与幂等事实。
- 特权变更与安装级组织目录读取使用同一 Identity/PostgreSQL 决策签发器。有效确切 <code>cloud:read</code> 凭证若无 <code>TenantLifecycleRead</code>，仅见其自身 Organization；已撤销、过期、不匹配或范围不足的凭证失败封闭。
- 工作负载信任在同一 Identity 权威中使用不可变 TrustDomain 与 WorkloadIdentityPolicy 修订。当前、确切、有界历史与按工作负载索引的读取，以及 CAS fencing 的接受，经 REST/OpenAPI、TypeScript 客户端与 CLI 暴露，无调用方自写 actor、凭证或 Installation 覆盖。
- 规范 <code>cloud.identity.workload-provider.v1</code> 配置文件将每个 TrustDomain 修订按摘要绑定到一个可替换提供商适配器。仅 API 的 <code>spiffe_https_web</code> 适配器执行新鲜 HTTPS、有界、严格 JSON 的 SPIFFE bundle 观察，并对照该确切修订准入。其契约将端点证据标为 <code>observed*</code>，将摘要绑定的配置文件策略标为 <code>declared*</code>；它不拥有证书签发、私钥、提供商注册表或并行信任状态。
- 带版本的 <code>cloud.identity.workload-runtime-evidence-binding.v1</code> 基础将一份确切策略摘要绑定到其 Workloads Claim、NodePool 与 Fleet Node 会话/能力快照，以及 Runtime Unit 世代与 Box 提供商证明。已验证的 C2 仅组合 Workloads 与 Fleet 所有者事实。仅组件的 C3a 在调度前准入一次通用 Identity 授权，并持久化不可变 Workloads 记录（含显式无策略结果），使崩溃重放不能重新标记遗留或运行中 Unit。仅组件的 C3b 在迁移 <code>181</code> 中加入唯一 Identity 拥有的不可变 <code>cloud.identity.workload-runtime-evidence-record.v1</code> 历史。确切准入重放可返回其历史事实；每次新写入在规范 Installation fencing 下重读当前 TrustDomain/Policy 与双方所有者事实，再经一个类型化 A3S ORM 仓库提交。PostgreSQL 拒绝变更、陈旧证据，以及未先串行化的 Policy/证据竞态。确定性 V1 记录仍缺少 Node 硬件证明，且不能授权凭证签发；C4 仍是必需的新鲜决策，而非推断能力。
- OpenShift 级结果——协调、调度、隔离、滚动、策略、可观测性与 day-two 运维——以及 TokenHub 级结果——受治理模型/提供商/密钥访问、路由、配额、诊断与用量——经 A3S 权威组合，而非复制 API 或控制面。

## 构造一致性

| 关注点 | 规范规则 |
| --- | --- |
| 命令并发 | 确切租户范围、幂等键、期望版本与载荷摘要事务性检查；漂移或冲突重放失败封闭 |
| 数据库写入 | 聚合、幂等、审计与 Outbox 经 A3S ORM/PostgreSQL 一并提交；数据库解析规范 Installation 谱系 |
| 跨系统工作 | A3S Flow saga 与所有者回执调和不确定结果；无数据库事务跨越外部提供商 |
| 速率限制与配额 | Gateway 强制请求限制；所有者准入强制持久配额。Redis 可加速计数器但永不成为配额真相 |
| 缓存 | Redis 持有有界、可重建的读、发现、token 与协调提示，带修订失效；缓存丢失改变延迟，不改变正确性 |
| 锁与租约 | PostgreSQL/CAS 拥有正确性与 fencing。分布式锁可降低争用，但不能替代版本或 Fleet Claims |
| 派发压力 | A3S Lane 仅为公平、背压与有界并发准入已持久化工作；它既不拥有工作流也不拥有队列真相 |
| 分析 | Doris 消费可重建遥测/证据投影；PostgreSQL 与有界上下文所有者仍是运维真相 |

## DDD 与单一权威

<p align="center">
  <img src="assets/readme/ddd-boundary.svg" width="100%" alt="A3S Cloud DDD 依赖方向：从入站适配器经 Presentation、Application、Domain、内向拥有端口、Infrastructure 提供商与已提交集成事实" />
</p>

Presentation 调用 Application；Application 协调其 Domain 与消费者拥有的端口；Infrastructure 实现这些内向端口。上下文仅通过同步所有者 Application 契约，或从所有者已提交 Outbox 发出的带版本事实协作。

| 关注点 | 唯一权威 | 禁止的重复 |
| --- | --- | --- |
| 租户身份与授权 | Identity + Projects | 适配器本地角色、仅 UI 策略、提供商会话，或以缓存声明为真相 |
| 产品含义 | 拥有 Agent、Workflow、Function、Cell、Inference、Application 或 Asset 的上下文 | Runtime/提供商字段变成产品状态 |
| 持久协调 | Operations + A3S Flow | 产品重试表、sleep 循环或另一工作流引擎 |
| 构建与发布投递 | Sources + Artifacts + 产品 Release 所有者 + Delivery Pipelines | 产品本地 CI 状态、晋升降级时重建，或可变部署标签 |
| 放置与滚动 | Workloads + Fleet | Agent、MCP、Cell、模型或 Gateway 专用调度器 |
| 提供商生命周期 | A3S Runtime + A3S Box | 产品域直接调用进程/容器/FaaS |
| 公共流量 | Edge 期望状态 + A3S Gateway 已应用状态 | Cloud 代理、按产品入口，或另一 Gateway 发布者 |
| 不可变与可变数据 | 共享对象客户端 + Data/S0 | 按产品 S3 客户端、备份引擎，或以提供商状态为期望状态真相 |
| 集成事实 | 一个范围感知事务 Outbox + A3S Event | 提交前发布、哨兵租户，或并行产品/平台事件总线 |
| 配置 | 由 <code>a3s-acl</code> 解析的 A3S ACL | 兼容解析器或另一产品配置语言 |

横切行为遵循一条可见有序管道：认证、授权、校验、幂等、事务、审计/Outbox，然后派发。日志、追踪、指标、缓存、速率限制与 AOP 拦截器观察或保护该路径；无一可成为第二业务权威。
[可执行架构棘轮](docs/architecture-audit.md) 阻止外层导入与重复机制在已知债务清理期间扩散。

## 交付状态

组合以闸门驱动，而非百分比驱动。截至 **2026-09-06**：

| 泳道 | 证据状态 |
| --- | --- |
| 租户范围 Identity、PostgreSQL/A3S ORM、Operations/Flow、Outbox、公共 API 与迁移 | **已验证基础** |
| Installation 范围与系统管理员 RBAC | **已验证核心，更广闸门进行中。** 原子全新引导、策略/绑定与支持授权仓库、确切特权决策、受保护变更、REST/OpenAPI、TypeScript 客户端、CLI、Management MCP，以及撤销 fencing 的组织目录已验证。预根安装的受控恢复，以及更广 MT3 角色矩阵、所有者端口清理与敌对租户证据仍待完成 |
| Workloads、Fleet、Runtime/Box、Gateway、供给、协作与企业控制 | **进行中；A0.4 真实提供商闸门已验证。** [A0.4 PostgreSQL/真实 Box 提供商闸门](https://github.com/A3S-Lab/Cloud/actions/runs/33686237668/job/100434300332) 与 [完整 Cloud CI](https://github.com/A3S-Lab/Cloud/actions/runs/33686237772) 对照 Box `65f3d3fc7c1e0e2cb1ba2d409a79f7357314f5ae` 与 OCI Runtime `878f8414cef3b85bef1b51fe6735017b25828252` 通过；更广组件/提供商闸门仍待 |
| Agent 与托管 MCP 产品泳道 | **进行中；A0.4 已验证。** A0.4 已发布 Agent 部署、PostgreSQL 持久化、真实 Box 恢复、Secret 再物化、取消与清理闸门由保留的 [提供商证据](https://github.com/A3S-Lab/Cloud/actions/runs/33686237668/job/100434300332) 验证；A0.3、A0.5 与托管 MCP 仍受闸门约束，因此组件证据不意味着完整 AaaS 可用 |
| 本体 Workflow 与 AI Applications/Files | **进行中。** 完整 WaaS 与 Application 产品仍受闸门约束 |
| Automations | **组件基础进行中。** 确切修订 webhook 准入、调度日历/misfire/并发/持久游标租约边界、确定性到期时间信封、带持久 PostgreSQL 状态的幂等调用准入、可注入有界调度 worker/规范化事件消费者边界，以及摘要验证的调用目标所有者交接已实现。控制面 supervisor 在提供所有者组合的提供商/处理器时接受并优雅停止这些进程。生产候选发现、目标接线、实况恢复证据与公开可用性仍开放 |
| Data/S0 与 Durable Cell | **基础进行中。** Durable Cell 是一等目标，但尚非可用托管服务 |
| 工作负载身份 | **已验证信任与 WI2-C1/C2 基础；C3a 与 C3b 在 main 上已验证。** [信任/提供商 main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33291073009)、[C1/C2 main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33310808529) 与 [C1/C2 Box 提供商符合性](https://github.com/A3S-Lab/Cloud/actions/runs/33310808538) 通过。C3a 经迁移 `180` 生产组合通用 Identity 授权 ACL 与一份不可变 Workloads 调度前 bound/no-policy 记录；完整 [C3a main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33319781762) 与 [同修订 Box 提供商符合性](https://github.com/A3S-Lab/Cloud/actions/runs/33319781830) 通过。C3b 加入迁移 `181`、一份类型化不可变 Identity 证据历史、确切历史重放、当前 Policy/TrustDomain 再验证、确定性同事实采纳，以及保留的并发/撤销测试，无公共 API 或第二所有者生命周期；[C3b main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33327919058) 与 [同修订 Box 提供商符合性](https://github.com/A3S-Lab/Cloud/actions/runs/33327919079) 通过。Fleet 硬件证明、完整签发决策、签发、强制、撤销与联邦仍开放 |
| FaaS、分布式推理、模型供给、Static Web、Runtime CI/CD 与完整 HA 运维 | **已规划或早期基础。** 其架构与权威边界已定义，但完整产品闸门仍待 |

确切依赖、证据与剩余闸门见 [产品路线图](ROADMAP.md)、[平台差距分析](docs/platform-gap-analysis.md) 与 [生态项目路线图](docs/project-roadmaps/README.md)。

## 部署模型

Cloud 系统服务与租户工作负载共享机制，但永不借用权威：

- 引导平面经一个依赖 DAG 安装 PostgreSQL、NATS、S3 兼容存储、Git、OCI Registry、A3S Use Registry、migrator、API、Worker、Relay、Gateway 与可观测性依赖；
- API、Worker、Relay、migrator、节点 agent 与 Gateway 独立扩缩；
- 租户工作负载仅经已准入发布、Workloads/Fleet 放置、Runtime/Box 执行与 Gateway 发布进入；
- 管理以 API/OpenAPI/客户端/CLI/MCP 为先；没有 Cloud Dashboard 或 UI 专用后端。

初始 Box 托管配置文件是安装基础。生产 HA 需要 [部署架构](docs/deployment-and-cluster-architecture.md) 中命名的干净安装、升级、回滚、依赖丢失、凭证轮换、存储恢复、节点排空与多副本闸门。

## 接口与配置

| 面 | 契约 | 从这里开始 |
| --- | --- | --- |
| REST/OpenAPI | 带版本 <code>/api/v1</code>、请求 ID、幂等、通用信封、已提交快照 | [指南](docs/openapi.md) · [openapi/v1.json](openapi/v1.json) |
| TypeScript 客户端 | 同一 REST 契约上的维护适配器 | [packages/cloud-client](packages/cloud-client) |
| CLI | 可脚本化结构化输出，无 token 参数 | [cli/README.md](cli/README.md) |
| Management MCP | 无会话、租户授权工具，经同一 Application 处理器 | [docs/management-mcp.md](docs/management-mcp.md) |

Cloud 与 Node Agent 仅接受由 <code>a3s-acl</code> 解析的封闭、已校验 **A3S ACL**。未知字段与不安全时序关系在启动前失败；Secret 值永不属于 ACL。从 [config/cloud.acl](config/cloud.acl)、[config/node.example.acl](config/node.example.acl) 与 [deploy/production](deploy/production/README.md) 基线开始。

Redis 是可选加速，Doris 是可选分析，二者皆非持久真相。S3 兼容对象存储与 NATS 是外部生产依赖。

## 仓库与开发

<details>
<summary><strong>仓库布局</strong></summary>

~~~text
Cloud/
|-- crates/
|   |-- contracts/       # versioned cross-process contracts
|   |-- control-plane/   # bounded contexts, API, workers, persistence
|   `-- node-agent/      # outbound node protocol and provider adapters
|-- migrations/          # PostgreSQL schema evolution
|-- config/              # closed A3S ACL configuration
|-- openapi/             # committed REST contract
|-- packages/cloud-client/
|-- cli/
|-- tools/               # provider, recovery, architecture, release gates
`-- docs/                # architecture, decisions, plans, and runbooks
~~~

</details>

<details>
<summary><strong>核心开发闸门</strong></summary>

~~~bash
cargo fmt --all -- --check
cargo test -p a3s-cloud-control-plane architecture_tests --lib
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
~~~

真实提供商与发布认证在隔离 Linux 主机上运行。重要的仓库自有闸门包括 [跨面符合性](tools/c0-conformance/README.md)、[Runtime 符合性](tools/runtime-conformance/README.md)、[Box 提供商符合性](tools/box-conformance/README.md)、[工作负载身份提供商符合性](tools/workload-identity-conformance/README.md)，以及 [已 pin Gateway 修订](tools/gateway-conformance/gateway-revision)。

</details>

## 文档

| 从这里开始 | 用途 |
| --- | --- |
| [产品路线图](ROADMAP.md) | 闸门状态、依赖、证据与交付顺序 |
| [技术架构](docs/architecture.md) | 稳定所有权、拓扑、一致性与失败行为 |
| [AI 服务平台](docs/ai-service-platform-architecture.md) | AaaS、WaaS、FaaS、Durable Cell、Inference、Runtime、Box 与 Gateway 组合 |
| [Agent 发布部署契约](docs/agent-release-deployment-contract.md) | Code 拥有的最终 manifest 生成、provenance、持久化、重放与 Runtime 投影 |
| [Agent Runtime](docs/agent-runtime-architecture.md) · [Function Runtime](docs/function-runtime-architecture.md) · [Durable Cell](docs/durable-cell-platform-plan.md) | 统一 Runtime 上的服务语义 |
| [Static Web](docs/static-web-hosting-architecture.md) · [模型供给](docs/model-supply-architecture.md) · [推理](docs/inference-plan.md) | 租户 UI、模型/权重与服务架构 |
| [集群部署](docs/deployment-and-cluster-architecture.md) · [弹性服务](docs/elastic-service-deployment-architecture.md) | 系统服务、CPU/GPU 调度、有状态/无状态收敛与 HA |
| [Runtime CI/CD](docs/runtime-cicd-architecture.md) · [工作负载身份](docs/workload-identity-and-service-connectivity-architecture.md) | 投递、证明、私有发现、mTLS 与撤销 |
| [分布式 API 一致性](docs/distributed-api-consistency-architecture.md) · [Redis 与 Lane](docs/redis-and-lane-platform-architecture.md) | 并发、事务、缓存、锁、公平与背压 |
| [可观测性与分析](docs/observability-and-analytics-architecture.md) · [平台差距分析](docs/platform-gap-analysis.md) | 遥测/SLO/事件设计与优先缺失结果 |
| [多租户平台](docs/multi-tenant-developer-platform-architecture.md) · [能力架构](docs/platform-capability-architecture.md) | 租户/管理员 RBAC 与 OpenShift-/TokenHub 级结果 |
| [DDD、AOP 与模式](docs/ddd-aop-and-pattern-architecture.md) · [架构审计](docs/architecture-audit.md) | 分层规则、切面顺序、模式与可执行债务棘轮 |
| [生态路线图](docs/project-roadmaps/README.md) | 每个 A3S 子项目的使命、依赖、证据与负面边界 |

## 许可证

[MIT](LICENSE)
