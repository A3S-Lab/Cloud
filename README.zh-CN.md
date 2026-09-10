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
  <a href="openapi/v1.json"><img alt="REST 契约 1.87.0" src="https://img.shields.io/badge/REST_contract-1.87.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT 许可证" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#cloud-是什么">第一性原理</a> &middot;
  <a href="#工作原理">架构</a> &middot;
  <a href="#产品泳道">产品泳道</a> &middot;
  <a href="#快速开始">快速开始</a> &middot;
  <a href="#交付状态">交付</a> &middot;
  <a href="#文档">文档</a>
</p>

**A3S Cloud 是自托管控制面：将租户授权的产品意图转化为运营商自有 CPU/GPU 基础设施上的期望状态。**
Cloud 在 PostgreSQL 中拥有产品与期望状态真相；A3S Flow 协调持久工作；A3S Runtime 定义统一生命周期契约；A3S Box 执行它；A3S Gateway 是唯一公共边缘。产品泳道（AaaS、WaaS、FaaS、Durable Cell、推理、Static Web）是同一路径上的投影——不是六套运行时。

> [!IMPORTANT]
> 架构目标不是可用性声明。仅当某能力在 [ROADMAP.md](ROADMAP.md) 中将其真实提供商、失败、恢复、清理、升级与发布闸门标为 <code>Verified</code> 后，该能力才算发布。此前即使存在组件，也应视为不可用。

> [!NOTE]
> A3S Cloud 不附带管理 Dashboard。管理面是 REST/OpenAPI、TypeScript 客户端、CLI 与 Management MCP。<code>WEB0</code> 落地后，租户 Web 发布与其他客户端共用同一 Gateway 与 API。

> [!TIP]
> 自动 CI 与 Box 符合性仅对推送到 <code>release</code> 以及面向 <code>release</code> 的 pull request 运行。<code>main</code> 不会自动启动这些工作流；临时验证请使用 workflow dispatch。

## Cloud 是什么

阅读本仓库的第一性原理：

| Cloud 是 | Cloud 不是 |
| --- | --- |
| PostgreSQL 中的期望状态与产品权威 | Gateway 之外的第二请求字节代理 |
| 所有服务类共用的 Workloads + Fleet 放置路径 | 按产品分裂的调度器（Agent / MCP / Cell / GPU / 推理） |
| Edge 编译的完整 Gateway 快照 | 推理 Key 明文仓库（Identity 拥有签发；Edge 只投影 ACL） |
| 提交后的 Outbox → Event 集成 | 提交前发布或仅 UI 策略 |
| API / CLI / MCP 优先的管理 | Cloud Dashboard 后端 |
| 闸门驱动交付 | 「有模块目录即功能完整」 |

**真相层级：** PostgreSQL 与拥有边界上下文是运维真相。Redis 可加速。Doris 可投影分析。二者皆不可成为配额、授权或期望状态权威。

## 工作原理

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud 权威图：从 A3S Gateway 经租户产品域、Identity、PostgreSQL、Flow、Workloads 与 Fleet、Runtime、Box、供给、存储、派发与可观测性" />
</p>

每种已准入服务遵循同一路径：

1. **准入（Admit）。** Gateway 认证公共流量；Identity 解析 Installation、Organization、Project、Environment、Principal 与凭证范围。
2. **提交（Commit）。** Application 用例经 A3S ORM 向 PostgreSQL 原子写入期望状态、幂等、审计与 Outbox 事实。
3. **协调（Coordinate）。** Operations 与 A3S Flow 拥有持久等待、重试、重放、审批、补偿、取消与投递历史。
4. **放置与执行。** Workloads 与 Fleet 拥有 Claims、滚动、排空与 fencing；Node Agent 经 A3S Box 收敛 Runtime <code>Task</code> 或 <code>Service</code> 单元。
5. **发布（Publish）。** Edge 编译完整带版本路由快照；Fleet 下发；Node Agent 安装；Gateway 应用并服务。

Runtime CI/CD 使用同一权威图：构建一次、验证摘要、晋升降级同一不可变发布、经 Workloads/Fleet 部署、经 Edge/Gateway 切流量，并回滚到更早已准入发布。

### 控制面与数据面

