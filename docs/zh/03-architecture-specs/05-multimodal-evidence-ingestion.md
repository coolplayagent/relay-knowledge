# 多模态证据摄取

[中文](../../zh/03-architecture-specs/05-multimodal-evidence-ingestion.md) | [English](../../en/03-architecture-specs/05-multimodal-evidence-ingestion.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

摄取层的目标不是把所有内容变成文本，而是把不同模态转成同一套 evidence contract。文本、图片、OCR、caption、表格、布局区域和代码片段都应保留来源、父子关系、置信度和提取状态。

## 2. Evidence 统一模型

Evidence 至少表达：source scope、source path、span 或 asset region、modality、content hash、parent evidence、extraction method、confidence、lifecycle status 和 created graph version。

派生 evidence 必须引用父 evidence：

```text
image asset
  -> OCR text evidence
  -> caption evidence
  -> layout/table region evidence
  -> image embedding metadata
```

检索时按父 evidence 聚合派生命中，避免把同一图片的 OCR、caption 和 embedding 当成三份重复上下文。

## 3. 摄取流水线

```text
source discovery
  -> scope normalization
  -> evidence write
  -> extraction task enqueue
  -> worker extraction
  -> proposal or derived evidence commit
  -> mutation log
  -> index refresh request
```

摄取只负责创建原始 evidence 和有界后台任务；OCR、caption、embedding、表格抽取和大型解析不得在查询热路径运行。

## 4. Worker 边界

Worker task 必须有 kind、scope、input evidence id、attempt、lease、timeout、budget、redacted config snapshot 和 output contract。外部模型或 OCR service 失败时，任务进入 retry/dead-letter，不允许写入半结构化事实绕过验证。

## 5. 去重与版本

- content hash 用于识别重复 source payload。
- extraction output hash 用于避免重复提交派生 evidence。
- evidence lifecycle 支持 proposed、accepted、rejected、superseded。
- 新提取结果不能覆盖旧 evidence，只能追加新版本或建立 supersedes 关系。

## 6. 验收标准

- 任一派生 evidence 都能追溯到原始 source 和 worker attempt。
- 查询结果能按父 evidence 聚合多模态命中。
- 失败的外部提取不会阻塞已有文本或图检索。

---

导航: 上一章: [4. Source Scope 模型](04-source-scope-model.md) | 下一章: [6. 图事实模型与版本化](06-graph-fact-model-and-versioning.md)

Python overload 绑定证明在 `await` 与产出值的操作处终止，因为协议调用或挂起期间的其他执行可能在装饰器求值前修改提供者。向语法上为空的内置 tuple、list 或 dictionary 执行 `yield from` 不会触发用户协议或挂起。match 捕获名称绑定到对应词法目标；点分值引用、映射键、类名和关键字标签不是捕获目标。仅当有界证明确认零参数直接调用沿直线路径同步到达当前装饰器时，才可以在该调用处固定延迟查找的结果。异步函数、生成器、条件路径或前置退出不能建立该证明；未调用的嵌套函数中的 yield 不会使外层函数成为生成器。三项分析共同消耗原有证明预算。

首次同步调用不能证明后续调用（包括别名调用）仍使用同一提供者。在该调用处固定证明还要求有界检查确认后续只有可证不触发执行的线性语句；后续调用、控制流或导入都会使该快捷证明失效。

直接调用证据还检查装饰器之前的导入：导入模块可能执行代码并替换提供者。未调用嵌套函数内的导入仍不属于当前执行路径。直接调用位于赋值右侧时，快捷证明要求左侧为没有注解的普通名称；属性和下标写入可能在调用返回后执行 setter，并用另一个装饰器提供者再次进入函数，解包与注解也保留未证明的副作用。这些未知情况保留可执行分类和 canonical 歧义，每个 symbol snapshot 仍保留自身调用边；检查继续共享既有证明预算。

C/C++ callable signature key 按结构规范化标准内置类型说明符：可省略的 `int`、整数类型显式 `signed` 与合法说明符顺序不会把等价声明分开（如 `unsigned`/`unsigned int`、`long`/`long int` 和 `long double`/`double long`）。规范化继续消耗既有每 callable 与共享每文件工作预算，显示签名保持原文；不会依据平台位宽合并不同整数等级或符号性、普通 `char` 与 signed/unsigned `char`、不同浮点类型或不同指针、引用、成员限定。typedef、依赖宏的 token、数组调整、不支持的声明形状与证据预算耗尽仍生成未知 key，并要求有歧义的 canonical 请求显式选择 symbol snapshot。真实 Git 回归在等价与不同内置类型间验证声明拥有的 callers 和精确 snapshot 选择；旧 completed scope 必须按新 C/C++ source-fact 组件重建后使用新 key。
