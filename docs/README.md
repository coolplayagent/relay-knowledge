# Documentation Bookshelf

The documentation is maintained as two language editions. Each edition uses the
same numbered book structure so links, chapters, specifications, research notes,
benchmarks, and verification records stay easy to compare.

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

Documentation refresh audits belong in `06-verification`, not in
`02-capabilities`, because they prove documentation freshness rather than
describe a user-facing capability. Root-level legacy document paths were removed.
New links should point directly to either `docs/zh/` or `docs/en/`.
