# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud turns Agent, Workflow, Function, Durable Cell, inference, and Web semantics into governed services on A3S Runtime and Box" />
</p>


<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=release" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.85.0" src="https://img.shields.io/badge/REST_contract-1.85.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#how-it-works">架构</a>·
  <a href="#service-outcomes">服务</a>·
  <a href="#quick-start">快速入门</a>·
  <a href="#platform-capabilities">功能</a>·
  <a href="#delivery-status">发货</a>·
  <a href="#documentation">文档</a>
</p>

**A3S Cloud 是一个自托管、代理优先的开发者平台，可将
将租户授权的产品意图转化为耐用的 AaaS、WaaS、FaaS、Durable Cell、
模型推理以及运营商拥有的 CPU/GPU 上的静态 Web 服务
基础设施。** 云拥有产品和期望状态的真相； A3S Flow
协调持久的工作； A3S Runtime定义了一个生命周期合约； A3S Box
执行它； A3S Gateway 是唯一的公共边。

> [!IMPORTANT]
> 架构目标不是可用性声明。能力被释放
> 仅在其真实提供者、故障、恢复、清理、升级和发布之后
> 登机口在 [ROADMAP.md](ROADMAP.md) 中标记为 <code>已验证</code>。

> [!NOTE]
> A3S Cloud 不附带管理仪表板。它确实托管不可变的
> React/Vue 和其他适用于应用程序和代理的租户 Web 版本。那些
> 站点使用与所有其他客户端相同的网关和 API。

> [!TIP]
> 自动 CI 和 Box 一致性仅在推送到 `release` 和拉取时运行
> 请求定位`release`。 `main`分支不会启动这些
> 自动工作流程；使用显式工作流调度条目
> 临时验证运行。

## 它是如何工作的

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud authority map from A3S Gateway through tenant product domains, Identity, PostgreSQL, Flow, Workloads and Fleet, Runtime, Box, supply, storage, dispatch, and observability" />
</p>

该架构对于每项服务都遵循一条路径：

1. **承认：** A3S Gateway 对公共流量进行认证；身份解决了
   准确的安装、组织、项目、环境、主体和
   凭证范围。
2. **提交：**应用程序用例自动写入所需状态，
   通过 A3S ORM 向 PostgreSQL 提供幂等性、审计证据和发件箱事实。
3. **坐标：** A3S Flow 和Operations拥有持久的等待，重试，重放，
   批准、补偿、取消和交付历史记录。
4. **放置并执行：** 工作负载和队列拥有单个 CPU/GPU 调度程序，
   声明、推出、流失和隔离；节点代理收敛A3S Runtime
   <code>任务</code>或<code>服务</code>单元至A3S Box。
5. **发布：** Edge编译完整的版本化路由快照； A3S Gateway
   应用并服务于他们。云永远不会成为第二个请求字节代理。

运行时 CI/CD 使用相同的权威图：构建一次，验证准确的摘要，
推广相同的不可变版本，通过工作负载/队列进行部署，转移
通过 Edge/Gateway 的流量，并回滚到较早承认的版本。
产品域保留发布真相； Flow 保留管道历史记录。

## 服务成果

六个产品成果共享两个执行类，而不是创建六个
运行时堆栈：

- **AaaS — 代理即服务。** 代理拥有对话、执行、
  语义事件、批准、检查点、分叉、工具证据、提供者
  绑定和恢复。有状态代理（例如 A3S Code）以温暖方式运行，
  会话防护运行时<code>Service</code> 单位；有界批次代理可以
  使用<code>任务</code>。
- **WaaS — 工作流即服务。**工作流拥有本体，不可变
  定义和计划、WorkflowRun、HumanTask、类型化节点顺序以及
  结果。 A3S Flow 坐标 Agent、Function、MCP、Inference、Cell、人类、
  连接器、任务和服务节点；没有重复的工作流运行时。
- **FaaS — 功能即服务。** 资产拥有不可变的功能
  发布/简介；它的应用程序门面将每次调用委托给
  执行、工作负载或连接器不拥有另一个生命周期。一个
  函数作为运行时<code>Task</code>运行，无状态
  <code>Service</code>，或外部 FaaS 连接器。无会话 MCP 和呼叫
  从 A3S Code 使用相同的模式。 `FN0.1` 冻结这些组件合约；
  FaaS 保持不可用，直到后来的所有者和生产大门通过。
