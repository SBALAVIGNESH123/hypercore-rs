use crate::engine::llama::{InferenceRequest, InferenceResponse};
use crate::server::openai::{
    ApiErrorDetail, ApiErrorResponse, ChatCompletionChunk, ChatCompletionChunkChoice,
    ChatCompletionDelta, ChatCompletionRequest,
};
use async_stream::stream;
use axum::{
    http::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::sse::{Event, Sse},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[derive(Clone)]
struct AppState {
    request_tx: mpsc::Sender<InferenceRequest>,
    drain_rx: tokio::sync::watch::Receiver<bool>,
}

pub async fn start_server(
    host: &str,
    port: u16,
    request_tx: mpsc::Sender<InferenceRequest>,
    drain_rx: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let state = AppState {
        request_tx,
        drain_rx,
    };

    let cors_layer = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/chat/completions", post(completions_handler))
        .layer(middleware::from_fn(auth_middleware))
        .layer(cors_layer)
        .layer(Extension(state))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024)); // SEC-2: 2MB body limit

    let addr = format!("{}:{}", host, port);
    info!("Starting HYPERCORE API Server on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// API key authentication middleware.
/// If HYPERCORE_API_KEY is set, all requests to /v1/* must include a matching Bearer token.
/// Health and metrics endpoints are exempt.
async fn auth_middleware(req: Request<axum::body::Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();

    // Health and metrics are always public
    if path == "/health" || path == "/metrics" {
        return next.run(req).await;
    }

    // Only enforce auth if HYPERCORE_API_KEY is set
    if let Ok(expected_key) = std::env::var("HYPERCORE_API_KEY") {
        let auth_header = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let provided_key = auth_header.strip_prefix("Bearer ").unwrap_or("");

        if provided_key != expected_key {
            warn!("Auth rejected: invalid or missing API key for {}", path);
            let err = ApiErrorResponse {
                error: ApiErrorDetail {
                    message: "Invalid or missing API key. Set Authorization: Bearer <key>"
                        .to_string(),
                    r#type: "authentication_error".to_string(),
                    param: None,
                    code: Some("invalid_api_key".to_string()),
                },
            };
            return (StatusCode::UNAUTHORIZED, Json(err)).into_response();
        }
    }

    next.run(req).await
}

async fn health_handler() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn models_handler() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [{
            "id": "hypercore-model",
            "object": "model",
            "created": 0,
            "owned_by": "hypercore",
            "permission": [],
            "root": "hypercore-model",
            "parent": null
        }]
    }))
}

