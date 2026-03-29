use super::foundation::{ApiKeyCipher, ApiKeyRevocationStore, ApiState};
use super::core::build_router;
use crate::core::settings::ApiInfraConfig;

pub async fn serve(bind_addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let auth = ApiKeyCipher::from_env()?;
    let api_config = ApiInfraConfig::load()
        .map_err(|error| Box::new(std::io::Error::other(error)) as Box<dyn std::error::Error>)?;
    let revocations = ApiKeyRevocationStore::load(api_config.revocation_path.clone())
        .map_err(|error| Box::new(std::io::Error::other(error.to_string())) as Box<dyn std::error::Error>)?;
    #[cfg(feature = "persistence")]
    let store = {
        crate::persistence::store::EdenStore::open(&api_config.db_path)
            .await
            .map_err(|error| -> Box<dyn std::error::Error> {
                Box::new(std::io::Error::other(error.to_string()))
            })?
    };

    let state = ApiState {
        auth,
        revocations,
        #[cfg(feature = "persistence")]
        store,
    };

    let app = build_router(state)?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