| 平面 | 权威 | 说明 |
| --- | --- | --- |
| 租户 / 管理 API | 控制面 <code>api</code> 角色 | 直连端口仅本地开发；生产经 Gateway 发布 |
| 持久 worker | <code>worker</code> 角色 | 健康检查与协调器；无管理面 |
| Outbox relay | <code>relay</code> 角色 | 仅事件发布 |
| 节点控制 | Fleet ↔ Node Agent（出站 mTLS） | 命令、观测、Gateway 快照安装——与租户请求字节分离 |
| 公共流量 | Edge 期望状态 → Gateway 已应用状态 | Cloud 永不代理请求体 |
| 推理 ACL（部分） | Identity 凭证 + Inference 路由 → Edge 编译器 | 在 Power 观测投递（<code>PW0</code>）前 workers 保持为空；完整 OpenAI 数据面等待 <code>BX0</code> + <code>PW0</code> |

## 产品泳道

六种产品结果共享两类执行（**Task** 与 **Service**）。状态词汇与 [ROADMAP.md](ROADMAP.md) 一致：**Verified**、**In progress**、**Planned**。

| 泳道 | 所有者意图 | 状态 |
| --- | --- | --- |
| **AaaS** — Agent as a Service | 对话、执行、事件、审批、检查点、Tool 证据 | **进行中**（A0.4 / A1.0 / A1.2 已验证；完整 AaaS 仍受闸门约束） |
| **WaaS** — Workflow as a Service | 本体、不可变计划、WorkflowRun、HumanTask；Flow 协调节点 | **进行中**（完整产品不可用） |
| **FaaS** — Function as a Service | 不可变 Function 配置；经 Executions / Workloads / Connectors 调用 | **进行中 / 不可用**（<code>FN0.1</code> 合约已冻结） |
| **Durable Cell** | 命名、串行化、可休眠的共享状态，落在普通 Service 上 | **进行中 / 不可用** |
| **模型推理** | Key、路由目录、Edge ACL、用量账本；Box 上的 Power 服务 | 总体 **已规划**（<code>I0</code>）；Cloud 侧 Key/路由/Edge/用量组件存在；端到端服务阻塞于 <code>BX0</code> + <code>PW0</code> |
| **Static Web** | Gateway 服务的不可变 Web 发布 | **已规划**（<code>WEB0</code>；Gateway 静态对象目标未实现） |

已承载产品工作的平台基础：

| 基础 | 状态 |
| --- | --- |
| 控制面、PostgreSQL/ORM、Operations/Flow、Outbox、迁移、公共 API | **已验证**（<code>F0</code>） |
| REST / TypeScript 客户端 / CLI / Management MCP 对等 | **已验证**核心（<code>C0.1</code>–<code>C0.2m</code>） |
| Workloads 副本 + Gateway target 投影 | **已验证**（<code>H0.1</code>–<code>H0.2</code>） |
| 仅 Box 执行/构建再认证 | **进行中**（<code>BX0</code>；发布阻塞项） |
| 作为 Box 托管推理 Service 的 A3S Power | **已规划**（<code>PW0</code>） |

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

从 A3S monorepo 根目录（推荐一键路径）：

~~~bash
just up          # 依赖（a3s-box compose，或 Docker 回退）+ 控制面后台
curl http://127.0.0.1:8080/api/v1/health/live
just down        # 停止 API 与本地 compose/docker 依赖
~~~

状态、日志与生成的引导 token 位于 <code>apps/cloud/.a3s/cloud/dev/</code>。优先使用带 [deploy/dev/compose.acl](deploy/dev/compose.acl) 的 <code>a3s-box compose</code>；Box 不可用时，<code>just up</code> 回退到同端口 Docker，并引导迁移/服务 Postgres 角色。

仅在本仓库内：

~~~bash
just up
# 或前台：just cloud
~~~

手动环境（生产形态主体）：

~~~bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud_serving:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_POSTGRES_MIGRATION_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
~~~

服务进程永不跑迁移。一次性 migrator 在 PostgreSQL 可达后、API/Worker/Relay/<code>all</code> 之前运行；生产使用不同的迁移与服务主体。

~~~bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
~~~

<details>
<summary><strong>引导第一个 Organization</strong></summary>

