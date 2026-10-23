//! Exercise provider failures through both real runtime ledgers without paid requests.
use loh::{
    domain::{EventData, Status},
    model::{DeepSeek, ModelEvent, Provider, Request, RequestFailure, Response},
    runtime::{
        meter::Metered,
        money::{Balance, Money, Prices, RMB},
        runner::{Runner, Settings},
        state::Session,
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

async fn run(provider: Arc<dyn Provider>) -> (Session, Balance, Vec<EventData>) {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(workspace.path().into(), "Inspect the project", 500_000, 60);
    let prices = Prices {
        input: 2000,
        cached: 40,
        output: 8000,
        search: 5_000_000,
        fetch: 5_000_000,
    };
    let money = Arc::new(
        Money::open(&state.path().join("money.db"), "fixture", 50 * RMB, &prices).unwrap(),
    );
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let (runner, _control, _) = Runner::new(
        session,
        store.clone(),
        Arc::new(Metered::new(provider, money.clone(), prices)),
        tools,
        Settings {
            linger: false,
            max_output: 128,
            ..Settings::default()
        },
    );
    let session = tokio::time::timeout(std::time::Duration::from_secs(5), runner.run())
        .await
        .unwrap()
        .unwrap();
    let events = store.events(&session.id, 0).unwrap();
    (
        session,
        money.balance().unwrap(),
        events.into_iter().map(|event| event.data).collect(),
    )
}

async fn server(status: u16) -> (DeepSeek, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let app = axum::Router::new().route(
        "/responses",
        axum::routing::post(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [("retry-after", "0")],
                    axum::Json(serde_json::json!({"error":{
                        "message":"fixture rejection", "type":"invalid_request_error",
                        "code":"invalid_request_error", "param":null
                    }})),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = DeepSeek::new(
        format!("http://{}", listener.local_addr().unwrap()),
        "deepseek-flash".into(),
        "fixture-key".into(),
    )
    .unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (provider, requests, task)
}

#[tokio::test]
async fn connection_refusal_releases_money_and_tokens() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let provider = DeepSeek::new(url, "deepseek-flash".into(), "fixture-key".into()).unwrap();
    let (session, balance, _) = run(Arc::new(provider)).await;
    assert_eq!(session.status, Status::Failed);
    assert_eq!(balance.used, 0);
    assert_eq!(
        balance.uncertain, 0,
        "no connection means no submitted inference"
    );
    assert_eq!(balance.reserved, 0);
    assert_eq!(session.budget.used, 0);
    assert_eq!(session.budget.uncertain, 0);
    assert!(session.budget.reservations.is_empty());
}

#[tokio::test]
async fn authentication_failure_stops_without_repeating_an_unchanged_request() {
    let (provider, requests, server) = server(401).await;
    let (session, balance, events) = run(Arc::new(provider)).await;
    server.abort();
    assert_eq!(session.status, Status::Failed);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(balance.used, 0);
    // A proxy's status is not proof of DeepSeek's billing outcome.
    assert_eq!(balance.uncertain, 2_098_176_000);
    assert!(session.budget.uncertain > 0);
    assert!(events.iter().any(|event| matches!(event,
        EventData::Diagnostic { message } if message.contains("API key")
    )));
    assert!(!session.actors["main"].context.items.iter().any(|item| {
        item.to_string()
            .contains("Retry with a smaller, complete response")
    }));
}

#[tokio::test]
async fn unclassified_rate_limits_require_a_reservation_for_every_attempt() {
    let (provider, requests, server) = server(429).await;
    let (session, balance, _) = run(Arc::new(provider)).await;
    server.abort();
    assert_eq!(session.status, Status::Failed);
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    assert_eq!(balance.uncertain, 6_294_528_000);
    assert_eq!(balance.reserved, 0);
    assert!(session.budget.uncertain > 0);
    assert!(session.budget.reservations.is_empty());
}

struct Rejection(u16);

#[async_trait::async_trait]
impl Provider for Rejection {
    async fn respond(
        &self,
        _: Request,
        _: tokio_util::sync::CancellationToken,
        _: tokio::sync::mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        Err(RequestFailure::Rejected {
            status: self.0,
            detail: "documented provider admission rejection".into(),
        }
        .into())
    }
}

#[tokio::test]
async fn documented_rejection_releases_both_runtime_ledgers() {
    // DeepSeek's transport tests establish when this typed result is justified;
    // this fixture exercises the consumer contract through the real runner/meter.
    for status in [400, 401, 402, 422, 429] {
        let (session, balance, _) = run(Arc::new(Rejection(status))).await;
        assert_eq!(session.status, Status::Failed, "{status}");
        assert_eq!(session.actors["main"].failures, 1, "{status}");
        assert_eq!(balance.used, 0);
        assert_eq!(balance.uncertain, 0);
        assert_eq!(balance.reserved, 0);
        assert_eq!(session.budget.used, 0);
        assert_eq!(session.budget.uncertain, 0);
        assert_eq!(session.budget.remaining(), 500_000);
    }
}

#[tokio::test]
async fn submitted_server_errors_keep_both_runtime_reservations_uncertain() {
    let (provider, requests, server) = server(503).await;
    let (session, balance, _) = run(Arc::new(provider)).await;
    server.abort();
    assert_eq!(session.status, Status::Failed);
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    assert_eq!(balance.used, 0);
    assert_eq!(balance.uncertain, 6_294_528_000);
    assert_eq!(balance.reserved, 0);
    assert_eq!(session.budget.used, 0);
    assert!(session.budget.uncertain > 0);
    assert!(session.budget.reservations.is_empty());
}

#[tokio::test]
async fn a_redirect_cannot_hide_a_submission_before_connection_refusal() {
    let unavailable = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let location = format!("http://{}/responses", unavailable.local_addr().unwrap());
    drop(unavailable);
    let app = axum::Router::new().route(
        "/responses",
        axum::routing::post(move || {
            let location = location.clone();
            async move {
                (
                    axum::http::StatusCode::TEMPORARY_REDIRECT,
                    [("location", location)],
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = DeepSeek::new(
        format!("http://{}", listener.local_addr().unwrap()),
        "fixture".into(),
        "fixture-key".into(),
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (session, balance, events) = run(Arc::new(provider)).await;
    server.abort();
    assert_eq!(session.status, Status::Failed);
    assert_eq!(balance.uncertain, 2_098_176_000);
    assert!(session.budget.uncertain > 0);
    assert!(events.iter().any(|event| matches!(event,
        EventData::Diagnostic { message } if message.contains("HTTP 307")
    )));
}
