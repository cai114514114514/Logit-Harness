use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use weft::tools::browser::BrowserClient;

#[tokio::test]
async fn owned_chrome_performs_unicode_input_click_and_returns_visual_evidence() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buffer = [0; 8192];
                let _ = socket.read(&mut buffer).await;
                let html = r#"<!doctype html><html lang="zh"><meta charset="UTF-8"><title>Weft browser check</title><body style="font:24px system-ui;padding:40px"><label>Name <input id="name"></label><button id="go" onclick="document.getElementById('result').textContent=document.getElementById('name').value;console.log('clicked')">Apply</button><p id="result">Waiting</p></body></html>"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                    html.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let state = tempfile::tempdir().unwrap();
    let browser = BrowserClient::new(
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        state.path().into(),
    );
    let cancel = CancellationToken::new();
    let page = browser.open(&url, cancel.clone()).await.unwrap();
    browser
        .input(&page, "#name", "你好 Weft", cancel.clone())
        .await
        .unwrap();
    browser.click(&page, "#go", cancel.clone()).await.unwrap();
    let snapshot = browser.inspect(&page, cancel).await.unwrap();
    assert!(snapshot.dom.contains("你好 Weft"));
    assert!(snapshot.accessibility.to_string().contains("你好 Weft"));
    assert!(snapshot.console.to_string().contains("clicked"));
    assert!(
        std::fs::read(&snapshot.screenshot)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
    if let Some(directory) = std::env::var_os("WEFT_BROWSER_ARTIFACTS") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let image = directory.join("browser.png");
        std::fs::copy(&snapshot.screenshot, &image).unwrap();
        let mut record = serde_json::to_value(&snapshot).unwrap();
        record["screenshot"] = serde_json::json!(image);
        std::fs::write(
            directory.join("browser.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
    }
    browser.close().await.unwrap();
    server.abort();
}

#[test]
fn image_tool_output_uses_native_image_parts() {
    let mut context = weft::model::Context::default();
    context.tool_images(
        "shot",
        &serde_json::json!({"observed":"screen"}),
        &[weft::tools::ToolImage {
            data_url: "data:image/png;base64,aGVsbG8=".into(),
        }],
    );
    let output = &context.items[0]["output"];
    assert!(output.is_array());
    assert_eq!(output[1]["type"], "input_image");
    assert!(
        output[1]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
}
