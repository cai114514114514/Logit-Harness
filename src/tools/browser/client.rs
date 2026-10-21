use base64::Engine;
use chromiumoxide::{
    Browser, BrowserConfig, Page,
    cdp::{
        browser_protocol::{
            accessibility::GetFullAxTreeParams, input::InsertTextParams,
            page::CaptureScreenshotFormat,
        },
        js_protocol::runtime::{EventConsoleApiCalled, EventExceptionThrown},
    },
    page::ScreenshotParams,
};
use futures::StreamExt;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Serialize)]
pub struct Snapshot {
    pub page: String,
    pub url: String,
    pub dom: String,
    pub accessibility: Value,
    pub console: Value,
    pub screenshot: PathBuf,
    #[serde(skip)]
    pub image: String,
}
struct Session {
    browser: Browser,
    pages: BTreeMap<String, Page>,
    listeners: Vec<tokio::task::JoinHandle<()>>,
    handler: tokio::task::JoinHandle<()>,
}
pub struct BrowserClient {
    chrome: PathBuf,
    directory: PathBuf,
    session: Mutex<Option<Session>>,
    console: Arc<StdMutex<BTreeMap<String, Vec<Value>>>>,
    locations: StdMutex<BTreeMap<String, String>>,
}
impl BrowserClient {
    pub fn new(chrome: PathBuf, directory: PathBuf) -> Self {
        Self {
            chrome,
            directory,
            session: Mutex::new(None),
            console: Arc::new(StdMutex::new(BTreeMap::new())),
            locations: StdMutex::new(BTreeMap::new()),
        }
    }
    async fn launch(&self) -> anyhow::Result<Session> {
        std::fs::create_dir_all(&self.directory)?;
        let secrets = std::env::vars()
            .filter(|(key, _)| {
                let key = key.to_uppercase();
                key.ends_with("API_KEY") || key.ends_with("ACCESS_TOKEN")
            })
            .map(|(key, _)| (key, String::new()));
        let config = BrowserConfig::builder()
            .envs(secrets)
            .chrome_executable(&self.chrome)
            .user_data_dir(
                self.directory
                    .join(format!("profile-{}", uuid::Uuid::new_v4())),
            )
            .new_headless_mode()
            .respect_https_errors()
            .request_timeout(Duration::from_secs(15))
            .build()
            .map_err(anyhow::Error::msg)?;
        let (mut browser, mut events) = Browser::launch(config).await?;
        let pid = browser
            .get_mut_child()
            .and_then(|child| child.as_mut_inner().id())
            .ok_or_else(|| anyhow::anyhow!("owned Chrome process missing"))?;
        let identity = crate::tools::identity::Identity::capture(pid)?
            .ok_or_else(|| anyhow::anyhow!("Chrome exited during startup"))?;
        let mut record = tempfile::NamedTempFile::new_in(&self.directory)?;
        serde_json::to_writer(&mut record, &identity)?;
        record.as_file().sync_all()?;
        record.persist(self.directory.join("process.json"))?;
        let handler = tokio::spawn(async move {
            while let Some(event) = events.next().await {
                if event.is_err() {
                    break;
                }
            }
        });
        Ok(Session {
            browser,
            pages: BTreeMap::new(),
            listeners: vec![],
            handler,
        })
    }
    pub fn local(&self, page: &str) -> bool {
        self.locations
            .lock()
            .ok()
            .and_then(|m| m.get(page).cloned())
            .and_then(|s| reqwest::Url::parse(&s).ok())
            .is_some_and(|url| matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    }
    pub async fn open(&self, url: &str, cancel: CancellationToken) -> anyhow::Result<String> {
        let parsed = reqwest::Url::parse(url)?;
        anyhow::ensure!(
            matches!(parsed.scheme(), "http" | "https"),
            "browser requires HTTP(S)"
        );
        let operation = async {
            let mut state = self.session.lock().await;
            if state.is_none() {
                *state = Some(self.launch().await?);
            }
            let session = state.as_mut().unwrap();
            let page = session.browser.new_page("about:blank").await?;
            let id = uuid::Uuid::new_v4().simple().to_string();
            let mut logs = page.event_listener::<EventConsoleApiCalled>().await?;
            let output = self.console.clone();
            let key = id.clone();
            session.listeners.push(tokio::spawn(async move {
                while let Some(event) = logs.next().await {
                    push(
                        &output,
                        &key,
                        serde_json::to_value(&*event).unwrap_or_default(),
                    );
                }
            }));
            let mut exceptions = page.event_listener::<EventExceptionThrown>().await?;
            let output = self.console.clone();
            let key = id.clone();
            session.listeners.push(tokio::spawn(async move {
                while let Some(event) = exceptions.next().await {
                    push(
                        &output,
                        &key,
                        serde_json::to_value(&*event).unwrap_or_default(),
                    );
                }
            }));
            page.goto(url).await?;
            let location = page.url().await?.unwrap_or_else(|| url.into());
            self.locations
                .lock()
                .map_err(|_| anyhow::anyhow!("browser location lock poisoned"))?
                .insert(id.clone(), location);
            session.pages.insert(id.clone(), page);
            Ok(id)
        };
        tokio::select! { biased; _=cancel.cancelled()=>anyhow::bail!("navigation cancelled; inspect actual browser state before retrying"), result=operation=>result }
    }
    pub async fn input(
        &self,
        id: &str,
        selector: &str,
        text: &str,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let operation = async {
            let state = self.session.lock().await;
            let page = owned_page(&state, id)?;
            let element = page.find_element(selector).await?;
            element.focus().await?;
            element
                .call_js_fn("function() { if (this.select) this.select(); }", false)
                .await?;
            // CDP's text insertion supports Unicode/IME text, unlike ASCII key maps.
            page.execute(InsertTextParams::new(text)).await?;
            Ok(())
        };
        tokio::select! {biased; _=cancel.cancelled()=>anyhow::bail!("input cancelled; its effect may already be visible"), result=operation=>result}
    }
    pub async fn click(
        &self,
        id: &str,
        selector: &str,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let operation = async {
            let state = self.session.lock().await;
            let page = owned_page(&state, id)?;
            page.find_element(selector).await?.click().await?;
            if let Some(url) = page.url().await? {
                self.locations
                    .lock()
                    .map_err(|_| anyhow::anyhow!("browser location lock poisoned"))?
                    .insert(id.into(), url);
            }
            Ok(())
        };
        tokio::select! {biased; _=cancel.cancelled()=>anyhow::bail!("click cancelled; inspect its actual effect before retrying"), result=operation=>result}
    }
    pub async fn inspect(&self, id: &str, cancel: CancellationToken) -> anyhow::Result<Snapshot> {
        let operation = async {
            let state = self.session.lock().await;
            let page = owned_page(&state, id)?;
            let dom: String = page
                .evaluate("document.documentElement.outerHTML")
                .await?
                .into_value()?;
            let ax = page.execute(GetFullAxTreeParams::default()).await?;
            let bytes = page
                .screenshot(
                    ScreenshotParams::builder()
                        .format(CaptureScreenshotFormat::Png)
                        .full_page(false)
                        .build(),
                )
                .await?;
            anyhow::ensure!(
                bytes.len() <= 8 * 1024 * 1024,
                "screenshot exceeds the inline image limit"
            );
            let screenshot = self.directory.join(format!("{}.png", uuid::Uuid::new_v4()));
            std::fs::write(&screenshot, &bytes)?;
            let url = page.url().await?.unwrap_or_default();
            self.locations
                .lock()
                .map_err(|_| anyhow::anyhow!("browser location lock poisoned"))?
                .insert(id.into(), url.clone());
            let console = serde_json::to_value(
                self.console
                    .lock()
                    .map_err(|_| anyhow::anyhow!("browser console lock poisoned"))?
                    .get(id)
                    .cloned()
                    .unwrap_or_default(),
            )?;
            let snapshot = Snapshot {
                page: id.into(),
                url,
                dom,
                accessibility: serde_json::to_value(ax.result)?,
                console,
                screenshot: screenshot.clone(),
                image: format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ),
            };
            std::fs::write(
                screenshot.with_extension("json"),
                serde_json::to_vec(&snapshot)?,
            )?;
            Ok(snapshot)
        };
        tokio::select! {biased; _=cancel.cancelled()=>anyhow::bail!("browser inspection cancelled"), result=operation=>result}
    }
    pub async fn close(&self) -> anyhow::Result<()> {
        if let Some(mut session) = self.session.lock().await.take() {
            if !matches!(
                tokio::time::timeout(Duration::from_secs(3), session.browser.close()).await,
                Ok(Ok(_))
            ) {
                let _ = session.browser.kill().await;
            }
            session.browser.wait().await?;
            for listener in session.listeners {
                listener.abort();
            }
            session.handler.abort();
        }
        Ok(())
    }
    /// Daemon calls this while holding exclusive workspace ownership, before new work.
    pub async fn recover(&self) -> anyhow::Result<()> {
        let record = self.directory.join("process.json");
        if record.is_file() {
            let identity: crate::tools::identity::Identity =
                serde_json::from_slice(&std::fs::read(record)?)?;
            if crate::tools::identity::Identity::capture(identity.pid)?.as_ref() == Some(&identity)
            {
                identity.stop_process().await?;
            }
        }
        Ok(())
    }
}
fn owned_page<'a>(session: &'a Option<Session>, id: &str) -> anyhow::Result<&'a Page> {
    session
        .as_ref()
        .and_then(|s| s.pages.get(id))
        .ok_or_else(|| anyhow::anyhow!("unknown owned browser page"))
}
fn push(output: &StdMutex<BTreeMap<String, Vec<Value>>>, key: &str, event: Value) {
    if let Ok(mut output) = output.lock() {
        let entries = output.entry(key.into()).or_default();
        entries.push(event);
        if entries.len() > 200 {
            entries.remove(0);
        }
    }
}
