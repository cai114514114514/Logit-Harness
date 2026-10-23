//! Delivery uses captured origins and durable request IDs; native launch is explicit.
use super::jobs::{Context, Jobs, Reply};
use crate::{
    runtime::{
        daemon,
        ipc::{self, ClientMessage, ServerMessage},
        state::Control,
    },
    store::Store,
};
impl Context {
    pub fn window(self, jobs: &mut Jobs, target: crate::runtime::view::ViewTarget) {
        jobs.spawn(async move {
            let origin = self.view.clone();
            let result = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                let command = crate::commands::window::prepare(
                    &self.paths,
                    &std::env::current_exe()?,
                    self.session
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("请先打开会话"))?,
                    &target,
                )?;
                let result = crate::commands::window::launch(command);
                Ok(format!(
                    "{}\n手动连接：{}",
                    result.error.unwrap_or_else(|| "已请求打开成员终端".into()),
                    result.attach
                ))
            })
            .await;
            (
                self.generation,
                Ok(Reply::Notice {
                    origin,
                    text: match result {
                        Ok(Ok(text)) => text,
                        Ok(Err(error)) => format!("打开失败：{error}"),
                        Err(error) => format!("打开中断：{error}"),
                    },
                }),
            )
        });
    }
    pub fn control(self, jobs: &mut Jobs, command: Control, draft: String, connected: bool) {
        let generation = self.generation;
        jobs.spawn(async move {
            let result = async {
                let id = self
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("请先打开会话"))?;
                if let Control::Directed { request } = &command {
                    let socket = self.paths.socket(id)?;
                    if !connected {
                        if tokio::net::UnixStream::connect(&socket).await.is_ok() {
                            ipc::check_protocol(&socket).await?;
                        }
                        let store = Store::open(&self.paths.database())?;
                        store.enqueue_once(id, &request.key(), &serde_json::to_value(&command)?)?;
                        daemon::launch(&self.paths, id).await?;
                    } else {
                        match ipc::request(
                            &socket,
                            ClientMessage::Addressed {
                                request: request.clone(),
                            },
                        )
                        .await
                        {
                            Ok(ServerMessage::Accepted { .. }) => (),
                            Ok(ServerMessage::Error { message }) => anyhow::bail!(message),
                            result => {
                                // A broken acknowledgement may follow persistence. Query
                                // the same ID; never manufacture a new request for retry.
                                if let Ok(ServerMessage::Receipt {
                                    receipt: Some(receipt),
                                }) = ipc::request(
                                    &socket,
                                    ClientMessage::Receipt {
                                        client_id: request.client_id.clone(),
                                        id: request.id.clone(),
                                    },
                                )
                                .await
                                {
                                    return Ok(Reply::Notice {
                                        origin: self.view.clone(),
                                        text: super::transcript::receipts::detail(&receipt),
                                    });
                                }
                                anyhow::bail!(
                                    "原请求 {} 送达状态未知：{result:?}；先核对回执，勿重新提交",
                                    request.id
                                );
                            }
                        }
                    }
                    return Ok(Reply::Accepted {
                        origin: self.view.clone(),
                        request: request.clone(),
                    });
                }
                if !connected {
                    let socket = self.paths.socket(id)?;
                    if tokio::net::UnixStream::connect(&socket).await.is_ok() {
                        ipc::check_protocol(&socket).await?;
                    }
                    if matches!(command, Control::Resume) {
                        daemon::resume(&self.paths, id).await?;
                        return Ok(Reply::Sent);
                    }
                    // Persist before starting an archived owner: /pause or /plan
                    // must be applied before it can schedule old engineering work.
                    let database = self.paths.database();
                    let target = id.clone();
                    let data = serde_json::to_value(&command)?;
                    tokio::task::spawn_blocking(move || {
                        Store::open(&database)?.enqueue(&target, &data)
                    })
                    .await??;
                    daemon::launch(&self.paths, id).await.map_err(|error| {
                        anyhow::anyhow!(
                            "指令已保存，后台启动失败：{error}。用 /resume 恢复；不要重复提交原指令"
                        )
                    })?;
                    return Ok(Reply::Sent);
                }
                match ipc::request(&self.paths.socket(id)?, ClientMessage::Command { command })
                    .await?
                {
                    ServerMessage::Ack => Ok(Reply::Sent),
                    ServerMessage::Error { message } => Err(anyhow::anyhow!(message)),
                    message => Ok(Reply::Message(message)),
                }
            }
            .await;
            (
                generation,
                result.or_else(|error: anyhow::Error| {
                    Ok(Reply::Failed {
                        origin: self.view.clone(),
                        text: format!("未确认送达：{error}。请先查看会话记录，避免重复提交。"),
                        draft,
                        revision: self.revision,
                    })
                }),
            )
        });
    }
}
