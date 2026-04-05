use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::oneshot;

use super::http::HttpState;
use crate::agent::{Input, Output};

#[derive(Deserialize)]
pub struct StreamChatRequest {
    message: String,
    #[serde(default = "default_session")]
    session_id: String,
}

fn default_session() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn stream_chat(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<StreamChatRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let inbound_tx = state.inbound_tx.clone();
    let session_id = req.session_id.clone();

    let stream = async_stream::stream! {
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<String>(64);

        let input = Input {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            content: req.message.clone(),
            stream_tx: Some(stream_tx),
        };

        let (reply_tx, reply_rx) = oneshot::channel::<Output>();

        yield Ok::<_, Infallible>(Event::default()
            .event("status")
            .data(r#"{"type":"thinking"}"#));

        if inbound_tx.send((input, reply_tx)).await.is_err() {
            yield Ok(Event::default()
                .event("error")
                .data(r#"{"error":"Agent worker unavailable"}"#));
            return;
        }

        // Spawn a task to wait for the final result
        let result_handle = tokio::spawn(async move {
            tokio::time::timeout(std::time::Duration::from_secs(120), reply_rx).await
        });

        // Stream text chunks as they arrive
        while let Some(chunk) = stream_rx.recv().await {
            yield Ok(Event::default()
                .event("text_delta")
                .data(serde_json::json!({"text": chunk}).to_string()));
        }

        // Channel closed — agent is done. Get the final result.
        match result_handle.await {
            Ok(Ok(Ok(output))) => {
                if let Some(usage) = &output.usage {
                    yield Ok(Event::default()
                        .event("usage")
                        .data(serde_json::json!({
                            "input_tokens": usage.input_tokens,
                            "output_tokens": usage.output_tokens,
                        }).to_string()));
                }
                yield Ok(Event::default()
                    .event("done")
                    .data(serde_json::json!({"session_id": session_id}).to_string()));
            }
            _ => {
                yield Ok(Event::default()
                    .event("error")
                    .data(r#"{"error":"Request failed or timed out"}"#));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
