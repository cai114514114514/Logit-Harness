use loh::tools::{
    Scope,
    browser::{BrowserClient, Permission},
};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

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
                let html = r#"<!doctype html><html lang="zh"><meta charset="UTF-8"><title>Logit- Harness browser check</title><body style="font:24px system-ui;padding:40px"><label>Name <input id="name"></label><button id="go" onclick="document.getElementById('result').textContent=document.getElementById('name').value;console.log('clicked')">Apply</button><p id="result">Waiting</p></body></html>"#;
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
    let args = serde_json::json!({"page":page});
    let scope = Scope::default();
    let permission = Permission {
        name: "browser_input",
        arguments: &args,
        scope: &scope,
    };
    browser
        .input(
            &page,
            "#name",
            "你好 Logit- Harness",
            &permission,
            cancel.clone(),
        )
        .await
        .unwrap();
    browser
        .click(&page, "#go", &permission, cancel.clone())
        .await
        .unwrap();
    let snapshot = browser.inspect(&page, cancel).await.unwrap();
    assert!(snapshot.dom.contains("你好 Logit- Harness"));
    assert!(
        snapshot
            .accessibility
            .to_string()
            .contains("你好 Logit- Harness")
    );
    assert!(snapshot.console.to_string().contains("clicked"));
    assert!(
        std::fs::read(&snapshot.screenshot)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
    if let Some(directory) = std::env::var_os("LOH_BROWSER_ARTIFACTS") {
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
    let mut context = loh::model::Context::default();
    context.tool_images(
        "shot",
        &serde_json::json!({"observed":"screen"}),
        &[loh::tools::ToolImage {
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

#[test]
fn remote_browser_approval_is_origin_scoped_and_never_elevates_readonly_actors() {
    use loh::tools::ToolError;
    let arguments = serde_json::json!({"page":"owned","selector":"#save"});
    let mut scope = Scope::default();
    let error = Permission {
        name: "browser_click",
        arguments: &arguments,
        scope: &scope,
    }
    .check("https://one.example/form")
    .unwrap_err();
    let ToolError::Approval { id, .. } = error.downcast_ref::<ToolError>().unwrap() else {
        panic!("expected scoped approval")
    };
    scope.approvals.insert(id.clone());
    let permission = Permission {
        name: "browser_click",
        arguments: &arguments,
        scope: &scope,
    };
    assert!(permission.check("https://one.example/form").is_ok());
    assert!(permission.check("https://two.example/form").is_err());
    assert!(permission.check("https://one.example:8443/form").is_err());
    scope.readonly = true;
    assert!(
        Permission {
            name: "browser_click",
            arguments: &arguments,
            scope: &scope
        }
        .check("https://one.example/form")
        .is_err()
    );
}

#[tokio::test]
async fn browser_rechecks_live_origin_after_script_navigation() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut bytes = [0; 4096];
                let _ = socket.read(&mut bytes).await;
                let html = format!(
                    r#"<!doctype html><html><head><meta charset="utf-8"></head><body><button id="navigate" onclick="setTimeout(()=>location.href='http://[::ffff:127.0.0.1]:{port}/',100)">Navigate</button><button id="save" onclick="document.body.dataset.saved='yes'">Save</button></body></html>"#
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                    html.len()
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
    let page = browser
        .open(&format!("http://127.0.0.1:{port}/"), cancel.clone())
        .await
        .unwrap();
    let scope = Scope::default();
    let args = serde_json::json!({"page":page,"selector":"#navigate"});
    browser
        .click(
            &page,
            "#navigate",
            &Permission {
                name: "browser_click",
                arguments: &args,
                scope: &scope,
            },
            cancel.clone(),
        )
        .await
        .unwrap();
    // Navigation happens after the first interaction returns. No inspect call may
    // refresh remembered state before the second interaction's own authorization.
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let args = serde_json::json!({"page":page,"selector":"#save"});
    let result = browser
        .click(
            &page,
            "#save",
            &Permission {
                name: "browser_click",
                arguments: &args,
                scope: &scope,
            },
            cancel.clone(),
        )
        .await;
    let snapshot = browser.inspect(&page, cancel).await.unwrap();
    browser.close().await.unwrap();
    server.abort();
    assert!(snapshot.url.starts_with("http://[::ffff:"));
    assert!(result.unwrap_err().is::<loh::tools::ToolError>());
    assert!(!snapshot.dom.contains("data-saved=\"yes\""));
}
