use loh::tools::web::{WebClient, WebUsage};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn web_api_returns_real_response_and_enforces_shared_limit() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0; 8192];
        let n = socket.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        let body =
            json!({"results":[{"title":"Rust","url":"https://www.rust-lang.org/"}]}).to_string();
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        request
    });
    let client = WebClient::new(base, "fixture".into(), 1, WebUsage::default()).unwrap();
    let response = client
        .search("Rust", 5, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(response["results"][0]["title"], "Rust");
    assert_eq!(client.usage().searches, 1);
    assert!(
        client
            .search("second", 5, CancellationToken::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    assert!(server.await.unwrap().starts_with("POST /v1/tools/search "));
}

#[tokio::test]
async fn invalid_query_does_not_consume_network_quota() {
    let client = WebClient::new(
        "http://127.0.0.1:1".into(),
        "fixture".into(),
        2,
        WebUsage::default(),
    )
    .unwrap();
    assert!(
        client
            .search(&"字".repeat(129), 5, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(client.usage().searches, 0);
    assert!(
        client
            .fetch("file:///etc/passwd", CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(client.usage().fetches, 0);
}

#[tokio::test]
async fn attempted_web_calls_keep_their_quota_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("web.json");
    let client = WebClient::new(
        "http://127.0.0.1:1".into(),
        "fixture".into(),
        1,
        WebUsage::default(),
    )
    .unwrap()
    .with_usage_file(path.clone())
    .unwrap();
    // An unavailable endpoint still consumes one attempted call; restarting cannot erase it.
    assert!(
        client
            .search("Rust", 1, CancellationToken::new())
            .await
            .is_err()
    );
    drop(client);
    let resumed = WebClient::new(
        "http://127.0.0.1:1".into(),
        "fixture".into(),
        1,
        WebUsage::default(),
    )
    .unwrap()
    .with_usage_file(path)
    .unwrap();
    assert_eq!(resumed.usage().searches, 1);
    assert!(
        resumed
            .search("Retry", 1, CancellationToken::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}

#[tokio::test]
async fn non_public_fetch_literals_are_rejected_before_quota_and_money() {
    use loh::runtime::money::{Money, Prices, RMB};
    use std::sync::Arc;

    let prices = Prices {
        input: 0,
        cached: 0,
        output: 0,
        search: 5_000_000,
        fetch: 5_000_000,
    };
    let directory = tempfile::tempdir().unwrap();
    let money =
        Arc::new(Money::open(&directory.path().join("money.db"), "fixture", RMB, &prices).unwrap());
    let mut failures = Vec::new();
    for url in [
        "http://127.0.0.1/",
        "http://10.0.0.1/",
        "http://169.254.169.254/",
        "http://localhost/",
        "http://[::1]/",
        "http://[::]/",
        "http://[fe80::1]/",
        "http://[fc00::1]/",
        "http://[ff02::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[::ffff:169.254.169.254]/",
        "http://[::ffff:10.0.0.1]/",
    ] {
        // The provider endpoint is an unavailable local fixture, never DeepSeek.
        let client = WebClient::new(
            "http://127.0.0.1:1".into(),
            "fixture".into(),
            1,
            WebUsage::default(),
        )
        .unwrap()
        .with_money(money.clone(), prices.clone());
        let error = client
            .fetch(url, CancellationToken::new())
            .await
            .unwrap_err();
        if !error.to_string().contains("non-public") || client.usage().fetches != 0 {
            failures.push(format!("{url}: {error}; quota={}", client.usage().fetches));
        }
    }
    let balance = money.balance().unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        (balance.used, balance.reserved, balance.uncertain),
        (0, 0, 0)
    );
}

#[tokio::test]
async fn public_ipv6_fetch_literals_still_reach_the_provider() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 8192];
            let size = socket.read(&mut buffer).await.unwrap();
            requests.push(String::from_utf8_lossy(&buffer[..size]).into_owned());
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
        }
        requests
    });
    let client = WebClient::new(base, "fixture".into(), 2, WebUsage::default()).unwrap();
    for url in [
        "https://[2606:4700:4700::1111]/",
        "https://[::ffff:8.8.8.8]/",
    ] {
        client.fetch(url, CancellationToken::new()).await.unwrap();
    }
    assert_eq!(client.usage().fetches, 2);
    for request in server.await.unwrap() {
        assert!(request.starts_with("POST /v1/tools/fetch "));
    }
}
