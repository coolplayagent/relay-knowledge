# Elastic Long Budgets for Large Repository Indexing

Large repository indexing no longer uses one fixed 180-second hard timeout. The 180-second value remains a historical baseline for regression comparison; the execution budget scales with repository size and observed throughput.

Elastic mode is enabled by default: omitting `index_budget_mode` is equivalent to `elastic`. Only an explicitly selected fixed/strict mode disables scale-based calculation. In elastic mode, the evaluator counts authorized Git files with `git ls-files`; when available, that observed count replaces `expected_file_count`.

The budget is calculated in this order:

1. `N / baseline_files_per_second × 1000` when a throughput baseline is configured.
2. Otherwise, `baseline_index_budget_ms × N / baseline_file_count`.
3. Clamp the result to `max_index_budget_ms`.

Registration adds the bounded `register_overhead_budget_ms`. The process timeout receives only a finite recovery margin; it does not bypass checkpoint, lease, freshness, or completion requirements.

The elastic model preserves the durable indexing contract: bounded batches and queues, a single writer per repository, staging manifests, atomic fact/FTS/checkpoint publication, attempt-scoped leases, orphan recovery, checkpoint replay, and stale/degraded reporting until finalization is complete.

Reports should include observed file count, baseline values, computed budget, cap, cold-index duration, checkpoint progress, and final freshness. A task that is still progressing within its budget is not success until its durable checkpoint and finalization complete.

The Linux kernel example uses 93,601 files, a historical 34,150-file/180-second baseline, approximately 80 files per second, and an 1,800-second cap.

Run the performance evaluation with:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```
