# Code Query Foundational Ranking Notes

2026-06-03 的 full self-iteration foundational 修复收紧了 `repo query` 在 imports 与 references kind 下的通用排序信号。

- Imports: 无测试意图的 import 查询会更明确地下调测试路径命中；extensionless relative module specifier（例如 `./redaction`）会更重视靠前的直接 import site，且该早行号信号可以压过 target-symbol/usage context 的轻微相邻路径偏好；bare import syntax（例如 `import "./protocol"`）会优先匹配 dynamic/side-effect import expression，而不是静态 named/type import 声明；imports 查询只会在 target-symbol 查询或已有目标符号证据时读取同文件 usage context，并对每个 path 的上下文 chunk 扫描设上限。
- References: type reference 查询会轻微提升参数名与类型名语义匹配的 callable 参数，例如 `instance: InstanceContext`，使直接业务入口优先于泛型 `ctx` 参数或被动字段。
- Definition source fallback: 当 read model/FTS/vtable 异常导致 definition 查询没有 indexed hits 时，仍会先通过索引内的受限候选路径查询定位文件；候选为空时底层 source grep 保持空结果，不会退化成无界仓库扫描。
- Hybrid chunks and repo-set: test/benchmark 与 generated chunk 降权保留正分，避免唯一候选被过滤；generated chunk 可由 header 或通用 generated path segment 识别；type-surface companion phrase 不参与 compound identifier 输入；repository-set domain-affinity bonus 只在 fresh 且有 overlay evidence 支持 priority 时生效。
- 约束: 这些规则不枚举仓库、路径、case id 或符号名，仍由真实 code graph、import/reference 行、路径类别和 source chunk evidence 驱动。