- **Durable Cell。** Durable Cells 拥有应用程序修订、兼容性、
  保留和部署/存储关联。一个普通的运行时
  <code>Service</code> 提供一个命名的、序列化的、可休眠的状态空间
  适用于人员和多个代理，无需复制代理或工作流程历史记录。
- **模型推理。**推理拥有模型修订、部署、角色
  拓扑、路由策略、使用和评估。它支持独立
  共享 CPU/GPU 上的副本和类型化多节点预填充/解码组
  放置导轨。
- **静态 Web。** 应用程序和资产拥有不可变的 Web 版本。反应，
  Vue 和其他承认的对象由带有缓存的网关直接提供服务，
  CSP、SPA回退、路由和回滚策略； SSR就是普通的
  <code>服务</code>简介。

唯一的通用执行类是**任务**和**服务**。代理，
表达功能、MCP、推理、Cell、构建和云系统行为
通过不可变的消费者拥有的配置文件。 A3S Runtime拥有统一
生命周期合同； A3S Box 提供商实现它。

## 快速开始

### 要求

- Rust 1.88 或更高版本
- PostgreSQL 17 或兼容的受支持版本
- 用于固定外部源获取的 Git CLI
- A3S Box 用于节点本地工作负载/构建执行
- 路由服务的固定A3S Gateway修订版
- 用于生产<code>all</code>、Worker 或 Relay 角色的 NATS JetStream
- Bun 仅适用于 TypeScript 客户端或 CLI 开发

### 启动开发API

~~~bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_POSTGRES_MIGRATION_URL="$A3S_CLOUD_POSTGRES_URL"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
~~~

服务进程从不运行迁移。一次性迁移器追随
PostgreSQL 是可访问的，并且在 API、Worker、Relay 或 <code>all</code> 之前；
生产使用不同的迁移和服务主体。

~~~bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
~~~

直接端口访问为本地开发提供了便利。制作出版
API只能通过A3S Gateway。

<details>
<summary><strong>启动第一个组织</strong></summary>

云仅存储 API 令牌摘要；调用者创建并保留
凭证。下面的请求还创建接受的基线平台角色
策略并将 <code>PlatformOwner</code> 绑定到同一个引导程序主体。
并发相同的请求在重播之前序列化，并且任何策略、审计、
或发件箱故障将回滚完整的身份和权限根。

~~~bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: $A3S_CLOUD_BOOTSTRAP_TOKEN" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"$A3S_CLOUD_ADMIN_TOKEN\",\"expiresAt\":null}"
~~~

随后的突变使用 <code>Authorization: Bearer ...</code> 和一个稳定的
<code>幂等性密钥</code>。

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

凭证来自环境变量或标准输入，并且永远不会
写入 CLI 上下文文件。

## 平台能力

### 建造、供应和推广

- 托管 Git 权威加上外部源修订、webhooks、可重现
  整机构建、出处、拉取请求预览、不可变工件，以及
  保消化促进。
- 独立的 **Git**、**OCI 注册表** 和 **A3S Use 注册表** 权限。
  没有一个被重载来模仿另一种供应类型。
- 受控逻辑模型和模型修订加上不可变的模型权重
  清单/对象、外部集线器解析（例如 ModelScope）、许可证、
  信任策略和可重构节点缓存。
- 一种支持流程的 CI/CD 模型，适用于代理、工作流、功能/MCP、耐用单元、
  推理、静态 Web 和云系统服务版本。
- 托管代理将一份规范的代码拥有的最终发布清单绑定到
  准确的 OCI 和签名的构建来源，然后安装其确定性存档
  通过普通工作负载/运行时路径只读。同样的清单
  拥有就绪路径、活跃路径和优雅关闭间隔；
  调用者无法在部署时削弱生命周期契约。

### 运行、扩展和恢复

- 一种用于 CPU 池、GPU 池、加速器/拓扑的异构调度器
  限制、索赔、反亲和力、帮派安置、维护、配额和
  抢占政策。
- 无状态水平缩放和缩放至零；有状态消耗，单写入器
  隔离、检查点/切换、恢复和位置感知放置。
- 具有独立副本、多节点组的分布式推理，
  预填充/解码分解角色、显式 KV 传输和一个共享
  调度程序而不是第二个推理控制平面。
