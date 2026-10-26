//! Ledger cards in native history: the contract, each revision, each verdict.
//!
//! A card is drawn from its event when it is committed, at the width of that
//! moment, and never rewritten. Its plain text is the entry body, so history
//! search and copy see exactly what was printed.
use super::{
    state::clean,
    text::{self, accent, fail, fresh, muted, stale},
    tool_text,
};
use crate::domain::contract::{AcceptanceCheck, ClauseRecord, ContractChanges, StaleReason};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

/// Everything a card needs, captured when its event arrives.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "card", rename_all = "snake_case")]
pub(super) enum Card {
    Contract {
        revision: u64,
        cause: String,
        clauses: Vec<ClauseRecord>,
        changes: ContractChanges,
        /// Display numbers of evidence known when the card arrived.
        labels: BTreeMap<String, usize>,
    },
    Acceptance {
        revision: u64,
        passed: bool,
        checks: Vec<AcceptanceCheck>,
        /// Clause wording at the time of the verdict.
        clauses: BTreeMap<String, String>,
        verifier: Verifier,
        totals: Totals,
    },
}

/// What the verifier did in its own context before the verdict.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct Verifier {
    pub files: usize,
    pub checks: usize,
    pub seconds: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct Totals {
    pub seconds: Option<u64>,
    pub tokens: u64,
    pub kv: Option<f64>,
}

