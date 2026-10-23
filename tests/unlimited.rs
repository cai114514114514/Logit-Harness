#[allow(dead_code)]
#[path = "fixtures/chat.rs"]
mod chat;

use loh::{
    commands::{self, Outcome, Service},
    config::{Config, Paths},
    domain::{Budget, Usage},
    runtime::state::{Control, Session},
};

#[test]
fn defaults_and_config_use_explicit_unlimited_values() {
    let config = Config::default();
    assert_eq!(config.tokens, u64::MAX);
    assert_eq!(config.seconds, u64::MAX);
    let loaded: Config = toml::from_str("tokens = 'unlimited'\nseconds = 'unlimited'\n").unwrap();
    loaded.validate().unwrap();
    let encoded = toml::to_string(&loaded).unwrap();
    assert!(encoded.contains("tokens = \"unlimited\""));
    assert!(encoded.contains("seconds = \"unlimited\""));
    assert!(toml::from_str::<Config>("tokens = 'infinity'").is_err());
}

#[test]
fn unlimited_still_accounts_usage_uncertainty_and_concurrent_reservations() {
    let mut budget = Budget::new(u64::MAX);
    budget.used = u64::MAX;
    let a = budget
        .reserve(100)
        .expect("unlimited must not be a merely large finite allowance");
    let b = budget.reserve(200).unwrap();
    budget
        .settle(
            &a,
            Some(&Usage {
                input: 12,
                output: 3,
                cached: 6,
            }),
        )
        .unwrap();
    budget.settle(&b, None).unwrap();
    assert_eq!(budget.used, u64::MAX);
    assert_eq!(budget.reported.tokens(), 15);
    assert_eq!(budget.uncertain, 200);
    assert!(budget.reservations.is_empty());
    assert!(Budget::new(0).reserve(1).is_err());
}

#[test]
fn budget_command_removes_both_caps_and_config_preserves_other_tables() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("home"));
    paths.prepare().unwrap();
    let service = Service::new(&paths, root.path(), None);
    assert!(matches!(
        service
            .execute(commands::parse("/budget unlimited").unwrap())
            .unwrap(),
        Outcome::Control(Control::Budget {
            tokens: Some(u64::MAX),
            seconds: Some(u64::MAX)
        })
    ));
    assert!(matches!(
        service
            .execute(commands::parse("/budget 100 unlimited").unwrap())
            .unwrap(),
        Outcome::Control(Control::Budget {
            tokens: Some(100),
            seconds: Some(u64::MAX)
        })
    ));
    let config = Config {
        campaign: Some(loh::runtime::state::Campaign {
            name: "fixed".into(),
            limit: 50_000_000_000,
            prices: loh::runtime::money::Prices {
                input: 1,
                cached: 1,
                output: 1,
                search: 1,
                fetch: 1,
            },
        }),
        ..Config::default()
    };
    std::fs::write(
        paths.home.join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let before = serde_json::to_value(&config.campaign).unwrap();
    for key in ["tokens", "seconds"] {
        loh::config::defaults::save(&paths, key, "unlimited").unwrap();
    }
    assert_eq!(
        serde_json::to_value(Config::load(&paths.home).unwrap().campaign).unwrap(),
        before
    );
}

#[tokio::test]
async fn unlimited_time_does_not_exhaust_and_is_preserved_on_restore() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Session::conversation(root.path().into(), "hello", u64::MAX, u64::MAX);
    s.elapsed = u64::MAX;
    let mut live = chat::Live::start(s, chat::Engineer::new(false));
    live.waiting().await;
    let saved = live.saved();
    assert_eq!(saved.seconds, u64::MAX);
    assert_eq!(saved.budget.limit, u64::MAX);
    assert_eq!(saved.status, loh::domain::Status::Waiting);
    live.stop().await;
}

#[tokio::test]
async fn unlimited_tokens_do_not_bypass_the_money_gate() {
    use loh::{
        model::{Provider, Request},
        runtime::{
            meter::Metered,
            money::{BudgetReached, Money, Prices},
        },
    };
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let prices = Prices {
        input: 1,
        cached: 1,
        output: 1,
        search: 1,
        fetch: 1,
    };
    let money = Arc::new(Money::open(&root.path().join("money.db"), "fixed", 1, &prices).unwrap());
    let provider = chat::Engineer::new(false);
    let metered = Metered::new(provider.clone(), money.clone(), prices);
    let mut tokens = Budget::new(u64::MAX);
    let reservation = tokens.reserve(100_000_000).unwrap();
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let error = metered
        .respond(
            Request {
                instructions: "ROLE: conversation\n".into(),
                input: vec![],
                tools: vec![],
                max_output: 100,
            },
            tokio_util::sync::CancellationToken::new(),
            events,
        )
        .await
        .unwrap_err();
    assert!(error.is::<BudgetReached>());
    assert!(provider.requests.lock().unwrap().is_empty());
    let balance = money.balance().unwrap();
    assert_eq!(balance.limit, 1);
    assert_eq!(
        (balance.used, balance.uncertain, balance.reserved),
        (0, 0, 0)
    );
    tokens
        .settle(&reservation, Some(&Usage::default()))
        .unwrap();
}

#[test]
fn local_status_spells_out_unlimited_without_changing_stored_numbers() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("home"));
    paths.prepare().unwrap();
    let session = Session::conversation(root.path().into(), "hello", u64::MAX, u64::MAX);
    let store = loh::store::Store::open(&paths.database()).unwrap();
    store
        .commit(
            &session.id,
            &session,
            loh::domain::EventData::Created {
                objective: "hello".into(),
                workspace: root.path().display().to_string(),
            },
        )
        .unwrap();
    let service = Service::new(&paths, root.path(), Some(&session.id));
    let Outcome::Local { text, .. } = service
        .execute(commands::parse("/status").unwrap())
        .unwrap()
    else {
        panic!("expected local status")
    };
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["budget"]["limit"], "unlimited");
    assert_eq!(value["seconds"], "unlimited");
    let saved: Session = store.load(&session.id).unwrap();
    assert_eq!((saved.budget.limit, saved.seconds), (u64::MAX, u64::MAX));
}