Cloud 只存储 API token 摘要；调用方创建并保留凭证。请求同时创建已接受的基线平台角色策略，并将 <code>PlatformOwner</code> 绑定到同一引导 Principal。

~~~bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: $A3S_CLOUD_BOOTSTRAP_TOKEN" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"$A3S_CLOUD_ADMIN_TOKEN\",\"expiresAt\":null}"
~~~

后续变更使用 <code>Authorization: Bearer ...</code> 与稳定的 <code>idempotency-key</code>。

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

## 按构造保证一致性

| 关注点 | 规范规则 |
| --- | --- |
| 命令并发 | 确切租户范围、幂等键、期望版本与载荷摘要事务校验；漂移或冲突重放失败关闭 |
| 数据库写入 | 聚合、幂等、审计与 Outbox 经 A3S ORM/PostgreSQL 一并提交 |
| 跨系统工作 | A3S Flow saga 与所有者回执调和不确定结果；数据库事务不跨越外部提供商 |
| 速率限制与配额 | Gateway 强制请求限制；所有者准入强制持久配额。Redis 永不成为配额真相 |
| 缓存 | Redis 持有有界、可重建提示；缓存丢失只改延迟，不改正确性 |
| 锁与租约 | PostgreSQL/CAS 拥有 fencing。分布式锁可减竞争，但不能替代版本或 Fleet Claims |
| 派发压力 | A3S Lane 只准入已持久工作；既不拥有 workflow 也不拥有队列真相 |
| 分析 | Doris 消费可重建投影；PostgreSQL 与边界上下文所有者仍是运维真相 |

## DDD 与单一权威

<p align="center">
  <img src="assets/readme/ddd-boundary.svg" width="100%" alt="A3S Cloud DDD 依赖方向：从入站适配器经 Presentation、Application、Domain、向内端口、Infrastructure 提供商与已提交集成事实" />
</p>

Presentation 调用 Application；Application 协调其 Domain 与消费者拥有的端口；Infrastructure 实现这些端口。上下文仅通过同步所有者 Application 合约，或所有者已提交 Outbox 发出的带版本事实协作。

| 关注点 | 唯一权威 | 禁止的重复 |
| --- | --- | --- |
| 租户身份与授权 | Identity + Projects | 适配器本地角色、仅 UI 策略、缓存当真相 |
| 产品含义 | 拥有 Agent、Workflow、Function、Cell、Inference、Application 或 Asset 的上下文 | Runtime/提供商字段当产品状态 |
| 持久协调 | Operations + A3S Flow | 产品重试表或另一 workflow 引擎 |
| 放置与滚动 | Workloads + Fleet | Agent/MCP/Cell/模型/Gateway 专用调度器 |
| 提供商生命周期 | A3S Runtime + A3S Box | 产品域直接调用进程/容器/FaaS |
| 公共流量 | Edge 期望状态 + Gateway 已应用状态 | Cloud 代理或并行 Gateway 发布者 |
| 不可变 / 可变数据 | 共享对象客户端 + Data/S0 | 按产品 S3 客户端当期望状态真相 |
| 集成事实 | 范围感知事务 Outbox + A3S Event | 提交前发布 |
| 配置 | 经 <code>a3s-acl</code> 的 A3S ACL | 兼容解析器或另一配置语言 |

横切顺序固定：认证 → 授权 → 校验 → 幂等 → 事务 → 审计/Outbox → 派发。可观测性与速率限制保护该路径；无一成为第二业务权威。[可执行架构棘轮](docs/architecture-audit.md) 阻止外层导入与重复机制扩散。

## 交付状态

以闸门驱动，而非百分比驱动。截至 **2026-09-10** 的摘要（确切证据与剩余退出见 [ROADMAP.md](ROADMAP.md)）：

