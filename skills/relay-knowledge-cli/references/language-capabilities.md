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
Fact version `config-registry-v56-portable-evidence-source-io-isolation-v1` and deferred query-index
plan v5 prevent old indexes from claiming the new capability. Retain normal
durable tasks, leases, checkpoints and status reporting during recovery.

Cross-file evidence has language-specific limits. JS/TS imports require a
matching export and explicit source extension. Rust imports require indexed
`mod` membership (including static `#[path]`), rather than a matching file
name. PHP includes require an `__DIR__` anchor. Plain relative Shell sources
retain unknown working-directory evidence; later source/eval/unset can
invalidate getters. Inspect unresolved metadata before suggesting a missing
configuration definition. Kotlin companions have their own member set.

Schema marker 10 retains the durable ownership cursor. A resumed writer preserves
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


Type-call aggregation preserves actual call byte and line ranges; ordinary function queries retain their existing context line ranges. Anonymous callbacks keep their own call owner; a matching member name on an unknown receiver never proves a target. JS static and instance `this` are separate; Java permits instance-qualified static calls. Class headers and computed member names do not establish the new class's `this` binding.

Ruby bracket reads and Rust `env::var_os` contribute environment evidence; pure writes and Python/JS method selectors do not become keys. Rust import provenance follows the nearest lexical scope and distinguishes local `std` modules from `::std`. Go package constants can compose keys across files using at most 32 string components, four snapshot resolution levels and 4,096 result bytes. Mutable values, non-string providers, getters used as constants and cycles remain unresolved; different unknown expressions retain separate identities.

Extensionless scripts require a supported interpreter shebang within the first 256 bytes; watcher admission preserves their update/delete events. A leading Flow pragma selects the existing typed JSX grammar while preserving JS/JSX identity and ordinary partial diagnostics for unsupported syntax. This is limited syntax recovery, not complete Flow type analysis. Vue counts only top-level SFC regions against the 16-region budget and reuses its parsed HTML tree.

Old snapshots have unknown call bytes. Reindex through the normal durable workflow for v56 evidence. If a migrated binding schema is missing, restore its runtime backup or rebuild in a new home; do not treat a new empty projection as current.

Repository and request language rules both apply, including their shared manifest paths. Do not reuse checkpoints solely because language unions match. `excluded` file rows are progress records, not query evidence or degradation. An incomplete source-fallback result cannot establish absence. Public language options and `lang:` qualifiers accept language names, never internal scope encodings. cgo calls require import/binding evidence; private C targets and ambiguous candidates remain unresolved.

Scope reuse also requires the requested evidence languages to be covered; shared manifest paths alone are insufficient. cgo keeps lossy or incomplete linkage signatures unresolved. C++ recovered decorated classes use their complete declaration identity; macro namespace wrappers remain opaque. Detached member ownership can use already resolved conditional angle-bracket includes, within the existing 64-target and writer budgets.

Shared JVM manifests retain distinct Java/Kotlin/Scala evidence. POM dependency coordinates are read structurally even on one line; arbitrary plugin configuration is not a dependency declaration. XML event errors detected by the reader, ambiguous coordinates or bounded-reader exhaustion fail the index task explicitly, so an unsuccessful run must not be interpreted as no dependencies. This fragment reader does not validate the complete POM/XML document structure.

Java static type receivers can resolve through package or explicit imports plus AST modifier/ownership proof. Shadowed, inherited or ambiguous bindings remain unresolved; repeated qualified targets are not disambiguated by file proximity. Display signatures alone cannot prove static dispatch.

Java `this` calls involving Object names or enum/record members remain unresolved without overload proof; anonymous bodies, including enum-constant bodies, are excluded from the enclosing type’s direct members.
