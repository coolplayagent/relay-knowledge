use std::io::{IsTerminal, Write};

#[tokio::main]
async fn main() {
    let interactive_text_output =
        std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match relay_knowledge::interfaces::cli::run_process(args.clone(), interactive_text_output).await
    {
        Ok(output) => {
            print!("{}", output.stdout);
            let _ = std::io::stdout().flush();
            eprint!("{}", output.stderr);
            if let Some(notice) = relay_knowledge::interfaces::cli::process_update_notice(
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