| 区域 | 证据状态 |
| --- | --- |
| 基础（<code>F0</code>）：Identity、PostgreSQL/ORM、Flow/Operations、Outbox、API、迁移 | **已验证** |
| 控制面（<code>C0.1</code>–<code>C0.2m</code>） | **已验证**核心；企业 <code>C0.5</code> / 更广 <code>C0.3</code> 切片仍开放 |
| Workloads / Fleet / Gateway 投影（<code>H0.1</code>–<code>H0.2</code>） | **已验证**；多节点 HA / 弹性（<code>H0.3</code>+）进行中 |
| 仅 Box 平台（<code>BX0</code>） | **进行中**（Box 背书生产声明的发布阻塞项） |
| Agent 泳道（<code>A0</code>/<code>A1</code>） | **进行中**；A0.4 与部分 A1 闸门已验证——完整 AaaS 仍受闸门约束 |
| Workflow / Applications / Automations / Cells / Knowledge | 作为完整产品 **进行中 / 不可用** |
| 推理（<code>I0</code>） | **已规划**产品；Cloud Key、路由目录、Edge ACL 演替与用量账本组件存在；Power workers 与端到端 OpenAI 数据面等待 <code>BX0</code> + <code>PW0</code> |
| FaaS / Static Web / Runtime CI/CD / Power | **已规划**或早期基础 |

## 部署模型

- 引导经一个依赖 DAG 安装 PostgreSQL、NATS、S3 兼容存储、Git、OCI Registry、A3S Use Registry、migrator、API、Worker、Relay、Gateway 与可观测性依赖。
- API、Worker、Relay、migrator、Node Agent 与 Gateway 独立扩缩。
- 租户工作负载仅经已准入发布、Workloads/Fleet 放置、Runtime/Box 执行与 Gateway 发布进入。
- 生产 HA 需要 [部署架构](docs/deployment-and-cluster-architecture.md) 中命名的干净安装、升级、回滚、依赖丢失、凭证轮换、存储恢复、节点排空与多副本闸门。

## 接口与配置

| 面 | 契约 | 从这里开始 |
| --- | --- | --- |
| REST/OpenAPI | 带版本 <code>/api/v1</code>、请求 ID、幂等、通用信封 | [指南](docs/openapi.md) · [openapi/v1.json](openapi/v1.json) |
| TypeScript 客户端 | 同一 REST 契约上的维护适配器 | [packages/cloud-client](packages/cloud-client) |
| CLI | 可脚本化结构化输出，无 token 参数 | [cli/README.md](cli/README.md) |
| Management MCP | 无会话、租户授权工具，经同一 Application 处理器 | [docs/management-mcp.md](docs/management-mcp.md) |

Cloud 与 Node Agent 仅接受由 <code>a3s-acl</code> 解析的封闭、已校验 **A3S ACL**。未知字段与不安全时序在启动前失败；Secret 值永不属于 ACL。从 [config/cloud.acl](config/cloud.acl)、[config/node.example.acl](config/node.example.acl) 与 [deploy/production](deploy/production/README.md) 开始。

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

真实提供商与发布认证在隔离 Linux 主机上运行。仓库闸门包括 [跨表面符合性](tools/c0-conformance/README.md)、[Runtime 符合性](tools/runtime-conformance/README.md)、[Box 提供商符合性](tools/box-conformance/README.md)、[工作负载身份符合性](tools/workload-identity-conformance/README.md) 与 [已 pin Gateway 修订](tools/gateway-conformance/gateway-revision)。

</details>

## 文档

| 从这里开始 | 用途 |
| --- | --- |
| [产品路线图](ROADMAP.md) | 闸门状态、依赖、证据、交付顺序 |
| [架构优化与执行路线图](docs/architecture-optimization-roadmap.md) | 执行波次、I0 双轨、完整性并行轨 |
| [技术架构](docs/architecture.md) | 所有权、拓扑、一致性、失败行为 |
| [AI 服务平台](docs/ai-service-platform-architecture.md) | AaaS / WaaS / FaaS / Cell / Inference 组合 |
| [推理计划](docs/inference-plan.md) | I0 合约、Edge ACL、用量、Power/Box 依赖 |
| [集群部署](docs/deployment-and-cluster-architecture.md) | 系统服务、调度、HA |
| [工作负载身份](docs/workload-identity-and-service-connectivity-architecture.md) | 信任、证明、私有发现 |
| [能力架构](docs/platform-capability-architecture.md) | OpenShift-/TokenHub 类结果，不复制 API |
| [架构审计](docs/architecture-audit.md) | 可执行债务棘轮 |
| [生态路线图](docs/project-roadmaps/README.md) | 每个 A3S 子项目的使命与负边界 |

## 许可证

[MIT](LICENSE)