impl Card {
    /// The speaker whose turn the card belongs to, for spacing.
    pub fn speaker(&self) -> &'static str {
        match self {
            Card::Contract { .. } => "main",
            Card::Acceptance { .. } => "verify",
        }
    }
    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        match self {
            Card::Contract {
                revision,
                cause,
                clauses,
                ..
            } if cause == "extract" => contract(*revision, clauses, width),
            Card::Contract {
                revision,
                cause,
                clauses,
                changes,
                labels,
            } => revision_card(*revision, cause, clauses, changes, labels, width),
            Card::Acceptance {
                revision,
                passed,
                checks,
                clauses,
                verifier,
                totals,
            } => receipt(*revision, *passed, checks, clauses, verifier, totals, width),
        }
    }
    /// Plain text for history, search and copy.
    pub fn plain(&self) -> String {
        self.lines(78)
            .iter()
            .map(|line| line.to_string().trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// `╞══ title ══ subtitle ═══…` or `╘══ text ═══…`, filled to the width.
fn edge(top: bool, spans: Vec<Span<'static>>, color: Color, width: usize) -> Line<'static> {
    let style = Style::default().fg(color);
    let mut line = vec![Span::styled(if top { "╞══ " } else { "╘══ " }, style)];
    // Corner, a space and at least two rule cells always fit; the text yields.
    let room = width.saturating_sub(7);
    let spans = if spans.iter().map(|s| s.content.width()).sum::<usize>() > room {
        let text = spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        vec![Span::styled(text::ellipsize(&text, room), spans[0].style)]
    } else {
        spans
    };
    let used = 4 + spans.iter().map(|s| s.content.width()).sum::<usize>() + 1;
    line.extend(spans);
    line.push(Span::styled(
        format!(" {}", "═".repeat(width.saturating_sub(used).max(2))),
        style,
    ));
    Line::from(line)
}

fn row(color: Color, mut spans: Vec<Span<'static>>) -> Line<'static> {
    spans.insert(0, Span::styled("│ ", Style::default().fg(color)));
    Line::from(spans)
}

fn plain(text: impl Into<String>) -> Span<'static> {
    Span::raw(text.into())
}

fn quiet(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(muted()))
}

fn bold(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

/// Keep the end of a revised wording, where the change usually is.
fn ellipsize_start(text: &str, width: usize) -> String {
    if text.width() <= width || width == 0 {
        return text.into();
    }
    let reversed = text.chars().rev().collect::<String>();
    let kept = text::ellipsize(&reversed, width);
    let kept = kept.trim_end_matches('…').chars().rev().collect::<String>();
    format!("…{kept}")
}

/// A revised wording without the start it shares with the old one, cut back to
/// the last space so the word before the change stays readable.
fn changed_part(before: &str, after: &str) -> String {
    let shared = before
        .char_indices()
        .zip(after.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((i, c), _)| i + c.len_utf8());
    match after[..shared].rfind(' ') {
        Some(space) if space > 0 => format!("…{}", &after[space + 1..]),
        _ => after.to_owned(),
    }
}

fn contract(revision: u64, clauses: &[ClauseRecord], width: usize) -> Vec<Line<'static>> {
    let color = accent();
    let room = width.saturating_sub(2);
    let mut lines = vec![edge(
        true,
        vec![
            bold(format!("契约 v{revision}"), color),
            Span::styled(" ══ ", Style::default().fg(color)),
            quiet("摘自你的原话"),
        ],
        color,
        width,
    )];
    for clause in clauses.iter().filter(|c| c.active) {
        let id = format!("{:<4}", clause.id);
        let note = if clause.revision == 1 && clause.text == clause.source && clause.id == "r1" {
            "完整原话"
        } else {
            ""
        };
        let text_room =
            room.saturating_sub(id.width() + if note.is_empty() { 0 } else { note.width() + 2 });
        let text = text::ellipsize(&clean(&clause.text), text_room);
        let pad = room.saturating_sub(id.width() + text.width() + note.width());
        lines.push(row(
            color,
            vec![
                quiet(id),
                plain(text),
                plain(" ".repeat(if note.is_empty() { 0 } else { pad })),
                quiet(note),
            ],
        ));
    }
    let bottom = if width >= 60 {
        "条款只能引用你的原话 · 模型不能删减 · /contract"
    } else {
        "只引用原话 · /contract"
    };
    lines.push(edge(false, vec![quiet(bottom)], color, width));
    lines
}

fn reason(reason: &StaleReason) -> &'static str {
    match reason {
        StaleReason::Clause => "条款已修订",
        StaleReason::Epoch => "新增条款使旧证据失效",
        StaleReason::Step => "所依赖的步骤已失效",
    }
}

fn revision_card(
    revision: u64,
    cause: &str,
    clauses: &[ClauseRecord],
    changes: &ContractChanges,
    labels: &BTreeMap<String, usize>,
    width: usize,
) -> Vec<Line<'static>> {
    let color = stale();
    let room = width.saturating_sub(2);
    let narrow = width < 70;
    let from = match cause {
        "answer" => "来自你的回答",
        "member" => "来自你给成员的补充",
        "replace" => "你换了目标",
        _ => "来自你的补充",
    };
    let mut lines = vec![edge(
        true,
        vec![
            bold(format!("修订 v{revision}"), color),
            Span::styled(" ══ ", Style::default().fg(color)),
            quiet(from),
        ],
        color,
        width,
    )];
    let text = |id: &str| {
        clauses
            .iter()
            .find(|c| c.id == id)
            .map_or_else(String::new, |c| clean(&c.text))
    };
    let mark = |symbol: &'static str, tone: Color| Span::styled(symbol, Style::default().fg(tone));
    for update in &changes.updated {
        let id = format!("{:<4}", update.id);
        lines.push(row(
            color,
            vec![
                mark("~ ", color),
                quiet(id.clone()),
                plain(text::ellipsize(
                    &clean(&update.before),
                    room.saturating_sub(2 + id.width()),
                )),
            ],
        ));
        lines.push(row(
            color,
            vec![
                plain("   "),
                mark("→  ", color),
                plain(ellipsize_start(
                    &if narrow {
                        changed_part(&clean(&update.before), &clean(&update.after))
                    } else {
                        clean(&update.after)
                    },
                    room.saturating_sub(6),
                )),
            ],
        ));
    }
    for (symbol, ids, tone) in [
        ("+ ", &changes.added, fresh()),
        ("− ", &changes.retired, muted()),
    ] {
        for id in ids {
            let label = format!("{id:<4}");
            lines.push(row(
                color,
                vec![
                    mark(symbol, tone),
                    quiet(label.clone()),
                    plain(text::ellipsize(
                        &text(id),
                        room.saturating_sub(2 + label.width()),
                    )),
                ],
            ));
        }
    }
    for cancelled in &changes.cancelled {
        let state = if cancelled.settled {
            "已结算"
        } else {
            "结算中"
        };
        let detail = if narrow {
            clean(&cancelled.subject)
        } else {
            format!(
                "{} {} · 作用于 {} · {state}",
                tool_text::verb(&cancelled.tool),
                clean(&cancelled.subject),
                cancelled.requirements.join(" ")
            )
        };
        lines.push(row(
            color,
            vec![
                mark("× ", fail()),
                plain("已取消  "),
                plain(text::ellipsize(&detail, room.saturating_sub(10))),
            ],
        ));
    }
    let stale_head =
        (!changes.stale.is_empty()).then(|| format!("{} 项证据过期", changes.stale.len()));
    let kept_head = (!changes.kept.is_empty()).then(|| {
        if width < 50 {
            format!("保留 {} 步", changes.kept.len())
        } else {
            format!("保留 {} 个步骤", changes.kept.len())
        }
    });
    if narrow && (stale_head.is_some() || kept_head.is_some()) {
        let mut spans = vec![];
        if let Some(head) = &stale_head {
            spans.extend([mark("⊘ ", stale()), plain(head.clone())]);
        }
        if let Some(head) = &kept_head {
            if !spans.is_empty() {
                spans.push(quiet(" · "));
            }
            spans.extend([mark("= ", muted()), plain(head.clone())]);
        }
        lines.push(row(color, spans));
    } else {
        if let Some(head) = stale_head {
            let first = &changes.stale[0];
            let label = labels
                .get(&first.id)
                .map_or_else(String::new, |n| format!("e{n} "));
            let more = if changes.stale.len() > 1 { " 等" } else { "" };
            let detail = format!(
                "{label}{} {}{more} · {}",
                tool_text::verb(&first.tool),
                clean(&first.subject),
                reason(&first.reason)
            );
            lines.push(row(
                color,
                vec![
                    mark("⊘ ", stale()),
                    plain(format!("{head}  ")),
                    plain(text::ellipsize(
                        &detail,
                        room.saturating_sub(4 + head.width()),
                    )),
                ],
            ));
        }
        if let Some(head) = kept_head {
            let steps = changes
                .kept
                .iter()
                .map(|s| format!("{} {}", s.id, clean(&s.description)))
                .collect::<Vec<_>>()
                .join(" · ");
            let detail = format!("{steps} · 代码不回滚");
            lines.push(row(
                color,
                vec![
                    mark("= ", muted()),
                    plain(format!("{head}  ")),
                    plain(text::ellipsize(
                        &detail,
                        room.saturating_sub(4 + head.width()),
                    )),
                ],
            ));
        }
    }
    let previous = revision.saturating_sub(1).max(1);
    let bottom = if width >= 70 {
        format!("契约 v{previous} → v{revision} · /contract 查看原话与修订链")
    } else {
        format!("v{previous} → v{revision} · /contract")
    };
    lines.push(edge(false, vec![quiet(bottom)], color, width));
    lines
}

