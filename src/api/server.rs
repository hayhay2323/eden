use super::core::build_router;
use super::foundation::{ApiKeyCipher, ApiKeyRevocationStore, ApiState};
use crate::core::runtime_tasks::RuntimeTaskStore;
use crate::core::settings::ApiInfraConfig;

pub async fn serve(bind_addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[api boot] init auth");
    let auth = ApiKeyCipher::from_env()?;
    eprintln!("[api boot] load config");
    let api_config = ApiInfraConfig::load()
        .map_err(|error| Box::new(std::io::Error::other(error)) as Box<dyn std::error::Error>)?;
    eprintln!("[api boot] load revocations");
    let revocations =
        ApiKeyRevocationStore::load(api_config.revocation_path.clone()).map_err(|error| {
            Box::new(std::io::Error::other(error.to_string())) as Box<dyn std::error::Error>
        })?;
    eprintln!(
        "[api boot] load runtime task registry {}",
        api_config.runtime_tasks_path
    );
    let runtime_tasks = RuntimeTaskStore::load(api_config.runtime_tasks_path.clone()).map_err(
        |error| -> Box<dyn std::error::Error> { Box::new(std::io::Error::other(error)) },
    )?;
    #[cfg(feature = "persistence")]
    let store = {
        eprintln!("[api boot] open persistence store {}", api_config.db_path);
        crate::persistence::store::EdenStore::open(&api_config.db_path)
            .await
            .map_err(|error| -> Box<dyn std::error::Error> {
                Box::new(std::io::Error::other(error.to_string()))
            })?
    };

    let state = ApiState {
        bind_addr,
        auth,
        revocations,
        runtime_tasks,
        #[cfg(feature = "persistence")]
        store,
    };

    eprintln!("[api boot] build router");
    let app = build_router(state)?;
    eprintln!("[api boot] bind listener {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    eprintln!("[api boot] serve loop starting");
    axum::serve(listener, app).await?;
    Ok(())
}
