use eden::cli::{parse_cli_command, run_cli_query, CliCommand};
use std::fs;
use std::path::PathBuf;

fn install_live_crash_hook(market_label: &str) {
    let market = market_label.to_string();
    std::panic::set_hook(Box::new(move |panic_info| {
        let payload = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|value| value.to_string())
            .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".into());
        let location = panic_info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".into());
        let message = format!(
            "[{} runtime panic] {} @ {}",
            market.to_uppercase(),
            payload,
            location
        );
        eprintln!("{}", message);

        let mut path = PathBuf::from("runtime");
        let _ = fs::create_dir_all(&path);
        path.push(format!("{}_crash.log", market));
        let _ = fs::write(path, format!("{}\n", message));
    }));
}

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

    if matches!(command, CliCommand::Live | CliCommand::UsLive) && !cfg!(feature = "persistence") {
        eprintln!(
            "Live sensory runtime requires a persistence-enabled build. Re-run with `cargo run --features persistence --bin eden -- <live|us>`."
        );
        std::process::exit(2);
    }

    if matches!(command, CliCommand::UsLive) {
        install_live_crash_hook("us");
        eprintln!("[main] entering us live runtime");
        eden::us::run().await;
        return;
    }

    if !matches!(command, CliCommand::Live) {
        if let Err(error) = run_cli_query(command).await {
            eprintln!("Query failed: {}", error);
            std::process::exit(1);
        }
        return;
    }

    install_live_crash_hook("hk");
    eprintln!("[main] entering hk live runtime");
    eden::hk::runtime::run().await;
}
