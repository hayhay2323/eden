use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures::stream;
use serde::Serialize;

use super::constants::CASE_STREAM_INTERVAL_SECS;
use super::core::sse_event_from_error;
use super::foundation::{ApiError, JsonEventStream};

pub(in crate::api) fn json_poll_sse<T, F, Fut, R, RFut>(
    loader: F,
    revision_loader: R,
) -> Sse<JsonEventStream>
where
    T: Serialize + Send + 'static,
    F: Fn() -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = Result<T, ApiError>> + Send + 'static,
    R: Fn() -> RFut + Clone + Send + 'static,
    RFut: std::future::Future<Output = Result<String, ApiError>> + Send + 'static,
{
    let stream = stream::unfold(
        (None::<String>, None::<String>, true),
        move |(mut last_revision, mut last_payload, first)| {
            let loader = loader.clone();
            let revision_loader = revision_loader.clone();
            async move {
                let mut first = first;
                loop {
                    if !first {
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            CASE_STREAM_INTERVAL_SECS,
                        ))
                        .await;
                    }
                    first = false;

                    let revision = match revision_loader().await {
                        Ok(revision) => revision,
                        Err(error) => {
                            let message = format!("stream_revision:{error}");
                            if last_payload.as_ref() == Some(&message) {
                                continue;
                            }
                            last_payload = Some(message.clone());
                            return Some((
                                Ok(sse_event_from_error(&message)),
                                (last_revision, last_payload, false),
                            ));
                        }
                    };

                    if last_revision.as_ref() == Some(&revision) {
                        continue;
                    }
                    last_revision = Some(revision);

                    let (event, fingerprint) = match loader().await {
                        Ok(payload) => match serde_json::to_string(&payload) {
                            Ok(json) => {
                                if last_payload.as_ref() == Some(&json) {
                                    continue;
                                }
                                (SseEvent::default().data(json.clone()), json)
                            }
                            Err(error) => {
                                let message = format!("encode_error:{error}");
                                if last_payload.as_ref() == Some(&message) {
                                    continue;
                                }
                                (sse_event_from_error(&message), message)
                            }
                        },
                        Err(error) => {
                            let message = format!("stream_error:{error}");
                            if last_payload.as_ref() == Some(&message) {
                                continue;
                            }
                            (sse_event_from_error(&message), message)
                        }
                    };

                    last_payload = Some(fingerprint);
                    return Some((Ok(event), (last_revision, last_payload, false)));
                }
            }
        },
    );

    let stream: JsonEventStream = Box::pin(stream);
    Sse::new(stream).keep_alive(
        KeepAlive::default()
            .interval(tokio::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
}

pub(in crate::api) async fn latest_file_revision(
    candidates: impl IntoIterator<Item = String>,
) -> Result<String, ApiError> {
    let mut best: Option<(u64, std::time::SystemTime, String)> = None;

    for path in candidates {
        let Ok(metadata) = tokio::fs::metadata(&path).await else {
            continue;
        };
        let modified = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &best {
            Some((_, best_modified, _)) if modified <= *best_modified => {}
            _ => best = Some((metadata.len(), modified, path)),
        }
    }

    let Some((len, modified, path)) = best else {
        return Err(ApiError::internal("failed to stat any stream artifact candidate"));
    };

    let modified = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|| "0".into());

    Ok(format!("{len}:{modified}:{path}"))
}
