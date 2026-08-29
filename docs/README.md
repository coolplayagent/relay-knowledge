# Documentation Bookshelf

The documentation is maintained as two language editions. Both editions use the
same six-volume taxonomy and link their counterparts directly. Chapter sets may
differ only when an edition explicitly identifies a language-specific deep dive
or a pending translation; the edition index records that mapping instead of
implying silent one-to-one coverage.

- [中文文档](./zh/README.md)
- [English documentation](./en/README.md)

## Directory Policy

- `01-user-guide`: executable user workflows and command guidance.
- `02-capabilities`: implemented product capabilities, organized from foundational behavior to competitive differentiators.
- `03-architecture-specs`: architecture and algorithm whitepaper chapters, hard
  contracts, interface boundaries, and forward product requirements.
- `04-research`: dated research and gap analysis; it may preserve roadmap
  language when the page is explicitly historical.
- `05-benchmarks`: benchmark runs, optimization studies, and performance notes.
- `06-verification`: audit records, validation runs, and dated evidence that a
  documentation or implementation pass was checked.

All content documents inside numbered volumes use a two-digit chapter prefix in
the filename, such as `05-hybrid-retrieval-advantage.md`. `README.md`
files are volume covers and tables of contents; when listed as readable pages
they are treated as chapter 0.

## Naming and Ordering Policy

- Use lowercase kebab-case after the two-digit chapter number. Prefer a concise
  subject name over status words such as `full`, `new`, or `latest`.
- Order user, capability, architecture, and research chapters by reading
  dependency: concepts before workflows, workflows before operations, and
  normative contracts before supporting reference pages.
- Keep matching English and Chinese chapters on the same number. A missing
  translation remains an explicit numbered gap in the other edition rather
  than giving the same chapter two identities.
- Order verification records by evidence date, then from broad system evidence
  to narrower audits on the same date. Their numeric prefixes are navigation
  order, not mutable status or priority.
- Put narrow references and historical detail in indexed child directories.
  Reference pages use an internal two-digit reading order; dated archives use
  an ISO `YYYY-MM-DD-` filename prefix so lexical and chronological order agree.

Every directory that contains Markdown has a `README.md` index. Each index links
its sibling documents and child indexes, chapter prefixes are unique within a
volume, code fences identify their language, and repository-local links must
resolve. These rules keep archived evidence discoverable without mixing it into
the current normative reading path.

Documentation refresh audits belong in `06-verification`, not in
`02-capabilities`, because they prove documentation freshness rather than
describe a user-facing capability. Root-level legacy document paths were removed.
New links should point directly to either `docs/zh/` or `docs/en/`.

## Release Readiness Path

Before tagging a new release, read the documentation in this order:

1. Root release entry points: [`README.md`](../README.md) and
   [`README.zh-CN.md`](../README.zh-CN.md).
2. User installation workflow:
   [`docs/en/01-user-guide/01-install-and-runtime.md`](./en/01-user-guide/01-install-and-runtime.md)
   and
   [`docs/zh/01-user-guide/01-install-and-runtime.md`](./zh/01-user-guide/01-install-and-runtime.md).
3. Release architecture contract:
   [`docs/en/03-architecture-specs/19-installation-release-and-upgrade.md`](./en/03-architecture-specs/19-installation-release-and-upgrade.md)
   and
   [`docs/zh/03-architecture-specs/19-installation-release-and-upgrade.md`](./zh/03-architecture-specs/19-installation-release-and-upgrade.md).
4. Current documentation and self-iteration readiness record:
   [`docs/en/06-verification/13-documentation-self-iteration-readiness-2026-08-18.md`](./en/06-verification/13-documentation-self-iteration-readiness-2026-08-18.md)
   and
   [`docs/zh/06-verification/13-documentation-self-iteration-readiness-2026-08-18.md`](./zh/06-verification/13-documentation-self-iteration-readiness-2026-08-18.md).

The [2026-06-05 documentation audit](./en/06-verification/11-documentation-release-readiness-2026-06-05.md)
remains historical evidence; it is not the current readiness result.

The Chinese edition currently carries detailed deployment, SRE, and security
addenda plus a few benchmark and verification records without standalone
English chapters. The English service chapter consolidates the core operational
workflow, and both edition indexes call out the remaining Chinese-only records
instead of silently omitting them from navigation.

## Documentation Quality Gate

Run the dependency-free checker from the repository root after changing any
documentation:

```bash
python3 tools/docs/check_docs.py
```

On Windows, use the Python launcher from PowerShell:

```powershell
py -3 tools/docs/check_docs.py
```

The checker validates every Markdown file under `docs/`: UTF-8 and whitespace,
one top-level title, ordered heading depth, labelled and balanced code fences,
shell-specific command examples, repository-local link targets and anchors,
directory index coverage, unique two-digit chapter numbers, and untranslated
prose accidentally left in the English edition. Its own parser smoke test is:

```bash
python3 tools/docs/check_docs.py --self-test
```

```powershell
py -3 tools/docs/check_docs.py --self-test
```

The pre-commit hook runs the equivalent combined self-test and repository check.
`check.sh`, the PR documentation job, and the release workflow run both gates
before Rust build or publication work.
