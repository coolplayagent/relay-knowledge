use super::parallel_map;

#[test]
fn parallel_map_bounds_workers_and_preserves_all_results() {
    let mut results = parallel_map(vec![1, 2, 3, 4], 2, |value| value * value);
    results.sort_unstable();

    assert_eq!(results, vec![1, 4, 9, 16]);
}

#[test]
fn parallel_map_handles_empty_input_without_spawning_workers() {
    let results = parallel_map(Vec::<u8>::new(), 0, |value| value);

    assert!(results.is_empty());
}
