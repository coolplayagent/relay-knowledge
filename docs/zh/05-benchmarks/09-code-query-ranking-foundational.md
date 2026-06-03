# Code Query Foundational Ranking Notes

2026-06-03 的 full self-iteration foundational 修复收紧了 `repo query` 在 imports 与 references kind 下的通用排序信号。

- Imports: 无测试意图的 import 查询会更明确地下调测试路径命中；extensionless relative module specifier（例如 `./redaction`）会更重视靠前的直接 import site；bare import syntax（例如 `import "./protocol"`）会优先匹配 dynamic/side-effect import expression，而不是静态 named/type import 声明；imports 查询只会在 target-symbol 查询或已有目标符号证据时读取同文件 usage context，并对每个 path 的上下文 chunk 扫描设上限。
- References: type reference 查询会轻微提升参数名与类型名语义匹配的 callable 参数，例如 `instance: InstanceContext`，使直接业务入口优先于泛型 `ctx` 参数或被动字段。
- 约束: 这些规则不枚举仓库、路径、case id 或符号名，仍由真实 code graph、import/reference 行、路径类别和 source chunk evidence 驱动。