- 独立可扩展的API、Worker、Relay、迁移器、节点代理和网关
  具有明确准备、迁移、租赁/领导、部署和恢复的角色
  合同。

### 储存、服务和观察

- 通过外部 HTTPS AWS S3 的一种类型化不可变对象客户端或
  S3 兼容存储。云捆绑没有 S3 服务器并且不存在对象
  存储为 POSIX/FUSE。可变卷、备份、恢复、保留和写入器
  fencing属于Data/S0。
- A3S Gateway 拥有 TLS、身份验证、请求限制、路由、模型/代理/
  功能/MCP 端点和租户 Web 交付。云服务保持私有。
- OpenTelemetry 关联日志、指标、跟踪、SLO 和事件。
  不可变的证据由域名所有者保留； Apache Doris 是可选的，
  可重建的分析和 SLO 预测。

### 管理租户和特权访问

- 一种不可变的安装身份和一种受歧视的身份
  跨租户的安装/组织/项目/环境范围合同
  隔离、成员身份、资源授予、配额、凭证、审核、发件箱、
  和生命周期清理。平台事实从来不借用哨兵组织。
- 一个独特的系统管理员 RBAC 平面，用于安装、车队、
  移民、政策、事件和打破玻璃的职责。系统角色从不
  以静默方式授予租户数据或秘密访问权限。
- 租户支持需要一个积极主动的人，一个公认的支持使用角色，
  一项短期的、不可续签的赠款、后代范围和一项封闭的赠款
  非敏感权限。每个都允许引脚可重播策略、凭证、
  约束力、授予、行动、资源和请求证据。
- 新的引导程序自动创建第一个组织，服务主体，
  所有者成员资格、API 令牌、可接受的基准平台角色策略，以及
  匹配 <code>PlatformOwner</code> 与共享审核、发件箱和
  幂等性事实。
- 特权突变和安装范围的组织目录读取使用
  相同的 Identity/PostgreSQL 决策发布者。有效的精确
  <code>cloud:读取</code>凭证，无需
  <code>TenantLifecycleRead</code>只看到自己的组织；撤销，
  过期、不匹配或范围不足的凭据无法关闭。
- 工作负载信任使用不可变的 TrustDomain 和 WorkloadIdentityPolicy
  同一身份机构的修订。当前的、精确的、有限的历史，
  工作负载索引读取加上 CAS 防护接受通过以下方式公开
  REST/OpenAPI、TypeScript 客户端和 CLI，无需调用者编写的参与者，
  凭据或安装覆盖。
- 规范的 <code>cloud.identity.workload-provider.v1</code> 配置文件绑定
  每个 TrustDomain 修订版都通过摘要对一个可替换的提供程序适配器进行修改。的
  仅 API <code>spiffe_https_web</code> 适配器执行新的 HTTPS，
  有界、严格的 JSON SPIFFE 捆绑观察并承认这一点
  准确的修订。其合约将端点证据标记为<code>observed*</code>
  和摘要绑定配置文件策略为<code>声明*</code>；它拥有没有
  证书颁发、私钥、提供商注册表或并行信任
  状态。
- 版本化
  <code>cloud.identity.workload-runtime-evidence-binding.v1</code> 基础
  将一个精确的策略摘要与其工作负载声明、节点池和队列节点绑定
  会话/功能快照，加上运行时单元生成和 Box 提供程序
  证明。已验证的 C2 仅包含工作负载和队列所有者事实。
  纯组件 C3a 之前承认一种通用身份授权
  调度并保留不可变的工作负载记录，包括显式的
  无策略结果，因此崩溃重放无法重新标记遗留或正在运行的单元。
  纯组件 C3b 添加了唯一的身份拥有的不可变属性
<code>cloud.identity.workload-runtime-evidence-record.v1</code> 历史记录
  迁移<code>181</code>。准确的入场重播可能会恢复其历史性
  事实；每个新的写入都会重新读取当前的 TrustDomain/Policy 和两个所有者
  规范安装围栏下的事实，然后通过一种类型提交
  A3S ORM 存储库。 PostgreSQL 拒绝突变、过时的证据和
  没有首先连载的政策/证据竞赛。确定性 V1
  记录仍然缺乏节点硬件证明并且无法授权凭证
  发行； C4 仍然是一个必需的新决定，而不是推断的
  能力。
