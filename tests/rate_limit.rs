//! Real localhost HTTP exchanges for the provider's rate-limit control flow.
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use loh::model::{DeepSeek, ModelEvent, Provider, Request};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Fixture {
    responses: Vec<StatusCode>,
    retry_after: &'static str,
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn respond(
    State(state): State<Fixture>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let index = {
        let mut requests = state.requests.lock().unwrap();
        let index = requests.len();
        requests.push(body);
        index
    };
    let status = state
        .responses
        .get(index)
        .copied()
        .unwrap_or(StatusCode::TOO_MANY_REQUESTS);
    if status == StatusCode::TOO_MANY_REQUESTS {
        return (
            status,
            [(header::RETRY_AFTER, state.retry_after)],
            "rate limited",
        )
            .into_response();
    }
    let event = json!({"type":"response.completed","response":{
        "status":"completed","model":"fixture","usage":{"input_tokens":4,"output_tokens":2},
        "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ready"}]}]
    }});
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/event-stream")],
        format!("data: {event}\n\n"),
    )
        .into_response()
}

async fn server(
    responses: Vec<StatusCode>,
    retry_after: &'static str,
) -> (DeepSeek, Fixture, tokio::task::JoinHandle<()>) {
    let state = Fixture {
        responses,
        retry_after,
        requests: Default::default(),
    };
    let router = Router::new()
        .route("/responses", post(respond))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model = DeepSeek::new(
        format!("http://{}", listener.local_addr().unwrap()),
        "deepseek-flash".into(),
        "localhost-fixture-key".into(),
    )
    .unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (model, state, task)
}

fn request() -> Request {
    Request {
        instructions: "Keep the complete request when retrying.".into(),
        input: vec![json!({"role":"user","content":"preserve this input"})],
        tools: vec![],
        max_output: 128,
    }
}

#[tokio::test]
async fn retry_preserves_request_and_reports_attempts() {
    let (model, state, server) = server(
        vec![
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::OK,
        ],
        "0",
    )
    .await;
    let (sink, mut events) = mpsc::unbounded_channel();
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        model.respond(request(), CancellationToken::new(), sink),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.usage.input, 4);
    let requests = state.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request == &requests[0]));
    let mut retries = vec![];
    while let Ok(event) = events.try_recv() {
        if let ModelEvent::Retry(attempt) = event {
            retries.push(attempt);
        }
    }
    assert_eq!(retries, [1, 2]);
    server.abort();
}

#[tokio::test]
async fn persistent_limit_stops_after_three_attempts() {
    let (model, state, server) = server(vec![], "0").await;
    let (sink, _) = mpsc::unbounded_channel();
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        model.respond(request(), CancellationToken::new(), sink),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.to_string().contains("HTTP 429"));
    assert_eq!(state.requests.lock().unwrap().len(), 3);
    server.abort();
}

#[tokio::test]
async fn cancellation_interrupts_retry_backoff() {
    let (model, state, server) = server(vec![], "30").await;
    let cancel = CancellationToken::new();
    let child = cancel.clone();
    let (sink, mut events) = mpsc::unbounded_channel();
    let pending = tokio::spawn(async move { model.respond(request(), child, sink).await });
    let retry = tokio::time::timeout(Duration::from_secs(3), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(retry, ModelEvent::Retry(1)));
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_secs(1), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("cancel"));
    assert_eq!(state.requests.lock().unwrap().len(), 1);
    server.abort();
}

#[tokio::test]
async fn malformed_retry_hint_uses_backoff() {
    let (model, state, server) = server(
        vec![StatusCode::TOO_MANY_REQUESTS, StatusCode::OK],
        "invalid",
    )
    .await;
    let (sink, _) = mpsc::unbounded_channel();
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(4),
        model.respond(request(), CancellationToken::new(), sink),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "a malformed delay must not become a hot retry loop"
    );
    assert_eq!(state.requests.lock().unwrap().len(), 2);
    server.abort();
}
