#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    if let Err(error) = eden::us::run().await {
        eprintln!("eden-us failed: {error}");
        std::process::exit(1);
    }
}
