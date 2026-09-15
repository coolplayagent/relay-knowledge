use std::io::{IsTerminal, Write};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    // This mode is a disposable filesystem worker, before any configuration,
    // service, database, or network startup. Its parent owns its lifetime.
    #[cfg(windows)]
    if let Some(result) = relay_knowledge::paths::windows_probe_worker(&args) {
        match result {
            Ok(output) => println!("{output}"),
            Err(error) => {
                println!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    #[cfg(windows)]
    if let Err(error) = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|path| {
            relay_knowledge::paths::initialize_windows_probe_executable(path)
                .map_err(|error| error.to_string())
        })
    {
        eprintln!("{error}");
        std::process::exit(1);
    }
    run_cli(args);
}

#[tokio::main]
async fn run_cli(args: Vec<String>) {
    let interactive_text_output =
        std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
    match relay_knowledge::bootstrap::cli::run_process(args.clone(), interactive_text_output).await
    {
        Ok(output) => {
            print!("{}", output.stdout);
            let _ = std::io::stdout().flush();
            eprint!("{}", output.stderr);
            if let Some(notice) = relay_knowledge::bootstrap::cli::process_update_notice(
                args,
                interactive_text_output,
            )
            .await
            {
                eprint!("{notice}");
            }
        }
        Err(error) => {
            eprintln!("{}", error.render_stderr());
            std::process::exit(error.exit_code());
        }
    }
}
