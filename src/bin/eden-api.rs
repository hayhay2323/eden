use eden::{default_bind_addr, serve, ApiInfraConfig, ApiKeyCipher, ApiKeyRevocationStore};
use std::io;

fn usage() -> &'static str {
    "usage: cargo run --bin eden-api -- serve [--bind <host:port>]\n       cargo run --bin eden-api -- mint-key [--label <value>] [--ttl-hours <value>] [--scope <value>]\n       cargo run --bin eden-api -- revoke-key --api-key <value>"
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    if let Err(error) = run(std::env::args().collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    match args.get(1).map(|value| value.as_str()) {
        None | Some("serve") => {
            let rest = if args.len() > 2 { &args[2..] } else { &[] };
            let bind = parse_bind_arg(rest)?;
            let config = ApiInfraConfig::load().map_err(usage_error_owned)?;
            config.log_startup(cfg!(feature = "persistence"));
            println!("eden-api starting on http://{bind}");
            serve(bind).await?;
            Ok(())
        }
        Some("mint-key") => {
            let mut label = "frontend".to_string();
            let mut ttl_hours = 24 * 30;
            let mut scope = "frontend:readonly".to_string();

            let mut index = 2usize;
            while index < args.len() {
                let flag = args[index].as_str();
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| usage_error(&format!("missing value for {flag}")))?;
                match flag {
                    "--label" => label = value.clone(),
                    "--ttl-hours" => {
                        ttl_hours = value
                            .parse::<u64>()
                            .map_err(|_| usage_error(&format!("invalid ttl-hours: {value}")))?
                    }
                    "--scope" => scope = value.clone(),
                    _ => {
                        return Err(
                            usage_error(&format!("unknown flag: {flag}\n{}", usage())).into()
                        )
                    }
                }
                index += 2;
            }

            let cipher = ApiKeyCipher::from_env()?;
            let minted = cipher.mint_key(&label, ttl_hours, Some(&scope))?;
            println!("{}", serde_json::to_string_pretty(&minted)?);
            Ok(())
        }
        Some("revoke-key") => {
            let mut api_key: Option<String> = None;
            let mut index = 2usize;
            while index < args.len() {
                let flag = args[index].as_str();
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| usage_error(&format!("missing value for {flag}")))?;
                match flag {
                    "--api-key" => api_key = Some(value.clone()),
                    _ => {
                        return Err(
                            usage_error(&format!("unknown flag: {flag}\n{}", usage())).into()
                        )
                    }
                }
                index += 2;
            }

            let api_key =
                api_key.ok_or_else(|| usage_error("missing required --api-key for revoke-key"))?;
            let config = ApiInfraConfig::load().map_err(usage_error_owned)?;
            let cipher = ApiKeyCipher::from_env()?;
            let claims = cipher.decode(&api_key)?;
            let store = ApiKeyRevocationStore::load(config.revocation_path)
                .map_err(|error| usage_error(&error.to_string()))?;
            store.revoke(&claims.token_id)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "revoked": true,
                    "token_id": claims.token_id,
                    "label": claims.label,
                    "scope": claims.scope
                }))?
            );
            Ok(())
        }
        Some("--help") | Some("-h") | Some("help") => {
            println!("{}", usage());
            Ok(())
        }
        Some(_) => Err(usage_error(usage()).into()),
    }
}

fn parse_bind_arg(args: &[String]) -> Result<std::net::SocketAddr, Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Ok(default_bind_addr()?);
    }

    if args.len() != 2 || args[0] != "--bind" {
        return Err(usage_error(usage()).into());
    }

    Ok(args[1].parse()?)
}

fn usage_error(message: &str) -> io::Error {
    io::Error::other(message.to_string())
}

fn usage_error_owned(message: String) -> io::Error {
    io::Error::other(message)
}
