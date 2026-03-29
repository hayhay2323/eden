use std::convert::Infallible;
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::http::StatusCode;
use axum::response::sse::Event as SseEvent;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use futures::Stream;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::serde::rfc3339;
use time::OffsetDateTime;

#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;

const DEFAULT_API_SCOPE: &str = "frontend:readonly";
pub(crate) const API_KEY_PREFIX: &str = "eden_pk_";

#[derive(Clone)]
pub struct ApiState {
    pub(super) auth: ApiKeyCipher,
    pub(super) revocations: ApiKeyRevocationStore,
    #[cfg(feature = "persistence")]
    pub(super) store: EdenStore,
}

#[derive(Clone)]
pub struct ApiKeyCipher {
    cipher: Arc<Aes256Gcm>,
}

pub type JsonEventStream = Pin<Box<dyn Stream<Item = Result<SseEvent, Infallible>> + Send>>;

#[derive(Clone)]
pub struct ApiKeyRevocationStore {
    path: Arc<String>,
    revoked_token_ids: Arc<RwLock<std::collections::HashSet<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyClaims {
    pub label: String,
    pub scope: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub token_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MintedApiKey {
    pub api_key: String,
    pub label: String,
    pub scope: String,
    #[serde(with = "rfc3339")]
    pub issued_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug)]
pub struct ApiError {
    pub(super) status: StatusCode,
    pub(super) message: String,
}

impl ApiError {
    pub(super) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub(super) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub(super) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub(super) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    #[cfg(not(feature = "persistence"))]
    pub(super) fn not_implemented(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            message: message.into(),
        }
    }

    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub(super) fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorBody {
            error: self.message,
        });
        (self.status, body).into_response()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiKeyCipher {
    pub fn from_env() -> Result<Self, ApiError> {
        let secret = env::var("EDEN_API_MASTER_KEY")
            .map_err(|_| ApiError::internal("EDEN_API_MASTER_KEY is not set"))?;
        Self::from_secret(&secret)
    }

    pub fn from_secret(secret: &str) -> Result<Self, ApiError> {
        if secret.trim().is_empty() {
            return Err(ApiError::internal("EDEN_API_MASTER_KEY is empty"));
        }
        let key = Sha256::digest(secret.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| {
            ApiError::internal(format!("failed to initialize API key cipher: {error}"))
        })?;
        Ok(Self {
            cipher: Arc::new(cipher),
        })
    }

    pub fn mint_key(
        &self,
        label: &str,
        ttl_hours: u64,
        scope: Option<&str>,
    ) -> Result<MintedApiKey, ApiError> {
        if label.trim().is_empty() {
            return Err(ApiError::bad_request("API key label cannot be empty"));
        }
        if ttl_hours == 0 {
            return Err(ApiError::bad_request("ttl_hours must be greater than 0"));
        }
        if ttl_hours > 24 * 365 {
            return Err(ApiError::bad_request("ttl_hours is too large"));
        }

        let issued_at = OffsetDateTime::now_utc();
        let expires_at = issued_at + time::Duration::hours(ttl_hours as i64);
        let claims = ApiKeyClaims {
            label: label.trim().to_string(),
            scope: scope.unwrap_or(DEFAULT_API_SCOPE).to_string(),
            issued_at: issued_at.unix_timestamp(),
            expires_at: expires_at.unix_timestamp(),
            token_id: random_token_id(),
        };
        let payload = serde_json::to_vec(&claims).map_err(|error| {
            ApiError::internal(format!("failed to encode API key claims: {error}"))
        })?;

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, payload.as_ref())
            .map_err(|error| ApiError::internal(format!("failed to encrypt API key: {error}")))?;

        let mut token_bytes = nonce_bytes.to_vec();
        token_bytes.extend(ciphertext);
        let api_key = format!("{API_KEY_PREFIX}{}", URL_SAFE_NO_PAD.encode(token_bytes));

        Ok(MintedApiKey {
            api_key,
            label: claims.label,
            scope: claims.scope,
            issued_at,
            expires_at,
        })
    }

    pub fn decode(&self, raw_key: &str) -> Result<ApiKeyClaims, ApiError> {
        let token = raw_key
            .trim()
            .strip_prefix(API_KEY_PREFIX)
            .ok_or_else(|| ApiError::unauthorized("invalid API key prefix"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|_| ApiError::unauthorized("invalid API key encoding"))?;
        if bytes.len() <= 12 {
            return Err(ApiError::unauthorized("invalid API key payload"));
        }

        let (nonce_bytes, ciphertext) = bytes.split_at(12);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| ApiError::unauthorized("invalid API key"))?;
        serde_json::from_slice(&plaintext)
            .map_err(|_| ApiError::unauthorized("invalid API key claims"))
    }

    pub fn validate(&self, raw_key: &str) -> Result<ApiKeyClaims, ApiError> {
        let claims = self.decode(raw_key)?;

        let now = OffsetDateTime::now_utc().unix_timestamp();
        if claims.expires_at <= now {
            return Err(ApiError::unauthorized("API key has expired"));
        }

        Ok(claims)
    }
}

impl ApiKeyRevocationStore {
    pub fn load(path: impl Into<String>) -> Result<Self, ApiError> {
        let path = path.into();
        let revoked = match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str::<Vec<String>>(&content)
                .map_err(|error| {
                    ApiError::internal(format!(
                        "failed to parse API key revocation store `{path}`: {error}"
                    ))
                })?
                .into_iter()
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::collections::HashSet::new()
            }
            Err(error) => {
                return Err(ApiError::internal(format!(
                    "failed to read API key revocation store `{path}`: {error}"
                )))
            }
        };

        Ok(Self {
            path: Arc::new(path),
            revoked_token_ids: Arc::new(RwLock::new(revoked)),
        })
    }

    pub fn path(&self) -> &str {
        self.path.as_ref()
    }

    pub fn revoked_count(&self) -> usize {
        self.revoked_token_ids
            .read()
            .map(|items| items.len())
            .unwrap_or(0)
    }

    pub fn is_revoked(&self, token_id: &str) -> bool {
        self.revoked_token_ids
            .read()
            .map(|items| items.contains(token_id))
            .unwrap_or(false)
    }

    pub fn revoke(&self, token_id: &str) -> Result<(), ApiError> {
        {
            let mut items = self.revoked_token_ids.write().map_err(|_| {
                ApiError::internal("failed to lock API key revocation store for write")
            })?;
            items.insert(token_id.to_string());
            self.persist_locked(&items)?;
        }
        Ok(())
    }

    fn persist_locked(
        &self,
        items: &std::collections::HashSet<String>,
    ) -> Result<(), ApiError> {
        let path = std::path::Path::new(self.path.as_ref());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ApiError::internal(format!(
                    "failed to create revocation store parent directory `{}`: {}",
                    parent.display(),
                    error
                ))
            })?;
        }
        let payload = serde_json::to_string_pretty(
            &items.iter().cloned().collect::<Vec<_>>(),
        )
        .map_err(|error| {
            ApiError::internal(format!("failed to encode revocation store: {error}"))
        })?;
        fs::write(path, payload).map_err(|error| {
            ApiError::internal(format!(
                "failed to write API key revocation store `{}`: {}",
                path.display(),
                error
            ))
        })
    }
}

pub fn default_bind_addr() -> Result<SocketAddr, ApiError> {
    crate::core::settings::ApiInfraConfig::load_bind_addr().map_err(ApiError::bad_request)
}

fn random_token_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
