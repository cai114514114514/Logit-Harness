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
