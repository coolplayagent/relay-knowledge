    #[test]
    fn shell_split_keeps_quoted_argument() {
        assert_eq!(
            shell_split("tool run \"hello world\" --file {prompt_file}").expect("split"),
            vec!["tool", "run", "hello world", "--file", "{prompt_file}"]
        );
    }
