use loh::tools::browser::BrowserClient;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn browser_rejects_non_public_literals_before_launch() {
    let directory = tempfile::tempdir().unwrap();
    let artifacts = directory.path().join("not-created");
    let browser = BrowserClient::new(directory.path().join("no-chrome"), artifacts.clone());
    for url in [
        "http://192.168.0.1/",
        "http://10.0.0.1/",
        "http://169.254.169.254/",
        "http://[fe80::1]/",
        "http://[fc00::1]/",
        "http://[::ffff:169.254.169.254]/",
        "http://[::ffff:10.0.0.1]/",
    ] {
        let error = browser
            .open(url, CancellationToken::new())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("non-public"), "{url}: {error}");
    }
    assert!(
        !artifacts.exists(),
        "rejected targets must not launch Chrome"
    );
}

/// 0.0.0.0 reaches this owned listener on macOS but is a forbidden unspecified
/// destination. That gives a real request-level proof without contacting a router
/// or cloud metadata service and without changing the host's network configuration.
#[tokio::test]
async fn browser_blocks_non_public_redirects_and_subresources() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let private_requests = Arc::new(Mutex::new(Vec::new()));
    let observed = private_requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let observed = observed.clone();
            tokio::spawn(async move {
                let mut bytes = [0; 8192];
                let count = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                if path.starts_with("/private") {
                    observed.lock().unwrap().push(path.to_string());
                }
                let (status, headers, body) = match path {
                    "/redirect" => (
                        "302 Found",
                        format!("Location: http://0.0.0.0:{port}/private-redirect\r\n"),
                        String::new(),
                    ),
                    "/resources" => (
                        "200 OK",
                        "Content-Type: text/html\r\n".into(),
                        format!(
                            r#"<!doctype html><html><body>
                            <p id="result">waiting</p>
                            <script src="http://0.0.0.0:{port}/private-script"></script>
                            <img src="http://0.0.0.0:{port}/private-image">
                            <iframe src="http://0.0.0.0:{port}/private-frame"></iframe>
                            <iframe src="data:text/html,inline-data-frame"></iframe>
                            <script>
                              const frame = document.createElement('iframe');
                              frame.src = URL.createObjectURL(new Blob(['inline-blob-frame'], {{type:'text/html'}}));
                              document.body.appendChild(frame);
                              fetch('http://0.0.0.0:{port}/private-fetch')
                                .then(response => response.text())
                                .then(text => document.querySelector('#result').textContent = text)
                                .catch(() => document.querySelector('#result').textContent = 'blocked');
                            </script></body></html>"#
                        ),
                    ),
                    _ => (
                        "200 OK",
                        "Content-Type: text/plain\r\nAccess-Control-Allow-Origin: *\r\n".into(),
                        "PRIVATE_FIXTURE_SECRET".into(),
                    ),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let browser = BrowserClient::new(
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
        directory.path().into(),
    );
    let cancel = CancellationToken::new();
    let result = async {
        let page = browser
            .open(
                &format!("http://127.0.0.1:{port}/resources"),
                cancel.clone(),
            )
            .await?;
        let mut snapshot = browser.inspect(&page, cancel.clone()).await?;
        for _ in 0..20 {
            if !snapshot.dom.contains("<p id=\"result\">waiting</p>") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            snapshot = browser.inspect(&page, cancel.clone()).await?;
        }
        let redirect = browser
            .open(&format!("http://127.0.0.1:{port}/redirect"), cancel.clone())
            .await;
        let redirect_dom = match redirect {
            Ok(page) => browser
                .inspect(&page, cancel)
                .await
                .map(|value| value.dom)
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        Ok::<_, anyhow::Error>((snapshot, redirect_dom))
    }
    .await;
    browser.close().await.unwrap();
    let identity: loh::tools::identity::Identity =
        serde_json::from_slice(&std::fs::read(directory.path().join("process.json")).unwrap())
            .unwrap();
    assert_ne!(
        loh::tools::identity::Identity::capture(identity.pid)
            .unwrap()
            .as_ref(),
        Some(&identity),
        "owned fixture Chrome must have exited after close"
    );
    println!("owned fixture Chrome {} exited after close", identity.pid);
    server.abort();
    let (snapshot, redirect_dom) = result.unwrap();
    let requests = private_requests.lock().unwrap();
    assert!(
        requests.is_empty(),
        "forbidden requests reached owned fixture: {requests:?}"
    );
    println!("forbidden fixture requests after admission guard: {requests:?}");
    assert!(snapshot.dom.contains("<p id=\"result\">blocked</p>"));
    assert!(!snapshot.dom.contains("PRIVATE_FIXTURE_SECRET"));
    assert!(!redirect_dom.contains("PRIVATE_FIXTURE_SECRET"));
}
