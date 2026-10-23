use loh::{
    cli::terminal::Transcript,
    domain::{EventData, SessionEvent},
};
use serde_json::json;

fn push(p: &mut Transcript, data: EventData) {
    p.receive(&SessionEvent {
        sequence: p.sequence() + 1,
        session: "fixture".into(),
        timestamp: "2026-10-23T00:00:00Z".into(),
        data,
    });
}
fn read(p: &mut Transcript, id: &str, response: &str, success: bool) {
    push(
        p,
        EventData::Identity {
            actor: "main".into(),
            response: response.into(),
            turn: "turn".into(),
            goal: None,
            action: Some(id.into()),
        },
    );
    push(
        p,
        EventData::ActionStarted {
            id: id.into(),
            tool: "read_file".into(),
            arguments: json!({"path":format!("{id}.py")}),
        },
    );
    push(
        p,
        EventData::ActionFinished {
            id: id.into(),
            success,
            result: json!({"path":format!("{id}.py"),"content":"complete source body"}),
        },
    );
}
fn text(rows: Vec<ratatui::text::Line<'static>>) -> String {
    rows.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn successful_read_is_one_summary_line() {
    let mut p = Transcript::default();
    read(&mut p, "calc", "r", true);
    let rows = p.drain_settled(80);
    assert_eq!(
        rows.iter()
            .filter(|r| !r.to_string().trim().is_empty())
            .count(),
        1
    );
    assert!(text(rows).contains("calc.py"));
    assert!(p.entries()[0].full_text().contains("complete source body"));
    assert!(p.drain_settled(80).is_empty());
}
#[test]
fn summary_never_crosses_response_and_keeps_hidden_steps_in_details() {
    let mut p = Transcript::default();
    for id in ["one", "two", "three", "four"] {
        read(&mut p, id, "r1", true);
    }
    let rows = text(p.drain_settled(80));
    assert!(
        rows.lines().filter(|s| !s.trim().is_empty()).count() <= 3,
        "{rows}"
    );
    assert!(rows.contains("/history"));
    assert!(text(p.lines(80, true)).contains("four.py"));
    for id in ["five", "six", "seven", "eight"] {
        read(&mut p, id, id, true);
    }
    let rows = text(p.drain_settled(80));
    for id in ["five", "six", "seven", "eight"] {
        assert!(rows.contains(id));
    }
}
#[test]
fn failed_command_keeps_exit_code_and_bounded_error_tail() {
    let mut p = Transcript::default();
    push(
        &mut p,
        EventData::ActionStarted {
            id: "run".into(),
            tool: "shell".into(),
            arguments: json!({"command":"python3 -m unittest"}),
        },
    );
    push(
        &mut p,
        EventData::ActionFinished {
            id: "run".into(),
            success: false,
            result: json!({"exit_code":9,"output":"line1\nline2\nline3\nline4\nline5\nline6\nline7\nlast error"}),
        },
    );
    let rows = text(p.drain_settled(80));
    assert!(rows.contains("退出 9"), "{rows}");
    assert!(rows.contains("last error"));
    assert!(!rows.contains("line1"));
    assert!(
        rows.lines().filter(|s| !s.trim().is_empty()).count() <= 6,
        "{rows}"
    );
    assert!(text(p.lines(80, true)).contains("line1"));
}
#[test]
fn failed_read_keeps_error_and_source_limit() {
    let mut p = Transcript::default();
    push(
        &mut p,
        EventData::ActionStarted {
            id: "a".into(),
            tool: "read_file".into(),
            arguments: json!({"path":"large.py"}),
        },
    );
    push(
        &mut p,
        EventData::ActionFinished {
            id: "a".into(),
            success: false,
            result: json!({"error":"cancelled","truncated":true}),
        },
    );
    let rows = text(p.drain_settled(80));
    assert!(
        rows.contains("cancelled") && rows.contains("截断"),
        "{rows}"
    );
}
