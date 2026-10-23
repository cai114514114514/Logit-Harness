//! Own the runner, socket task and resource shutdown as one daemon lifetime.
use crate::runtime::state::Control;
use anyhow::Context;
use futures::future::BoxFuture;
use std::{fmt, future::Future};
use tokio::{
    sync::mpsc,
    task::{JoinError, JoinHandle},
};
use tokio_util::sync::CancellationToken;

pub(super) type Shutdown<'a> = (&'static str, BoxFuture<'a, anyhow::Result<()>>);

enum Exit<T> {
    Runner(Result<anyhow::Result<T>, JoinError>),
    Socket(Result<anyhow::Result<()>, JoinError>),
    Signal(anyhow::Result<()>),
}

pub(super) async fn run<T>(
    mut runner: JoinHandle<anyhow::Result<T>>,
    mut socket: JoinHandle<anyhow::Result<()>>,
    control: mpsc::Sender<Control>,
    cancel: CancellationToken,
    signal: impl Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    let exit = tokio::select! {
        result = &mut runner => Exit::Runner(result),
        result = &mut socket => Exit::Socket(result),
        result = signal => Exit::Signal(result),
    };
    // Stop accepting commands before asking the runner to settle. Aborting the
    // runner would drop active writers before their cancellation is recorded.
    cancel.cancel();
    let mut failures = Failures::default();
    let mut runner_done = false;
    let mut socket_done = false;
    match exit {
        Exit::Runner(result) => {
            runner_done = true;
            failures.record(joined(result).context("runner failed"));
        }
        Exit::Socket(result) => {
            socket_done = true;
            failures.record(match joined(result) {
                Ok(()) => Err(anyhow::anyhow!("socket server ended unexpectedly")),
                Err(error) => Err(error.context("socket server failed")),
            });
        }
        Exit::Signal(result) => failures.record(result.context("shutdown signal failed")),
    }
    if !runner_done {
        failures.record(
            control
                .send(Control::Stop)
                .await
                .context("could not send runner stop"),
        );
        // A closed control channel is not a completed task: even after a failed
        // send, wait for the runner's remaining work and retain its result.
        failures.record(joined(runner.await).context("runner failed"));
    }
    if !socket_done {
        // The select branch already consumed a completed handle's result. Only
        // await the other handle here; polling a JoinHandle twice can panic.
        failures.record(joined(socket.await).context("socket shutdown failed"));
    }
    failures.finish()
}

fn joined<T>(result: Result<anyhow::Result<T>, JoinError>) -> anyhow::Result<()> {
    result?.map(|_| ())
}

pub(super) async fn finish<'a>(
    work: impl Future<Output = anyhow::Result<()>>,
    shutdown: impl IntoIterator<Item = Shutdown<'a>>,
) -> anyhow::Result<()> {
    let mut failures = Failures::default();
    failures.record(work.await);
    for (name, close) in shutdown {
        failures.record(
            close
                .await
                .with_context(|| format!("{name} shutdown failed")),
        );
    }
    failures.finish()
}

/// Keep the initiating error as the source, while reporting every later failure.
#[derive(Debug, Default)]
pub(super) struct Failures(Vec<anyhow::Error>);

impl Failures {
    pub(super) fn record(&mut self, result: anyhow::Result<()>) {
        if let Err(error) = result {
            self.0.push(error);
        }
    }

    pub(super) fn finish(mut self) -> anyhow::Result<()> {
        match self.0.len() {
            0 => Ok(()),
            1 => Err(self.0.remove(0)),
            _ => Err(self.into()),
        }
    }
}

impl fmt::Display for Failures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str("; additionally: ")?;
            }
            write!(formatter, "{error:#}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Failures {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.first().map(|error| error.as_ref())
    }
}

#[cfg(test)]
mod tests;
