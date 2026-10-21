use weft::domain::Usage;
use weft::runtime::money::{Money, Prices, RMB};

fn prices() -> Prices {
    Prices {
        input: 2000,
        cached: 40,
        output: 8000,
        search: 5_000_000,
        fetch: 5_000_000,
    }
}

#[test]
fn money_counts_cached_tokens_at_their_own_verified_rate() {
    let usage = Usage {
        input: 1_000_000,
        cached: 500_000,
        output: 100_000,
    };
    assert_eq!(prices().cost(&usage), 1_820_000_000);
    assert_eq!(prices().reserve(1_000_000, 100_000), 2_800_000_000);
}

#[test]
fn concurrent_runners_share_one_persistent_campaign_cap() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("money.db");
    let first = Money::open(&path, "evaluation", 50 * RMB, &prices()).unwrap();
    let second = Money::open(&path, "evaluation", 50 * RMB, &prices()).unwrap();
    let reservation = first.reserve(40 * RMB).unwrap();
    assert!(second.reserve(11 * RMB).is_err());
    first.settle(&reservation, Some(5 * RMB)).unwrap();
    assert_eq!(second.balance().unwrap().used, 5 * RMB);
    let unresolved = second.reserve(10 * RMB).unwrap();
    second.settle(&unresolved, None).unwrap();
    drop(first);
    drop(second);
    let reopened = Money::open(&path, "evaluation", 50 * RMB, &prices()).unwrap();
    let balance = reopened.balance().unwrap();
    assert_eq!(balance.uncertain, 10 * RMB);
    assert_eq!(balance.used, 5 * RMB);
    assert!(reopened.reserve(36 * RMB).is_err());
    assert!(reopened.settle(&reservation, Some(0)).is_err());
}

struct UsageProvider;
#[async_trait::async_trait]
impl weft::model::Provider for UsageProvider {
    async fn respond(
        &self,
        _request: weft::model::Request,
        _cancel: tokio_util::sync::CancellationToken,
        _events: tokio::sync::mpsc::UnboundedSender<weft::model::ModelEvent>,
    ) -> anyhow::Result<weft::model::Response> {
        Ok(weft::model::Response {
            output: vec![],
            usage: Usage {
                input: 100,
                cached: 50,
                output: 20,
            },
            model: "fixture".into(),
        })
    }
}

#[tokio::test]
async fn all_provider_roles_are_charged_through_the_same_wrapper() {
    use weft::model::Provider;
    let temp = tempfile::tempdir().unwrap();
    let money = std::sync::Arc::new(
        Money::open(
            &temp.path().join("money.db"),
            "campaign",
            10 * RMB,
            &prices(),
        )
        .unwrap(),
    );
    let provider = weft::runtime::meter::Metered::new(
        std::sync::Arc::new(UsageProvider),
        money.clone(),
        prices(),
    );
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    provider
        .respond(
            weft::model::Request {
                instructions: "verify".into(),
                input: vec![],
                tools: vec![],
                max_output: 100,
            },
            tokio_util::sync::CancellationToken::new(),
            events,
        )
        .await
        .unwrap();
    assert_eq!(money.balance().unwrap().used, 262000);
    assert_eq!(money.balance().unwrap().reserved, 0);
}

#[tokio::test]
async fn exhausted_money_prevents_contacting_the_provider() {
    use weft::model::Provider;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let inner = weft::model::DeepSeek::new(
        format!("http://{}", listener.local_addr().unwrap()),
        "deepseek-flash".into(),
        "fixture".into(),
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let money = std::sync::Arc::new(
        Money::open(&temp.path().join("money.db"), "campaign", 1, &prices()).unwrap(),
    );
    let provider = weft::runtime::meter::Metered::new(std::sync::Arc::new(inner), money, prices());
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        provider.respond(
            weft::model::Request {
                instructions: "work".into(),
                input: vec![],
                tools: vec![],
                max_output: 100,
            },
            tokio_util::sync::CancellationToken::new(),
            events,
        ),
    )
    .await;
    assert!(
        result.is_ok(),
        "budget refusal should happen before a network wait"
    );
    assert!(
        result
            .unwrap()
            .unwrap_err()
            .is::<weft::runtime::meter::BudgetReached>()
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
}
struct SlowProvider;
#[async_trait::async_trait]
impl weft::model::Provider for SlowProvider {
    async fn respond(
        &self,
        request: weft::model::Request,
        cancel: tokio_util::sync::CancellationToken,
        events: tokio::sync::mpsc::UnboundedSender<weft::model::ModelEvent>,
    ) -> anyhow::Result<weft::model::Response> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        UsageProvider.respond(request, cancel, events).await
    }
}

#[tokio::test]
async fn temporary_reservations_wait_instead_of_exhausting_shared_campaign() {
    use weft::model::Provider;
    let temp = tempfile::tempdir().unwrap();
    let money = std::sync::Arc::new(
        Money::open(
            &temp.path().join("money.db"),
            "campaign",
            3 * RMB,
            &prices(),
        )
        .unwrap(),
    );
    let provider = weft::runtime::meter::Metered::new(
        std::sync::Arc::new(SlowProvider),
        money.clone(),
        prices(),
    );
    let request = || weft::model::Request {
        instructions: "work".into(),
        input: vec![],
        tools: vec![],
        max_output: 100,
    };
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let (first, second) = tokio::join!(
        provider.respond(
            request(),
            tokio_util::sync::CancellationToken::new(),
            events.clone()
        ),
        provider.respond(
            request(),
            tokio_util::sync::CancellationToken::new(),
            events
        )
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(money.balance().unwrap().used, 524000);
}

#[tokio::test]
async fn dropping_a_paid_request_keeps_its_unknown_cost_accounted() {
    use weft::model::Provider;
    let temp = tempfile::tempdir().unwrap();
    let money = std::sync::Arc::new(
        Money::open(
            &temp.path().join("money.db"),
            "campaign",
            3 * RMB,
            &prices(),
        )
        .unwrap(),
    );
    let provider = weft::runtime::meter::Metered::new(
        std::sync::Arc::new(SlowProvider),
        money.clone(),
        prices(),
    );
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let request = provider.respond(
        weft::model::Request {
            instructions: "work".into(),
            input: vec![],
            tools: vec![],
            max_output: 100,
        },
        tokio_util::sync::CancellationToken::new(),
        events,
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), request)
            .await
            .is_err()
    );
    let balance = money.balance().unwrap();
    assert_eq!(balance.reserved, 0);
    assert!(balance.uncertain > 2 * RMB);
}
#[test]
fn dead_request_owners_become_uncertain_without_resetting_the_campaign() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("money.db");
    let money = Money::open(&path, "campaign", 50 * RMB, &prices()).unwrap();
    let id = money.reserve(2 * RMB).unwrap();
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let identity = weft::tools::identity::Identity::capture(child.id())
        .unwrap()
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE charge_owners SET identity=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&identity).unwrap(), id],
        )
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    let reopened = Money::open(&path, "campaign", 50 * RMB, &prices()).unwrap();
    assert_eq!(reopened.balance().unwrap().uncertain, 2 * RMB);
    assert_eq!(reopened.balance().unwrap().reserved, 0);
    assert!(Money::open(&path, "campaign", 51 * RMB, &prices()).is_err());
}
