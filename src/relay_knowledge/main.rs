#[tokio::main]
async fn main() {
    match relay_knowledge::interfaces::cli::run(std::env::args().skip(1)).await {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("{}", error.render_stderr());
            std::process::exit(error.exit_code());
        }
    }
}
