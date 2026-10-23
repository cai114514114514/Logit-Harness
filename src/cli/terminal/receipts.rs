//! A queued acknowledgement is not application. Preserve literal input once,
//! then append only meaningful receipt transitions to immutable scrollback.
use crate::runtime::team::{MemberControl, Receipt, ReceiptStatus};
use crate::runtime::view::ViewTarget;
use ratatui::text::{Line, Span};

pub(super) struct Delivery {
    origin: ViewTarget,
    status: ReceiptStatus,
    request: crate::runtime::team::Request,
}
pub(in crate::cli::terminal) fn input(action: &MemberControl) -> String {
    match action {
        MemberControl::Message { text } | MemberControl::Side { text } => text.clone(),
        MemberControl::Skill { id, arguments, .. } => format!("/skills run {id} {arguments}"),
        MemberControl::Pause => "/pause".into(),
        MemberControl::Resume => "/resume".into(),
        MemberControl::Stop => "/stop".into(),
        MemberControl::Compact => "/compact".into(),
        MemberControl::Limit { limit } => format!("/team limit {limit}"),
    }
}
pub(in crate::cli::terminal) fn detail(receipt: &Receipt) -> String {
    described(
        receipt,
        &super::super::team::identity(receipt.request.target.member_id()),
    )
}
fn described(receipt: &Receipt, name: &str) -> String {
    let status = match receipt.status {
        ReceiptStatus::Pending => "待应用",
        ReceiptStatus::Applied => "已应用",
        ReceiptStatus::Failed => "应用失败",
    };
    let mut text = format!(
        "{name} · {status} · 请求 {} · {}",
        super::super::team::identity(&receipt.request.id),
        super::super::team::assignment(&receipt.request.target)
    );
    if let Some(error) = &receipt.error {
        text.push_str(&format!(
            "\n原因：{error}\n保留输入：\n{}",
            input(&receipt.request.action)
        ));
        if error.contains("/answer ") {
            text.push_str(&format!("\n核对实际效果后到 main：/agent main，再 /answer {} <确认后的要求>。不要自动重试。", receipt.request.recovery_id()));
        }
    }
    text
}

use super::{Kind, Transcript};
impl Transcript {
    pub(in crate::cli::terminal) fn accepted(
        &mut self,
        origin: &crate::runtime::view::ViewTarget,
        request: &crate::runtime::team::Request,
    ) {
        if self.deliveries.contains_key(&request.key()) {
            return;
        }
        self.deliveries.insert(
            request.key(),
            Delivery {
                origin: origin.clone(),
                status: ReceiptStatus::Pending,
                request: request.clone(),
            },
        );
        self.note_in(
            origin,
            &format!(
                "{} · 已接收（持久化确认）· 请求 {} · 应用状态见回执",
                self.name(request.target.member_id()),
                super::super::team::identity(&request.id)
            ),
        );
        self.entries.last_mut().unwrap().result = Some(serde_json::json!({"accepted": request}));
        self.entries.last_mut().unwrap().quiet = true;
    }
    pub(super) fn mail(&mut self, mail: &crate::runtime::team::Mail, stamp: &str) {
        if !self.entries.iter().any(|e| e.id == mail.id) {
            let i = self.add(
                mail.id.clone(),
                if mail.user { Kind::User } else { Kind::Note },
                &mail.from,
                if mail.user {
                    "你".into()
                } else {
                    format!("来自 {}", mail.from)
                },
                stamp,
            );
            self.entries[i].body = if mail.user {
                mail.text.clone()
            } else {
                format!("来自 {}：{}", self.name(&mail.from), mail.text)
            };
            self.entries[i].result = Some(serde_json::json!({"mail":mail}));
            self.done(i);
        }
        let id = format!("mail-status:{}", mail.id);
        let text = if let Some(error) = &mail.error {
            format!("投递失败：{error}\n保留输入：{}", mail.text)
        } else if mail.delivered {
            "已投递".into()
        } else {
            "待投递".into()
        };
        self.update_notice(id.clone(), text, stamp);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.quiet = mail.error.is_none();
        }
    }
    pub(super) fn receipt(&mut self, receipt: &crate::runtime::team::Receipt, stamp: &str) {
        if receipt.status == ReceiptStatus::Pending
            && self
                .deliveries
                .get(&receipt.request.key())
                .is_some_and(|d| d.status != ReceiptStatus::Pending)
        {
            return;
        }
        self.deliveries.insert(
            receipt.request.key(),
            Delivery {
                origin: self.origin.clone(),
                status: receipt.status.clone(),
                request: receipt.request.clone(),
            },
        );
        let id = receipt.request.mail_id();
        if !self.entries.iter().any(|e| e.id == id) {
            let i = self.add(id, Kind::User, "user", "你".into(), stamp);
            self.entries[i].body = input(&receipt.request.action);
            self.done(i);
        }
        let id = format!("receipt:{}", receipt.request.key());
        self.update_notice(
            id.clone(),
            described(receipt, &self.name(receipt.request.target.member_id())),
            stamp,
        );
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .unwrap();
        entry.result = Some(serde_json::json!({"receipt": receipt}));
        entry.quiet = receipt.status != ReceiptStatus::Failed;
    }

    pub fn pending_lines(&self, width: usize) -> Vec<Line<'static>> {
        self.deliveries
            .values()
            .filter(|delivery| {
                delivery.status == ReceiptStatus::Pending
                    && self.view.as_ref().is_none_or(|view| {
                        super::super::ownership::same_view(view, &delivery.origin)
                    })
            })
            .flat_map(|delivery| {
                let stale = self
                    .view
                    .as_ref()
                    .is_some_and(|view| view.target != delivery.origin.target);
                super::super::text::wrap(
                    vec![Span::raw(format!(
                        "{} · 已接收，等待处理{} · {}",
                        self.name(delivery.request.target.member_id()),
                        if stale { "（旧任务）" } else { "" },
                        super::super::text::ellipsize(
                            &input(&delivery.request.action).replace('\n', " "),
                            32
                        )
                    ))],
                    width,
                )
            })
            .collect()
    }
    fn update_notice(&mut self, id: String, text: String, stamp: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.id == id) {
            if self.entries[i].body == text {
                return;
            }
            // Settled native rows are immutable. Append the changed status once,
            // while the durable history entry retains its one correlated ID.
            self.entries[i].printed.clear();
            self.entries[i].body = text;
            self.entries[i].updated = stamp.into();
            self.entries[i].settled = Some(self.sequence);
        } else {
            let i = self.add(id, Kind::Note, "receipt", "回执".into(), stamp);
            self.entries[i].body = text;
            self.done(i);
        }
    }
}
