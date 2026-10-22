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
