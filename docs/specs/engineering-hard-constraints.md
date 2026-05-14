# 工程硬约束

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: `relay-knowledge` 全部 Rust 代码、文档、基础运行时模块、网络与 HTTP 能力
> 约束级别: Hard constraints。新增代码、重构、文档和后续 LLM 生成内容都必须遵守。

## 1. 设计结论

`relay-knowledge` 不能演进成由浅包装函数、未使用代码和散落平台逻辑堆成的项目。基础能力必须集中在明确模块中，网络能力必须从第一天按高并发、事件驱动和可治理的运行时来设计。

硬约束如下:

1. **禁止浅函数**: 不新增只改名、只转发、只包一层调用、没有不变量或策略价值的函数。
2. **禁止死代码**: 不保留未使用模块、函数、类型、字段、feature、测试夹具、注释掉的代码或投机性扩展点。
3. **文档必须完整**: 公共模块、配置、路径、网络能力、故障模式、运维行为和安全边界必须有对应文档。
4. **参数必须可解释**: CLI 参数、API 字段、配置项、环境变量、输出格式和枚举值必须有清晰语义、默认值、边界、失败模式和示例，供人类、脚本、skill 和 LLM 正确使用。
5. **基础模块必须存在且边界清晰**: `env` 管理环境变量，`paths` 管理路径，`net` 管理全部网络能力，HTTP 归属 `net`。
6. **模块之间禁止循环依赖**: Rust crate、模块、trait、service、adapter 和配置对象的依赖图必须是有向无环图。
7. **HTTP 必须事件驱动**: HTTP server/client 必须基于操作系统事件通知模型接入异步运行时，不能采用每连接一个线程或阻塞 socket 模型。
8. **网络必须支持超大并发接入能力**: 设计必须包含连接预算、内存预算、背压、超时、取消、限流、观测指标和压测目标。
9. **必须有 QoS 模块**: 网络与 HTTP 接入必须经过 `net::qos` 的准入、优先级、限流和资源预算策略。
10. **文件最大 1000 行**: 任何纳入版本控制的文件都不能超过 1000 行，超出前必须拆分职责或拆分文档。
11. **UT 覆盖率必须大于 90%**: 单元测试行覆盖率必须保持在 90% 以上，不能依赖集成测试掩盖缺失的单元覆盖。
12. **分层测试与 Playwright Chromium 浏览器集成测试必须可用**: 整个项目必须分 UT 和集成测试两层验证；集成测试层必须像 `relay-teams` 一样安装 Playwright Chromium 并运行浏览器集成测试。

## 2. 禁止浅函数

函数必须至少满足以下一项:

- 维护领域不变量或运行时不变量。
- 执行有意义的校验、解析、转换、聚合或错误归一化。
- 隔离外部边界，例如环境变量、文件系统路径、网络、存储、进程、时间、随机数或操作系统 API。
- 编排可观测、可取消、可超时、有背压的工作流。
- 把复杂策略封装成可测试的单元，并有明确调用方。

禁止模式:

- 只调用另一个同签名或近似同签名函数的 pass-through wrapper。
- 为了“看起来分层”而拆出的单行函数。
- 没有额外语义的 getter、builder step、转换函数或名称别名。
- 只返回固定字符串、固定路径或固定默认值的函数；应优先使用常量、配置类型或带校验的构造函数。
- 先写空函数、TODO 函数或占位模块等待未来补充。

允许的薄边界必须有明确理由，例如 trait adapter、FFI/平台边界、测试替身边界、错误类型映射、telemetry span 边界、资源生命周期边界或公共 API 稳定性边界。代码审查时必须能说明该函数新增了什么语义。

## 3. 禁止死代码

死代码包括但不限于:

- 没有生产调用方且没有被测试覆盖的函数、类型、字段、模块和文件。
- 未接入的 feature flag、配置项、环境变量、CLI 参数和 HTTP 路由。
- 注释掉的旧实现、临时实验代码、未使用的 mock、未被引用的 fixture。
- 只为未来假设保留的抽象层、trait、enum variant 或扩展点。
- 长期存在的 TODO、panic 占位、unimplemented 占位和 unreachable 伪逻辑。

约束:

- 新增 public API 必须有生产调用路径，或在 specs 中记录为明确扩展点，并提供测试验证其 contract。
- 不允许用 `#[allow(dead_code)]`、`#[allow(unused)]` 或类似方式隐藏死代码，除非是生成代码、平台条件编译或外部协议要求；例外必须写明原因和移除条件。
- 删除能力时必须同步删除过期测试、文档、配置、feature、示例和 CI 钩子。
- 条件编译代码必须至少有一个 CI 或本地测试路径能覆盖其 contract。

