# Type calls and configuration evidence

Use short type names with `repo query --kind callers|callees` when the user
asks for a type's direct member calls. Java, Python, JS/JSX, TS/TSX, C++, C#,
Rust, Go, Kotlin, Scala, Ruby, PHP and Swift supply explicit ownership;
Vue reuses its JS/TS script. C, Bash and Starlark keep ordinary function
queries. SQL/build/template syntax does not receive synthetic class members.

Results retain actual call sites. Nested functions/types, inherited methods
and unproven dynamic receivers do not become direct members. Preserve empty
directional answers. The 64-type, 1,024-member and 200-call-candidate ceilings
are explicit budgets; narrow a failed expansion to a member.

For `repo feature-flags --source`, use the CLI help's canonical identifier:
`java`, `python`, `javascript`, `jsx`, `typescript`, `tsx`, `c`, `cpp`,
`csharp`, `rust`, `go`, `kotlin`, `scala`, `ruby`, `php`, `swift`, `starlark`
or `vue`. Existing `shell`, `ctmpl`, `dotenv`, `properties` and `ini` remain
valid. Structured build/configuration formats use their own canonical ids.

Configuration reads, static keys, zero-argument getters and conditions are
connected only through indexed evidence in the authorized snapshot. Explicit
imports or native type/module identities can connect files. An environment
variable and a property key retain separate namespaces even with identical
names. Source filters retain a group's connected evidence.

Check `analysis_complete`, unresolved references and `flow_incomplete` before
drawing absence conclusions. Reassignment, shadowing, dynamic keys, unknown
calls and unsupported conversions can stop tracing. Static defaults are not
runtime values: Python/JS Boolean conversion of the string `false` differs
from C# Boolean parsing. Do not generalize Java inheritance or package rules
to another language.

After this extraction upgrade, rebuild through `repo index` or `repo update`.
Fact version `config-registry-v55-type-ownership` and deferred query-index
plan v4 prevent old indexes from claiming the new capability. Retain normal
durable tasks, leases, checkpoints and status reporting during recovery.

Cross-file evidence has language-specific limits. JS/TS imports require a
matching export and explicit source extension. Rust imports require indexed
`mod` membership (including static `#[path]`), rather than a matching file
name. PHP includes require an `__DIR__` anchor. Plain relative Shell sources
retain unknown working-directory evidence; later source/eval/unset can
invalidate getters. Inspect unresolved metadata before suggesting a missing
configuration definition. Kotlin companions have their own member set.

Schema marker 9 adds the durable ownership cursor. A resumed writer preserves
its lease and publication barrier; a resource-limit error on an in-place
update calls for a durable staged rebuild, not larger unbounded budgets.

Rust root membership is proved through indexed `mod` edges; bin/test/example
crates are separate, and ambiguous/shared roots remain unresolved. Swift typed
imports require one physical module directory, with at most 1,024 Swift file
facts. Reassigned getter providers and unsupported native aliases cannot justify
cross-file reads. A non-nullable conversion does not trigger a `??` fallback.

Java zero-argument getter names are unrestricted. Known method/property rewrites
invalidate local expansion as well as exported provider proof. C# relative aliases
retain namespace scope; `global::` is explicit. C++ primary templates and concrete
specializations retain separate ownership. Swift constructors, requirements and
subscripts use actual member ranges. Dockerfile evidence remains stage/import
coverage, without evaluating arbitrary commands. A detached C++ implementation
with a named concrete template argument remains unresolved when the argument's
namespace binding cannot be proved.
