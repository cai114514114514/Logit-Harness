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
