use super::*;
use crate::domain::{EventData, SessionEvent};
use serde_json::json;

#[test]
fn search_detail_and_both_copy_routes_keep_persisted_stream_and_final_metadata() {
    let folder = tempfile::tempdir().unwrap();
    let paths = Paths::new(folder.path().into());
    let store = Store::open(&paths.database()).unwrap();
    let stream = format!(
        "UNIQUE_EARLY_PREFIX!!\n{}END\n",
        "0123456789\n".repeat(12_000)
    );
    assert_eq!(stream.len(), 132026);
    let tail = &stream[stream.len() - 65536..];
    for (index, result) in [
        json!({"result":{"data":{"process":{"output":tail,"code":0,"cancelled":false}}}}),
        json!({"error":"FAILED_AFTER_STREAM"}),
        json!({"result":{"data":{"process":{"output":tail,"code":null,"cancelled":true}}}}),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("tool-{index}");
        let mut live = Transcript::default();
        let data = [
            EventData::ActionStarted {
                id: id.clone(),
                tool: "shell".into(),
                arguments: json!({"command":"fixture"}),
            },
            EventData::Delta {
                actor: id.clone(),
                text: stream.clone(),
            },
            EventData::ActionFinished {
                id: id.clone(),
                success: index == 0,
                result: result.clone(),
            },
        ];
        for (sequence, data) in data.into_iter().enumerate() {
            live.receive(&SessionEvent {
                session: id.clone(),
                sequence: sequence as u64 + 1,
                timestamp: "2026-10-22T00:00:00Z".into(),
                data: data.clone(),
            });
            store.append(&id, data).unwrap();
        }
        for transcript in [live, load(&paths, &id).unwrap()] {
            let entry = &transcript.entries()[0];
            assert_eq!(entry.body, stream);
            assert_eq!(entry.result.as_ref(), Some(&result));
            let copy = selected_text(&transcript, &id).unwrap();
            assert!(copy.starts_with(&stream));
            assert!(copy.contains("最终结果"));
            if index == 1 {
                assert!(copy.contains("FAILED_AFTER_STREAM"));
            }
            if index == 2 {
                assert!(copy.contains("\"cancelled\": true"));
            }
            assert!(
                entry.lines(80, false).len() < 20,
                "default display must stay compact"
            );
            let browser = Browser::new(transcript, "UNIQUE_EARLY_PREFIX".into(), true);
            assert_eq!(browser.copy().as_deref(), Some(copy.as_str()));
        }
    }
}

#[test]
fn quiet_and_nonstream_results_remain_searchable_with_metadata() {
    let mut transcript = Transcript::default();
    for (sequence, result) in [
        json!({"result":{"data":{"process":{"output":"","code":0}}}}),
        json!({"result":{"data":{"path":"large.txt","content":"visible file prefix","truncated":true}}}),
        json!({"error":"no stream failure"}),
        json!({"result":{"data":{"process":{"output":"nonstream\nraw\tbytes\u{1b}[31m","code":0}}}}),
    ].into_iter().enumerate() {
        transcript.receive(&SessionEvent { session:"fixture".into(), sequence:sequence as u64+1, timestamp:"2026-10-22T00:00:00Z".into(), data:EventData::ActionFinished { id:format!("quiet-{sequence}"), success:sequence != 2, result } });
    }
    assert!(
        selected_text(&transcript, "quiet-0")
            .unwrap()
            .contains("\"code\": 0")
    );
    assert!(
        selected_text(&transcript, "quiet-1")
            .unwrap()
            .contains("visible file prefix")
    );
    assert!(
        selected_text(&transcript, "quiet-2")
            .unwrap()
            .contains("no stream failure")
    );
    assert!(
        selected_text(&transcript, "quiet-3")
            .unwrap()
            .starts_with("nonstream\nraw\tbytes\u{1b}[31m")
    );
    assert!(
        transcript.entries()[1]
            .lines(80, false)
            .iter()
            .map(ToString::to_string)
            .collect::<String>()
            .contains("截断")
    );
    assert!(selected_text(&transcript, "quiet-").is_err());
}

#[test]
fn compact_results_keep_source_limits_separate_from_display_folding() {
    let cases = [
        (
            json!({"result":{"data":{"path":"large.txt","content":"first\nsecond","truncated":true}}}),
            "工具结果已截断",
        ),
        (
            json!({"result":{"data":{"process":{"output":"first\nsecond","truncated":true}}}}),
            "工具结果已截断",
        ),
        (
            json!({"data":{"output":"","truncated":true}}),
            "工具结果已截断",
        ),
        (
            json!({"data":{"matches":[{"path":"a","line":1,"text":"hit"}],"limit_reached":true,"truncated_files":["a"]}}),
            "搜索结果受限",
        ),
    ];
    for (index, (result, notice)) in cases.into_iter().enumerate() {
        let mut transcript = Transcript::default();
        transcript.receive(&SessionEvent {
            session: "limits".into(),
            sequence: 1,
            timestamp: String::new(),
            data: EventData::ActionFinished {
                id: index.to_string(),
                success: true,
                result: result.clone(),
            },
        });
        let entry = &transcript.entries()[0];
        let rows = entry.lines(80, false);
        let compact = rows
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            compact.contains(notice),
            "source limit hidden by compact summary: {compact}"
        );
        assert!(
            !compact.contains("second"),
            "compact view must omit raw body"
        );
        assert!(
            entry
                .full_text()
                .contains(&serde_json::to_string_pretty(&result).unwrap())
        );
        assert!(
            rows.len() < 10,
            "limit notice must keep the summary compact"
        );
        assert_eq!(entry.result.as_ref(), Some(&result));
    }
    let mut transcript = Transcript::default();
    transcript.receive(&SessionEvent {
        session: "complete".into(),
        sequence: 1,
        timestamp: String::new(),
        data: EventData::ActionFinished {
            id: "complete".into(),
            success: true,
            result: json!({"data":{"output":"first\nsecond\nthird"}}),
        },
    });
    let compact = transcript
        .lines(80, false)
        .iter()
        .map(ToString::to_string)
        .collect::<String>();
    assert!(!compact.contains("second"));
    assert!(transcript.entries()[0].full_text().contains("second"));
    assert!(
        !compact.contains("已截断"),
        "UI folding must not imply lost source output"
    );
}