## 4. 文档完整性

文档是交付物的一部分。以下变化必须更新文档:

- 新增或改变 public module、public type、CLI/API 行为、配置、环境变量、路径、网络端口、协议、后台服务或数据目录。
- 新增 `env`、`paths`、`net`、`net::http`、`net::qos` 相关行为。
- 影响安装、升级、回滚、卸载、服务运行、日志、缓存、索引、数据库、迁移或诊断的行为。
- 新增故障模式、降级策略、超时、重试、背压、限流、QoS 或安全边界。

公共模块文档至少说明:

- 模块职责和禁止承载的职责。
- 输入、输出、错误类型和重要不变量。
- 配置来源、默认值和优先级。
- 资源使用边界，例如连接数、队列深度、超时、内存预算、磁盘目录。
- 可观测字段，例如日志事件、metrics、trace attribute 和 health 状态。
- 测试策略和不需要外部服务的验证方式。

## 4A. 参数可解释性

参数解释是接口 contract 的一部分，不是附属说明。任何新增或改变 CLI 参数、API request/response 字段、配置项、环境变量、输出格式、枚举值或 Web operation 字段的变更，都必须同步说明它如何被人类、脚本、skill 和 LLM 正确使用。

要求:

- 每个参数必须说明语义、类型、是否必填、默认值、允许取值、是否可重复、单位、边界值和失败模式。
- 每个参数必须说明读写影响，例如只读查询、写入图状态、刷新索引、写入 service definition、改变后台 operator state 或启动常驻服务。
- 命令行参数必须在 CLI 自描述输出中可发现；skill 和 LLM 应优先使用机器可读规格，而不是解析自然语言帮助文本。
- 同名参数在不同命令中含义不同的，必须在命令局部解释清楚，例如 `--kind` 在 `repo query`、`index refresh` 和 `worker` 中代表不同枚举。
- 默认值、隐式行为、废弃策略和兼容别名必须有测试覆盖，避免后续重构让参数含义漂移。

禁止:

- 新增只有名字、没有解释、没有测试、没有文档入口的参数。
- 让参数含义依赖调用者猜测、README 示例或源码实现细节。
- 只给人工 `--help` 文案但不提供可机器消费的说明，除非该参数明确不是 CLI/API/配置/agent 暴露面。
- 用模糊名称隐藏有副作用的行为，例如写入状态、刷新索引、启动网络监听或变更 service manager 文件。

## 5. 基础模块边界

### 5.1 `env`

`env` 是唯一管理环境变量的模块。

职责:

- 读取、解析、校验和归一化环境变量。
- 定义环境变量命名、前缀、默认值、废弃策略和错误信息。
- 把环境变量转换为 typed config，不把原始字符串散落到业务模块。
- 记录敏感字段处理规则，避免 secret 进入日志、metrics、trace 和错误文本。

禁止:

- 在 application、domain、storage、indexing、retrieval、net、interfaces 中直接调用 `std::env::var` 读取配置。
- 在多个模块中重复定义同一个环境变量名。
- 让环境变量覆盖路径、安全或 QoS 策略却没有文档和测试。

### 5.2 `paths`

`paths` 是唯一管理路径的模块。

职责:

- 解析平台目录，包括 config、data、state、cache、log、temp、runtime、service 和 install 目录。
- 保持二进制安装目录与运行时状态目录分离。
- 提供路径校验、创建策略、权限策略、迁移策略和 dry-run 预览。
- 支持 Linux、macOS、Windows 的平台约定。

禁止:

- 在业务模块中手写平台目录、拼接家目录或依赖当前工作目录保存运行状态。
- 把数据库、索引、日志、缓存、dead-letter、临时文件默认放进仓库目录或 release 解压目录。
- 在没有 `paths` contract 的情况下新增配置文件、数据文件或日志文件位置。

### 5.3 `net`

`net` 是全部网络能力的所有者。

职责:

- 管理 TCP、UDP、HTTP、TLS、DNS、proxy、连接池、监听 socket、client、server、超时、重试和网络观测。
- 对外提供小接口，业务模块只表达意图，不直接创建 socket、HTTP client 或 HTTP server。
- 集成 `net::qos`，所有入站和出站网络操作都必须经过资源预算和限流策略。
- 暴露网络 health、连接数、队列深度、丢弃数、超时数、限流数、握手失败数和重试数。

