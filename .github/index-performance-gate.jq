([.evaluation.gates[] | select(.passed | not)] | length) == 0 and
([.evaluation.cases[] | select(.passed | not)] | length) == 0 and
([.evaluation.repositories[] |
  select(.repository == "index_performance_many_files")] | length) == 1 and
(.evaluation.repositories[] |
  select(.repository == "index_performance_many_files") |
  (.index_summary.cold.task.state == "succeeded") and
  (.index_summary.cold.checkpoint.state == "completed") and
  (.index_summary.incremental.summary.changed_path_count == 3) and
  (.index_summary.incremental.summary.progress.blob_read_count == 2) and
  (.index_summary.incremental.summary.progress.parsed_file_count == 2) and
  any(.commands[];
    .name == "index_performance_many_files_cold_index_completion" and
    .exit_code == 0) and
  any(.commands[];
    .name == "index_performance_many_files_incremental_index_completion" and
    .exit_code == 0) and
  any(.metrics[];
    .name == "index_performance_many_files_cold_index_ms" and
    (env.ENFORCE_LATENCY != "true" or
     (.budget != null and .value <= .budget))) and
  any(.metrics[];
    .name == "index_performance_many_files_cold_register_index_ms" and
    (env.ENFORCE_LATENCY != "true" or
     (.budget != null and .value <= .budget))) and
  any(.metrics[];
    .name == "index_performance_many_files_incremental_index_ms" and
    (env.ENFORCE_LATENCY != "true" or
     (.budget != null and .value <= .budget))))