async fn metrics_handler() -> impl IntoResponse {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = vec![];
    let metric_families = crate::metrics::prometheus_sink::REGISTRY.gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

async fn completions_handler(
    Extension(state): Extension<AppState>,
    Json(payload): Json<ChatCompletionRequest>,
) -> Response {
    if *state.drain_rx.borrow() {
        let err = ApiErrorResponse {
            error: ApiErrorDetail {
                message: "Server is shutting down. Not accepting new requests.".to_string(),
                r#type: "server_error".to_string(),
                param: None,
                code: Some("service_unavailable".to_string()),
            },
        };
        return (StatusCode::SERVICE_UNAVAILABLE, Json(err)).into_response();
    }

    info!("API: Received chat completion request");

    let capacity = state.request_tx.capacity();
    if capacity == 0 {
        let err = ApiErrorResponse {
            error: ApiErrorDetail {
                message: "Server is currently overloaded. Please try again later.".to_string(),
                r#type: "server_error".to_string(),
                param: None,
                code: Some("rate_limit_exceeded".to_string()),
            },
        };
        return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
    }

    if payload.messages.is_empty() {
        let err = ApiErrorResponse {
            error: ApiErrorDetail {
                message: "Messages array cannot be empty".to_string(),
                r#type: "invalid_request_error".to_string(),
                param: Some("messages".to_string()),
                code: None,
            },
        };
        return (StatusCode::BAD_REQUEST, Json(err)).into_response();
    }

    crate::metrics::events::dispatch(crate::metrics::events::MetricEvent::QueueDepthUpdated {
        depth: 1024 - capacity,
    });

    // ChatML prompt template for instruction-tuned models
    let prompt = payload
        .messages
        .iter()
        .map(|m| format!("<|im_start|>{}\n{}<|im_end|>", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n<|im_start|>assistant\n";

    // Heuristic prompt token estimate (~4 chars per token)
    let estimated_prompt_tokens = prompt.len() / 4;

    // 1.5 Pre-Queue Fast Token Heuristic (Soft Guard)
    // Roughly 4 characters per token. If prompt length is > 8192 * 6, it's definitively too large.
    if prompt.len() > 8192 * 6 {
        let err = ApiErrorResponse {
            error: ApiErrorDetail {
                message:
                    "Pre-validation: Prompt is definitively too large for 8192 context window."
                        .to_string(),
                r#type: "invalid_request_error".to_string(),
                param: None,
                code: Some("context_length_exceeded".to_string()),
            },
        };
        return (StatusCode::BAD_REQUEST, Json(err)).into_response();
    }

    let (response_tx, mut response_rx) = mpsc::channel(100);
    let cancel = CancellationToken::new();
    let session_id = crate::SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as usize;
    let model_id = payload
        .model
        .clone()
        .unwrap_or_else(|| "hypercore-model".to_string());
    let request_id = uuid::Uuid::new_v4().to_string();

    let req = InferenceRequest {
        request_id,
        prompt,
        response_tx,
        cancel: cancel.clone(),
        session_id,
        priority: 1, // Normal priority for API requests
        timeline: Default::default(),
        max_tokens: payload.max_tokens,
        temperature: payload.temperature,
    };

    let is_streaming = payload.stream.unwrap_or(false);

    // Submit job to engine queue
    if let Err(e) = state.request_tx.send(req).await {
        error!("API Engine Queue Error: {:?}", e);
        let err = ApiErrorResponse {
            error: ApiErrorDetail {
                message: "Internal engine fault.".to_string(),
                r#type: "server_error".to_string(),
                param: None,
                code: Some("engine_fault".to_string()),
            },
        };
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
    }

    if !is_streaming {
        // Non-streaming mode: collect all tokens, return single JSON response
        let _cancel_guard = cancel.drop_guard();
        let mut full_content = String::new();
        let mut token_count: usize = 0;

        while let Some(res) = response_rx.recv().await {
            match res {
                Ok(InferenceResponse::Admitted) => {}
                Ok(InferenceResponse::Token(token)) => {
                    full_content.push_str(&token);
                    token_count += 1;
                }
                Err(e) => {
                    error!("[Session {}] Engine Error: {:?}", session_id, e);
                    let err = ApiErrorResponse {
                        error: ApiErrorDetail {
                            message: format!("{}", e),
                            r#type: "server_error".to_string(),
                            param: None,
                            code: Some("engine_error".to_string()),
                        },
                    };
                    return (StatusCode::BAD_REQUEST, Json(err)).into_response();
                }
            }
        }

        let response_json = serde_json::json!({
            "id": format!("chatcmpl-{}", session_id),
            "object": "chat.completion",
            "created": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            "model": model_id,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": full_content,
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": estimated_prompt_tokens,
                "completion_tokens": token_count,
                "total_tokens": estimated_prompt_tokens + token_count
            }
        });

        return (StatusCode::OK, Json(response_json)).into_response();
    }

    // Streaming mode (SSE)
    let stream_res = stream! {
        let _cancel_guard = cancel.drop_guard();

        let mut index = 0;

        while let Some(res) = response_rx.recv().await {
            match res {
                Ok(InferenceResponse::Admitted) => {
                    // Do nothing for Admitted in SSE stream
                }
                Ok(InferenceResponse::Token(token)) => {
                    let chunk = ChatCompletionChunk {
                        id: format!("chatcmpl-{}", session_id),
                        object: "chat.completion.chunk".to_string(),
                        created: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                        model: model_id.clone(),
                        choices: vec![
                            ChatCompletionChunkChoice {
                                index,
                                delta: ChatCompletionDelta {
                                    role: if index == 0 { Some("assistant".to_string()) } else { None },
                                    content: Some(token),
                                },
                                finish_reason: None,
                            }
                        ]
                    };
                    yield Ok::<Event, std::convert::Infallible>(Event::default().data(serde_json::to_string(&chunk).unwrap()));
                    index += 1;
                }
                Err(e) => {
                    error!("[Session {}] API Engine Error: {:?}", session_id, e);
                    yield Ok::<Event, std::convert::Infallible>(Event::default().data("[DONE]"));
                }
            }
        }

        let final_chunk = ChatCompletionChunk {
            id: format!("chatcmpl-{}", session_id),
            object: "chat.completion.chunk".to_string(),
            created: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            model: model_id.clone(),
            choices: vec![
                ChatCompletionChunkChoice {
                    index,
                    delta: ChatCompletionDelta {
                        role: None,
                        content: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }
            ]
        };
        yield Ok::<Event, std::convert::Infallible>(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));
        yield Ok::<Event, std::convert::Infallible>(Event::default().data("[DONE]"));
    };

    (StatusCode::OK, Sse::new(stream_res)).into_response()
}