禁止:

- 在 `net` 之外直接依赖具体 HTTP 框架类型、socket 类型或网络运行时细节。
- 让 domain、application、storage 或 indexing 持有 HTTP framework request/response 类型。
- 为单个功能临时创建未治理的 HTTP client、监听端口或后台网络循环。

## 6. 模块依赖无环

模块之间不能出现循环依赖。这里的“循环依赖”不仅包括 Rust crate 级别无法编译的循环，也包括逻辑上的双向耦合，例如两个模块互相持有具体类型、trait 定义放错层导致上下层互相引用、service 互相调用、adapter 反向依赖 application 的同时 application 又依赖 adapter 类型。

依赖规则:

- 依赖方向必须稳定保持为 `interfaces -> api -> application -> domain`，外层适配器依赖内层 contract，内层不能反向依赖外层实现。
- `env`、`paths`、`net` 是基础边界模块。业务模块可以通过 typed config 或小接口使用其结果，但基础模块不能依赖业务模块。
- `domain` 只能承载领域模型、不变量和领域错误，不能依赖 application、storage、indexing、retrieval、net、interfaces 或具体 telemetry 后端。
- 需要依赖倒置时，trait 和公共 contract 必须放在更内层或专门的 contract 模块中，具体实现放在外层 adapter 中。
- 如果两个模块需要互相引用数据结构，必须抽取共享类型到更低层模块，或重新划分职责，不能用 callback、全局状态、lazy singleton 或 trait object 绕出隐式环。
- 新增模块时必须在文档或模块级注释中说明允许依赖的上游/下游边界。

禁止模式:

- `application` 调用 `interfaces` 或 HTTP framework 类型。
- `domain` 为了方便直接导入 storage、net、env、paths 或 observability 实现。
- `net` 依赖具体业务 service 来决定协议、限流或错误语义；应通过通用 request context、QoS policy 和 typed error 交互。
- 两个 service 通过互相持有引用、channel 或 callback 形成启动、关闭或错误处理环。
- 为了解决编译顺序问题创建“common”垃圾模块，把所有类型无边界地塞进去。

发现循环依赖时必须重构边界。优先顺序是: 抽取更小的领域类型、把 trait 下沉到调用方需要的内层 contract、把实现上移到 adapter、拆分 orchestration 与 pure policy、删除不必要的双向调用。

## 7. HTTP 与 QoS 约束

HTTP 归属 `net::http`，QoS 归属 `net::qos`。

HTTP 实现要求:

- 必须基于操作系统事件驱动能力，例如 Linux `epoll`、macOS/BSD `kqueue`、Windows IOCP，通常通过成熟异步运行时或 HTTP 库使用。
- 必须使用非阻塞 I/O，禁止每连接一个线程、阻塞 accept/read/write、busy polling 或无界任务生成。
- 必须支持 graceful shutdown、request cancellation、timeout、keep-alive 管理、连接预算、请求体大小限制和流式响应背压。
- HTTP adapter 只能调用统一 API/application service，不能复制业务逻辑，不能直接访问数据库、索引或 mutation log。
- 大并发目标必须通过压测或基准文档定义，包括目标连接数、请求率、内存上限、延迟预算、降级行为和测试环境。

QoS 实现要求:

- `net::qos` 必须定义 admission control、per-source/per-tenant 限流、优先级、连接预算、请求预算、队列预算和超时预算。
- 超出预算时必须明确拒绝、排队、降级或取消，不能无界等待或无界占用内存。
- QoS 决策必须可观测，至少记录被拒绝、被限流、超时、取消、降级和队列等待。
- 后台同步、索引刷新、LLM/embedding 调用、Web/API 请求必须能使用不同优先级和预算。

## 8. 文件长度与测试质量

### 8.1 文件最大 1000 行

任何纳入版本控制的文件都不能超过 1000 行，包括 Rust 源码、测试、文档、配置、脚本和工作流文件。新增内容会让文件超过 1000 行时，必须先拆分。

拆分原则:

- Rust 代码按职责拆到更小模块，不能为了绕过行数限制制造循环依赖或浅函数。
- 测试文件按行为域、接口或模块拆分，避免一个测试文件承载整个系统。
- 文档按主题、运行模式或规格边界拆分，并在索引文档中互相链接。
- 工作流和脚本按独立 gate 或复用脚本拆分，避免单文件承载所有 CI 逻辑。

禁止通过压缩格式、超长行、合并无关职责或生成难读代码绕过 1000 行限制。

