//! Real localhost HTTP exchanges for the provider's rate-limit control flow.
use super::DeepSeek;
use crate::model::{ModelEvent, Provider, Request, RequestFailure, failure_usage};
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::post,
};
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
    if !status.is_success() {
        return (
            status,
            [(header::RETRY_AFTER, state.retry_after)],
            Json(
                json!({"error":{"message":"fixture rejection", "type":"invalid_request_error",
                "code":"invalid_request_error", "param":null}}),
            ),
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
    let mut model = DeepSeek::new(
        format!("http://{}", listener.local_addr().unwrap()),
        "deepseek-flash".into(),
        "localhost-fixture-key".into(),
    )
    .unwrap();
    // Exercise authoritative DeepSeek error handling against a local HTTP fixture.
    // Production derives this private policy only from the immutable HTTPS base URL.
    model.official = true;
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
    assert!(matches!(
        error.downcast_ref::<RequestFailure>(),
        Some(RequestFailure::Rejected { status: 429, .. })
    ));
    assert_eq!(failure_usage(&error).unwrap().tokens(), 0);
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
    assert_eq!(failure_usage(&error).unwrap().tokens(), 0);
    assert_eq!(state.requests.lock().unwrap().len(), 1);
    server.abort();
}

#[tokio::test]
async fn documented_rejections_release_the_reservation() {
    use crate::runtime::{
        meter::Metered,
        money::{Money, Prices, RMB},
    };
    let prices = Prices {
        input: 2000,
        cached: 40,
        output: 8000,
        search: 0,
        fetch: 0,
    };
    for status in [400, 401, 402, 422, 429] {
        let (model, state, server) =
            server(vec![StatusCode::from_u16(status).unwrap(); 3], "0").await;
        let root = tempfile::tempdir().unwrap();
        let money = Arc::new(
            Money::open(&root.path().join("money.db"), "fixture", 3 * RMB, &prices).unwrap(),
        );
        let provider = Metered::new(Arc::new(model), money.clone(), prices.clone());
        let (sink, _) = mpsc::unbounded_channel();
        let error = provider
            .respond(request(), CancellationToken::new(), sink)
            .await
            .unwrap_err();
        server.abort();
        assert!(
            matches!(
                error.downcast_ref::<RequestFailure>(),
                Some(RequestFailure::Rejected { .. })
            ),
            "{status}: {error}"
        );
        assert_eq!(
            state.requests.lock().unwrap().len(),
            if status == 429 { 3 } else { 1 }
        );
        assert_eq!(money.balance().unwrap().used, 0);
        assert_eq!(money.balance().unwrap().uncertain, 0);
        assert_eq!(money.balance().unwrap().reserved, 0);
    }
}

#[tokio::test]
async fn undocumented_and_server_errors_keep_usage_unknown() {
    for status in [403, 404, 408, 500, 503] {
        let (model, state, server) = server(vec![StatusCode::from_u16(status).unwrap()], "0").await;
        let (sink, _) = mpsc::unbounded_channel();
        let error = model
            .respond(request(), CancellationToken::new(), sink)
            .await
            .unwrap_err();
        server.abort();
        assert!(
            failure_usage(&error).is_none(),
            "{status} must not invent free usage"
        );
        assert_eq!(state.requests.lock().unwrap().len(), 1);
    }
}

#[test]
fn only_the_official_https_endpoint_uses_deepseek_rejection_semantics() {
    for (url, official) in [
        ("https://api.deepseek.com", true),
        ("https://api.deepseek.com/v1/", true),
        ("https://api.deepseek.com.proxy.test", false),
        ("https://proxy.test", false),
        ("https://api.deepseek.com:8443", false),
        ("https://api.deepseek.com/unknown", false),
        ("http://127.0.0.1:8123", false),
    ] {
        let provider = DeepSeek::new(url.into(), "fixture".into(), "fixture-key".into()).unwrap();
        assert_eq!(provider.official, official, "{url}");
    }
}

#[test]
fn malformed_or_usage_bearing_error_bodies_do_not_prove_zero_usage() {
    for body in [
        "",
        "<html>upstream failure</html>",
        "{\"error\":",
        "{}",
        r#"{"error":{"message":""}}"#,
        r#"{"error":{"message":"failed"},"usage":{"input_tokens":10,"output_tokens":2}}"#,
        r#"{"error":{"message":"failed"},"response":{"status":"incomplete"}}"#,
        r#"{"error":{"message":"failed"},"output":[]}"#,
    ] {
        assert!(!super::documented_rejection(400, body.as_bytes()), "{body}");
    }
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
