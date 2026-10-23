use super::{Shutdown, finish, run};
use crate::runtime::state::Control;
use futures::FutureExt;
use std::{future::pending, net::SocketAddr, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

/// Real listeners model resources owned by the daemon. Their shutdown futures
/// only return after the listening task exits, so a missed close remains visible.
struct Resource {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Resource {
    async fn open() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _listener = listener;
            let _ = stopped.await;
        });
        Self {
            address,
            stop: Some(stop),
            task: Some(task),
        }
    }

    fn shutdown(&mut self, name: &'static str, fail: bool) -> Shutdown<'_> {
        (
            name,
            async move {
                self.stop.take().unwrap().send(()).unwrap();
                self.task.take().unwrap().await?;
                anyhow::ensure!(!fail, "{name} fixture close failed");
                Ok(())
            }
            .boxed(),
        )
    }

    async fn is_closed(&self) -> bool {
        TcpStream::connect(self.address).await.is_err()
    }
}

impl Drop for Resource {
    fn drop(&mut self) {
        // Test failure must not leave fixture tasks alive. The sender dropping
        // wakes the resource; production shutdown is asserted before teardown.
        self.stop.take();
    }
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), future)
        .await
        .expect("daemon supervision did not finish")
}

#[derive(Clone, Copy)]
enum SocketExit {
    Closed,
    Error,
    Panic,
}

async fn socket_exit_closes_resources(exit: SocketExit) {
    let mut service = Resource::open().await;
    let mut browser = Resource::open().await;
    let mut mcp = Resource::open().await;
    let root = tempfile::tempdir().unwrap();
    let settled = root.path().join("writer-settled");
    let (control, mut receiver) = mpsc::channel(1);
    let (stopping, stopped) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let runner = tokio::spawn({
        let settled = settled.clone();
        async move {
            assert!(matches!(receiver.recv().await, Some(Control::Stop)));
            stopping.send(()).unwrap();
            released.await.unwrap();
            tokio::fs::write(settled, "durable writer settlement").await?;
            Ok(())
        }
    });
    let runner_finished = runner.abort_handle();
    let cancel = CancellationToken::new();
    let socket = tokio::spawn(async move {
        match exit {
            SocketExit::Closed => Ok(()),
            SocketExit::Error => anyhow::bail!("fixture socket failed"),
            SocketExit::Panic => panic!("fixture socket panicked"),
        }
    });
    let work = finish(
        run(runner, socket, control, cancel.clone(), pending()),
        [
            service.shutdown("services", false),
            browser.shutdown("browser", false),
            mcp.shutdown("MCP", false),
        ],
    );
    // Keep the writer in settlement while independently checking the daemon has
    // neither returned nor aborted it. Then let it persist its final write.
    let release_writer = async {
        stopped.await.unwrap();
        assert!(!runner_finished.is_finished());
        assert!(!settled.exists());
        release.send(()).unwrap();
    };
    let (result, ()) = bounded(async { tokio::join!(work, release_writer) }).await;
    let message = format!("{:#}", result.unwrap_err());
    match exit {
        SocketExit::Closed => assert!(message.contains("socket server ended unexpectedly")),
        SocketExit::Error => assert!(message.contains("fixture socket failed")),
        SocketExit::Panic => assert!(message.contains("fixture socket panicked")),
    }
    assert!(runner_finished.is_finished(), "runner task leaked");
    assert_eq!(
        std::fs::read_to_string(settled).unwrap(),
        "durable writer settlement"
    );
    assert!(service.is_closed().await, "service listener leaked");
    assert!(browser.is_closed().await, "browser listener leaked");
    assert!(mcp.is_closed().await, "MCP listener leaked");
    assert!(cancel.is_cancelled());
}

#[tokio::test]
async fn socket_error_settles_writer_and_closes_resources() {
    socket_exit_closes_resources(SocketExit::Error).await;
}

#[tokio::test]
async fn socket_success_is_unexpected_but_still_closes_resources() {
    socket_exit_closes_resources(SocketExit::Closed).await;
}

#[tokio::test]
async fn socket_panic_settles_writer_and_closes_resources() {
    socket_exit_closes_resources(SocketExit::Panic).await;
}