- OpenShift 级成果——协调、调度、隔离、推出、
  策略、可观察性和第二天操作以及 TokenHub 级
  结果——受控模型/提供商/密钥访问、路由、配额、诊断、
  和使用——由 A3S 权威组成，而不是复制 API 或
  控制平面。

## 构造的一致性

|关注|规范规则 |
| ---| ---|
|命令并发|以事务方式检查确切的租户范围、幂等性密钥、预期版本和有效负载摘要；漂移或冲突重放失败关闭|
|数据库写入|聚合、幂等、审计、Outbox通过A3S ORM/PostgreSQL一起提交；数据库解析规范安装沿袭|
|跨系统工作 | A3S Flow 传奇故事和所有者收据协调不确定的结果；没有数据库事务跨越外部提供商|
|速率限制和配额 |网关强制执行请求限制；所有者入场强制执行持久配额。 Redis 可能会加速计数器，但永远不会成为配额真理 |
|缓存| Redis 拥有有界的、可重构的读取、发现、令牌和协调提示以及修订后的失效；缓存丢失会改变延迟，而不是正确性
|锁和租赁| PostgreSQL/CAS 拥有正确性和防护。分布式锁可以减少争用，但不能替换版本或队列声明 |
|调度压力| A3S Lane 出于公平性、背压和有限并发性的目的，只允许持久工作；它既不拥有工作流也不拥有队列真相|
|分析|多丽丝消耗可重建的遥测/证据投影； PostgreSQL 和有界上下文所有者仍然是可操作的真理

## DDD 和单一权限

<p align="center">
  <img src="assets/readme/ddd-boundary.svg" width="100%" alt="A3S Cloud DDD dependency direction from inbound adapters through Presentation, Application, Domain, inward-owned ports, Infrastructure providers, and committed integration facts" />
</p>

演示调用应用程序；应用程序协调其域和
消费者拥有的港口；基础设施实施这些入境港口。上下文
仅通过同步所有者应用程序合约或
从所有者提交的发件箱发出的版本化事实。

|关注|独家授权|禁止重复 |
| --- | --- | --- |
|租户身份和授权 |身份+项目|适配器本地角色、仅限 UI 的策略、提供程序会话或缓存声明为真 |
|产品含义 |拥有代理、工作流、功能、单元、推理、应用程序或资产上下文 |运行时/提供程序字段成为产品状态 |
|持久协调 |运营 + A3S Flow |产品重试表、睡眠循环或其他工作流程引擎 |
|构建和发布交付 |来源+工件+产品发布所有者+交付管道|产品本地 CI 状态、升级时重建或可变部署标签 |
|放置和推出|工作负载 + 机队 |特定于代理、MCP、单元、模型或网关的调度程序
|提供商生命周期 | A3S Runtime + A3S Box |来自产品领域的直接流程/容器/FaaS 调用 |
|公网流量 |边缘期望状态 + A3S Gateway 应用状态 |云代理、每个产品入口或其他网关发布者 |
|不可变和可变数据 |共享对象客户端+数据/S0 |每个产品的 S3 客户端、备份引擎或提供程序状态为所需状态真相 |
|整合事实|一个范围感知的事务发件箱 + A3S Event |提交前发布、哨兵租户或并行产品/平台事件总线 |
|配置| A3S ACL 由 <code>a3s-acl</code> 解析 |兼容性解析器或其他产品配置语言 |

横切行为遵循一个可见的有序管道：身份验证、
授权、验证、幂等性、交易、审计/发件箱，然后
派遣。日志记录、跟踪、指标、缓存、速率限制和 AOP 拦截器
观察或保护该路径；没有人可以成为第二个商业权威。
[Executable architecture ratchets](docs/architecture-audit.md)止外层
在消除已知债务的同时，避免进口和重复机制的扩散。

## 交货状态

投资组合是门驱动的，而不是百分比驱动的。截至 **2026-09-06**：