### 8.2 UT 覆盖率必须大于 90%

单元测试行覆盖率必须大于 90%。覆盖率门槛适用于核心 Rust 代码和后续新增的 Python/TypeScript 等生产代码；每种语言都必须使用该语言可靠的覆盖率工具单独统计。

Rust 默认要求:

- 使用 `cargo llvm-cov` 或等价可审计工具统计 unit-test 覆盖率。
- CI 中必须以 `--all-targets --all-features` 或等价范围运行覆盖率检查。
- 新模块必须优先补 `#[cfg(test)]` 单元测试，覆盖不变量、错误分支、边界值和取消/超时/背压策略。
- 集成测试可以补充端到端信心，但不能替代单元测试覆盖率门槛。

覆盖率下降到 90% 或以下时，必须先补测试或收窄无效代码；不能通过排除关键模块、移动代码到未统计目录或删除有价值分支来满足数字。

### 8.3 分层测试与 Playwright Chromium 浏览器集成测试门禁

分层测试是整个项目的硬要求，不是某一种语言或某一个目录的可选规则。CI、setup 脚本和本地开发命令必须至少区分两层:

- **UT 层**: 快速、确定、隔离外部服务，负责覆盖领域不变量、解析、错误分支、资源预算、边界值和纯策略。
- **集成测试层**: 验证 CLI/API/Web adapter、文件系统边界、网络边界、服务组合、跨模块 contract 和真实运行时 wiring。

集成测试层必须具备浏览器集成测试能力，并像 `relay-teams` 一样安装 Playwright Chromium 后再运行浏览器集成测试。即使当前核心实现是 Rust，也必须为未来 Web、HTTP、CLI-to-Web、诊断界面和服务控制台保留可执行的浏览器测试门禁，避免交互层只靠手工验证。

要求:

- 使用锁定的 dev/test 依赖环境安装 Playwright 测试工具；如果采用 Python harness，默认使用 `uv sync --extra dev --no-default-groups`。
- 在浏览器集成测试前安装 Chromium，例如 `uv run --extra dev python -m playwright install --with-deps chromium` 或等价命令。
- CI 必须设置稳定的浏览器缓存目录，例如 `PLAYWRIGHT_BROWSERS_PATH`，避免每个步骤使用不同浏览器安装位置。
- 浏览器集成测试必须覆盖关键 Web/API 交互路径、流式输出、错误状态、超时/取消、服务健康和可观测诊断入口。
- Playwright Chromium 安装失败、浏览器集成测试失败或测试环境无法启动，都必须阻塞 PR。
- CI 不能只跑 `cargo test` 或只跑端到端测试；必须显式包含 UT gate 和 integration gate。

当前 PR workflow 已显式拆分 format、clippy、unit/integration test、coverage、build
和 browser integration gate。coverage gate 使用 `cargo llvm-cov --all-targets --all-features --fail-under-lines 90`。
browser integration gate 使用 `PLAYWRIGHT_BROWSERS_PATH`、`uv sync --extra dev --no-default-groups`、
`uv run --extra dev python -m playwright install --with-deps chromium` 和 `uv run --extra dev pytest tests/browser`。
- 集成测试失败、Playwright Chromium 浏览器测试失败或 UT 覆盖率不达标，都必须阻塞合并。

禁止把 Web/API/诊断界面改动以“暂无前端”或“临时页面”名义绕过浏览器集成测试门禁。

## 9. 代码审查与验证门槛

每个 PR 或 LLM 生成变更必须自查:

- 是否新增了浅函数；如果有薄边界，是否有明确语义和测试。
- 是否留下死代码、未接入配置、未使用 feature 或注释掉的实现。
- 是否更新了对应 docs、README、安装发布规格或运行诊断文档。
- 是否为新增或修改的参数补全可解释性说明、机器可读 CLI metadata、默认值、边界、失败模式和测试。
- 是否把环境变量、路径和网络能力放进 `env`、`paths`、`net` 的正确边界。
- 是否引入模块、crate、trait、service、adapter 或配置对象之间的循环依赖。
- 是否新增或保留超过 1000 行的版本控制文件。
- UT 行覆盖率是否大于 90%，新增逻辑是否有针对性单元测试。
- 是否保持 UT 与集成测试两层 gate，集成测试层是否已安装 Playwright Chromium 并运行浏览器集成测试。
- HTTP 是否事件驱动、非阻塞、可背压、可取消、可超时，并接入 `net::qos`。
- 是否运行 `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings` 和 `cargo test --all-targets --all-features`。