#[tokio::test]
async fn shutdown_errors_do_not_skip_later_resources_or_hide_work_failure() {
    let mut service = Resource::open().await;
    let mut browser = Resource::open().await;
    let mut mcp = Resource::open().await;
    let result = finish(
        async { anyhow::bail!("fixture runner failed") },
        [
            service.shutdown("services", true),
            browser.shutdown("browser", true),
            mcp.shutdown("MCP", true),
        ],
    )
    .await;
    let error = result.unwrap_err();
    let message = format!("{error:#}");
    for expected in [
        "fixture runner failed",
        "services fixture close failed",
        "browser fixture close failed",
        "MCP fixture close failed",
    ] {
        assert!(message.contains(expected), "missing {expected}: {message}");
    }
    assert!(service.is_closed().await);
    assert!(browser.is_closed().await);
    assert!(mcp.is_closed().await);
}

#[tokio::test]
async fn first_close_error_still_closes_the_remaining_resources() {
    let mut service = Resource::open().await;
    let mut browser = Resource::open().await;
    let mut mcp = Resource::open().await;
    let error = finish(
        async { Ok(()) },
        [
            service.shutdown("services", true),
            browser.shutdown("browser", false),
            mcp.shutdown("MCP", false),
        ],
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("services fixture close failed"));
    assert!(service.is_closed().await);
    assert!(browser.is_closed().await, "browser listener leaked");
    assert!(mcp.is_closed().await, "MCP listener leaked");
}

#[tokio::test]
async fn closed_control_channel_still_waits_for_runner_and_closes_resources() {
    let root = tempfile::tempdir().unwrap();
    let settled = root.path().join("writer-settled");
    let mut resource = Resource::open().await;
    let (control, receiver) = mpsc::channel(1);
    let (ready, started) = oneshot::channel();
    let runner = tokio::spawn({
        let settled = settled.clone();
        async move {
            drop(receiver);
            ready.send(()).unwrap();
            // A closed receiver does not prove its owning task has finished.
            // Model a final asynchronous flush after control reception closes.
            tokio::time::sleep(Duration::from_millis(30)).await;
            tokio::fs::write(settled, "flushed").await?;
            anyhow::bail!("fixture runner flush failed");
        }
    });
    let runner_finished = runner.abort_handle();
    let socket = tokio::spawn(async move {
        started.await.unwrap();
        anyhow::bail!("fixture socket failed");
    });
    let result = bounded(finish(
        run::<()>(runner, socket, control, CancellationToken::new(), pending()),
        [resource.shutdown("owned resource", false)],
    ))
    .await;
    let message = format!("{:#}", result.unwrap_err());
    assert!(runner_finished.is_finished(), "runner task leaked");
    assert_eq!(std::fs::read_to_string(settled).unwrap(), "flushed");
    for expected in [
        "fixture socket failed",
        "channel closed",
        "fixture runner flush failed",
    ] {
        assert!(message.contains(expected), "missing {expected}: {message}");
    }
    assert!(resource.is_closed().await);
}

#[tokio::test]
async fn runner_panic_cancels_socket_and_closes_resources() {
    let mut resource = Resource::open().await;
    let (control, _receiver) = mpsc::channel(1);
    let runner = tokio::spawn(async { panic!("fixture runner panicked") });
    let cancel = CancellationToken::new();
    let socket = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            cancel.cancelled().await;
            anyhow::bail!("fixture socket close failed");
        }
    });
    let socket_finished = socket.abort_handle();
    let result = bounded(finish(
        run::<()>(runner, socket, control, cancel, pending()),
        [resource.shutdown("owned resource", false)],
    ))
    .await;
    let message = format!("{:#}", result.unwrap_err());
    assert!(message.contains("fixture runner panicked"));
    assert!(message.contains("fixture socket close failed"));
    assert!(socket_finished.is_finished(), "socket task leaked");
    assert!(resource.is_closed().await);
}

#[tokio::test]
async fn signal_error_stops_runner_and_closes_socket() {
    let (control, mut receiver) = mpsc::channel(1);
    let runner = tokio::spawn(async move {
        assert!(matches!(receiver.recv().await, Some(Control::Stop)));
        Ok(())
    });
    let runner_finished = runner.abort_handle();
    let cancel = CancellationToken::new();
    let socket = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            cancel.cancelled().await;
            Ok(())
        }
    });
    let socket_finished = socket.abort_handle();
    let result = bounded(run(runner, socket, control, cancel, async {
        anyhow::bail!("fixture signal failed")
    }))
    .await;
    assert!(format!("{:#}", result.unwrap_err()).contains("fixture signal failed"));
    assert!(runner_finished.is_finished(), "runner task leaked");
    assert!(socket_finished.is_finished(), "socket task leaked");
}
