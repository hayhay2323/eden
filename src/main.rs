use eden::cli::{parse_cli_command, run_cli_query, CliCommand};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let args = std::env::args().collect::<Vec<_>>();
    let command = match parse_cli_command(&args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(2);
        }
    };

    if matches!(command, CliCommand::UsLive) {
        if let Err(error) = eden::us::run().await {
            eprintln!("US runtime failed: {}", error);
            std::process::exit(1);
        }
        return;
    }

    if !matches!(command, CliCommand::Live) {
        if let Err(error) = run_cli_query(command).await {
            eprintln!("Query failed: {}", error);
            std::process::exit(1);
        }
        return;
    }

    eden::hk::runtime::run().await;
}
