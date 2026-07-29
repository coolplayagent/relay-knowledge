fn read_pipe<R>(mut reader: R) -> JoinHandle<String>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut output = String::new();
        let _ = reader.read_to_string(&mut output);
        output
    })
}

fn write_pipe<W>(mut writer: W, input: String) -> JoinHandle<Result<(), String>>
where
    W: Write + Send + 'static,
{
    std::thread::spawn(move || {
        writer
            .write_all(input.as_bytes())
            .map_err(|error| error.to_string())
    })
}

fn join_reader(reader: Option<JoinHandle<String>>) -> String {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn append_stdin_error(stderr: &mut String, writer: Option<JoinHandle<Result<(), String>>>) {
    let Some(writer) = writer else {
        return;
    };
    let error = match writer.join() {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "stdin writer thread panicked".to_owned(),
    };
    if !stderr.is_empty() {
        stderr.push('\n');
    }
    stderr.push_str("stdin write failed: ");
    stderr.push_str(&error);
}