fn duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, seconds / 60 % 60)
    }
}

fn tokens(count: u64) -> String {
    if count >= 1000 {
        format!("{:.1}k tokens", count as f64 / 1000.)
    } else {
        format!("{count} tokens")
    }
}

fn evidence_verb(check: &AcceptanceCheck) -> String {
    check.evidence.first().map_or_else(
        || "无有效证据".into(),
        |cited| {
            let verb = match cited.kind {
                crate::domain::EvidenceKind::Review => "评审",
                _ => tool_text::verb(&cited.tool),
            };
            format!("{verb}  {}", clean(&cited.subject))
        },
    )
}

fn receipt(
    revision: u64,
    passed: bool,
    checks: &[AcceptanceCheck],
    clauses: &BTreeMap<String, String>,
    verifier: &Verifier,
    totals: &Totals,
    width: usize,
) -> Vec<Line<'static>> {
    let color = if passed { fresh() } else { fail() };
    let room = width.saturating_sub(2);
    let mut head = vec!["验收".to_owned(), "独立上下文".to_owned()];
    if verifier.files > 0 {
        head.push(format!("读取 {} 个文件", verifier.files));
    }
    if verifier.checks > 0 {
        head.push(format!("重跑 {} 组检查", verifier.checks));
    }
    if let Some(seconds) = verifier.seconds {
        head.push(duration(seconds));
    }
    let mut lines = vec![Line::from(vec![
        Span::styled("║ ", Style::default().fg(fresh())),
        plain(text::ellipsize(&head.join(" · "), room)),
    ])];
    let title = if passed {
        "验收回执"
    } else {
        "验收未通过"
    };
    lines.push(edge(
        true,
        vec![
            bold(title, color),
            Span::styled(" ══ ", Style::default().fg(color)),
            quiet(format!("独立验收 · 契约 v{revision}")),
        ],
        color,
        width,
    ));
    let clause_room = if width >= 70 { room * 9 / 20 } else { room / 2 };
    for check in checks {
        let (stamp, tone) = if check.passed {
            ("◉ ", fresh())
        } else {
            ("⊗ ", fail())
        };
        let id = format!("{:<4}", check.requirement);
        let wording = clauses
            .get(&check.requirement)
            .map(|t| text::ellipsize(&clean(t), clause_room))
            .unwrap_or_default();
        let gap = clause_room.saturating_sub(wording.width()) + 2;
        let used = 2 + id.width() + wording.width() + gap;
        lines.push(row(
            color,
            vec![
                Span::styled(stamp, Style::default().fg(tone)),
                quiet(id),
                plain(wording),
                plain(" ".repeat(gap)),
                plain(text::ellipsize(
                    &evidence_verb(check),
                    room.saturating_sub(used),
                )),
            ],
        ));
        if !check.passed && !check.reason.trim().is_empty() {
            for line in text::wrap(vec![plain(clean(&check.reason))], room.saturating_sub(6)) {
                let mut spans = vec![plain("      ")];
                spans.extend(line.spans);
                lines.push(row(color, spans));
            }
        }
    }
    let total = checks.len();
    let ok = checks.iter().filter(|c| c.passed).count();
    let summary = if passed {
        format!("{ok}/{total} 通过 · 证据绑定 v{revision} 条款版本、步骤版本与文件指纹")
    } else if ok < total {
        format!("{ok}/{total} 通过 · 有条款未通过时不会标记完成")
    } else {
        format!("{ok}/{total} 通过 · 完成条件尚未满足，未标记完成")
    };
    lines.push(row(color, vec![plain(text::ellipsize(&summary, room))]));
    let bottom = if passed {
        let mut parts = vec!["完成".to_owned()];
        if let Some(seconds) = totals.seconds {
            parts.push(format!("用时 {}", duration(seconds)));
        }
        if totals.tokens > 0 {
            parts.push(tokens(totals.tokens));
        }
        if let Some(kv) = totals.kv {
            parts.push(format!("KV {:.0}%", kv * 100.));
        }
        parts.join(" · ")
    } else {
        format!(
            "已把 {} 条失败原因交回 Logit · 继续修复",
            total.saturating_sub(ok).max(1)
        )
    };
    lines.push(edge(false, vec![quiet(bottom)], color, width));
    lines
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