|车道 |证据状态 |
| ---| ---|
|租户范围的身份、PostgreSQL/A3S ORM、操作/流程、发件箱、公共 API 和迁移 | **经过验证的基础** |
|安装范围和系统管理员RBAC | **已验证核心，更广泛的大门正在进行中。** 原子新引导程序、策略/绑定和支持授予存储库、精确特权决策、受保护的突变、REST/OpenAPI、TypeScript 客户端、CLI、管理 MCP 和撤销防护组织目录均经过验证。预 root 安装的受控恢复以及更广泛的 MT3 角色矩阵、所有者端口清理和敌对租户证据仍然存在 |
|工作负载、队列、运行时/Box、网关、供应、协作和企业控制 | **进行中; A0.4 真实提供商门已验证。** [A0.4 PostgreSQL/real-Box provider gate](https://github.com/A3S-Lab/Cloud/actions/runs/33686237668/job/100434300332) 和 [complete Cloud CI](https://github.com/A3S-Lab/Cloud/actions/runs/33686237772) 通过 Box `65f3d3fc7c1e0e2cb1ba2d409a79f7357314f5ae` 和 OCI Runtime `878f8414cef3b85bef1b51fe6735017b25828252`；更广泛的组件/提供商门仍然存在
|代理和托管 MCP 产品通道 | **进行中; A0.4 已验证。** A0.4 已发布-Agent 部署、PostgreSQL 持久化、真实 Box 恢复、Secret 重物化、取消和清理门由保留的[provider evidence](https://github.com/A3S-Lab/Cloud/actions/runs/33686237668/job/100434300332) 进行验证； A0.3、A0.5 和托管 MCP 仍受门限限制，因此组件证据并不意味着完整的 AaaS 可用性 |
|本体工作流程和人工智能应用程序/文件| **正在进行中。** 完整的 WaaS 和应用程序产品仍然受限制 |
|自动化| **组件基础正在进行中。** 精确修订 Webhook 准入、计划日历/失火/并发/持久游标租用边界、确定性到期时间信封、具有持久 PostgreSQL 状态的幂等调用准入、可注入有界计划工作人员/规范化事件使用者边界以及摘要验证的调用目标所有者切换。当提供所有者组成的提供者/处理程序时，控制平面主管接受并优雅地停止这些进程。生产候选者发现、目标布线、实时恢复证据和公开可用性仍然开放|
|数据/S0 和耐用电池 | **基础工作正在进行中。** Durable Cell 是一流目标，但尚未成为可用的托管服务 |
|工作负载身份| **经过验证的信任和WI2-C1/C2基础； C3a 和 C3b 在 main 上验证。** [trust/provider main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33291073009)、[C1/C2 main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33310808529) 和 [C1/C2 Box provider conformance](https://github.com/A3S-Lab/Cloud/actions/runs/33310808538) 通过。 C3a生产-通过迁移组成通用身份授权ACL和一个不可变的工作负载预调度绑定/无策略记录`180`；完整的[C3a main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33319781762)和[same-revision Box provider conformance](https://github.com/A3S-Lab/Cloud/actions/runs/33319781830)通行证。 C3b 添加了迁移`181`、一种类型化的不可变身份证据历史、精确的历史重播、当前策略/信任域重新验证、确定性相同事实采用以及保留并发/撤销测试，无需公共 API 或第二所有者生命周期； [C3b main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33327919058) 和 [same-revision Box provider conformance](https://github.com/A3S-Lab/Cloud/actions/runs/33327919079) 通过。舰队硬件证明、完整的发行决定、发行、执行、撤销和联合保持开放 |
| FaaS、分布式推理、模型提供、静态 Web、运行时 CI/CD 和完整的 HA 操作 | **计划的或早期的基础。**它们的架构和权限边界已定义，但完整的产品门仍然存在 |

请参阅 [product roadmap](ROADMAP.md)、[platform gap
analysis](docs/platform-gap-analysis.md) 和 [ecosystem project
roadmaps](docs/project-roadmaps/README.md) 了解确切的依赖性、证据和
剩余的门。

## 部署模型

云系统服务和租户工作负载共享机制但从不借用
权威机构：

- 引导平面安装 PostgreSQL、NATS、S3 兼容存储、Git、
  OCI 注册表、A3S Use 注册表、迁移器、API、Worker、中继、网关和
  通过一个依赖 DAG 的可观察性依赖；
- API、Worker、Relay、迁移器、节点代理和网关独立扩展；
- 租户工作负载仅通过承认的版本、工作负载/队列进入
  放置、运行时/框执行和网关发布；
- 管理是API/OpenAPI/客户端/CLI/MCP优先；没有云仪表板
  或特定于 UI 的后端。

初始 Box 托管的配置文件是安装基础。生产HA
需要指定的全新安装、升级、回滚、依赖丢失、
凭证轮换、存储恢复、节点耗尽和多副本门
[deployment architecture](docs/deployment-and-cluster-architecture.md)。

## 接口和配置

|表面|合同|从这里开始 |
| --- | --- | --- |
|休息/OpenAPI |版本化<code>/api/v1</code>、请求 ID、幂等性、通用信封、提交快照 | [Guide](docs/openapi.md) · [openapi/v1.json](openapi/v1.json) |
| TypeScript 客户端 |通过相同的 REST 合约维护适配器 | [packages/cloud-client](packages/cloud-client) |
|命令行|没有标记参数的可编写脚本的结构化输出[cli/README.md](cli/README.md) |
|管理MCP |相同应用程序处理程序上的无会话、租户授权工具 | [docs/management-mcp.md](docs/management-mcp.md) |

云和节点代理仅接受已关闭、经过验证的 **A3S ACL** 解析
<code>a3s-acl</code>。未知字段和不安全时序关系失败之前
启动；秘密值永远不属于 ACL。开始于
[config/cloud.acl](config/cloud.acl),
[config/node.example.acl](config/node.example.acl)，以及
[deploy/production](deploy/production/README.md) 基线。

Redis 是可选加速，Doris 是可选分析，两者都不是
持久的真理。兼容S3的对象存储和NATS是外部生产的
依赖关系。

## 存储库和开发

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
<summary><strong>核心开发门</strong></summary>

~~~bash
cargo fmt --all -- --check
cargo test -p a3s-cloud-control-plane architecture_tests --lib
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
~~~

真实提供商和发布认证在隔离的 Linux 主机上运行。重要
存储库拥有的门包括 [cross-surface
conformance](tools/c0-conformance/README.md)、[Runtime
conformance](tools/runtime-conformance/README.md)、[Box provider
conformance](tools/box-conformance/README.md)、[workload-identity provider
conformance](tools/workload-identity-conformance/README.md) 和 [pinned
Gateway revision](tools/gateway-conformance/gateway-revision)。

</details>

## 文档

|从这里开始 |目的|
| --- | --- |
| [Product roadmap](ROADMAP.md) |门状态、依赖性、证据和交货单 |
| [Technical architecture](docs/architecture.md) |稳定的所有权、拓扑、一致性和故障行为 |
| [AI service platform](docs/ai-service-platform-architecture.md) | AaaS、WaaS、FaaS、Durable Cell、推理、运行时、Box 和网关组合 |
| [Agent release deployment contract](docs/agent-release-deployment-contract.md) |代码拥有的最终清单生成、出处、持久性、重播和运行时投影 |
| [Agent Runtime](docs/agent-runtime-architecture.md)·[Function Runtime](docs/function-runtime-architecture.md)·[Durable Cell](docs/durable-cell-platform-plan.md) |统一运行时的服务语义|
| [Static Web](docs/static-web-hosting-architecture.md)·[model supply](docs/model-supply-architecture.md)·[inference](docs/inference-plan.md) |租户 UI、模型/权重和服务架构 |
| [Cluster deployment](docs/deployment-and-cluster-architecture.md) · [elastic services](docs/elastic-service-deployment-architecture.md) |系统服务、CPU/GPU 调度、有状态/无状态融合、HA |
| [Runtime CI/CD](docs/runtime-cicd-architecture.md) · [workload identity](docs/workload-identity-and-service-connectivity-architecture.md) |交付、证明、私有发现、mTLS 和撤销 |
| [Distributed API consistency](docs/distributed-api-consistency-architecture.md) · [Redis and Lane](docs/redis-and-lane-platform-architecture.md) |并发、事务、缓存、锁、公平性和背压 |
| [Observability and analytics](docs/observability-and-analytics-architecture.md) · [platform gap analysis](docs/platform-gap-analysis.md) |遥测/SLO/事件设计和优先缺失结果 |
| [Multi-tenant platform](docs/multi-tenant-developer-platform-architecture.md) · [capability architecture](docs/platform-capability-architecture.md) |租户/管理员 RBAC 和 OpenShift-/TokenHub 级结果 |
| [DDD, AOP, and patterns](docs/ddd-aop-and-pattern-architecture.md) · [architecture audit](docs/architecture-audit.md) |层规则、方面顺序、模式和可执行债务棘轮 |
| [Ecosystem roadmaps](docs/project-roadmaps/README.md) |每个 A3S 子项目的使命、依赖性、证据和负面边界 |

## 许可证

[MIT](LICENSE)
