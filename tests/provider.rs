use loh::model::{Context, DeepSeek, Provider, Request, StreamDecoder};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn terminal(output: Value) -> String {
    format!(
        "event: response.completed\ndata: {}\n\n",
        json!({
            "type":"response.completed", "response": {
                "status":"completed", "model":"deepseek-flash", "output":output,
                "usage":{"input_tokens":50,"output_tokens":20,"input_tokens_details":{"cached_tokens":10}}
            }
        })
    )
}

fn output() -> Value {
    json!([
        {"type":"reasoning","content":[{"type":"reasoning_text","text":"inspect first"}]},
        {"type":"function_call","call_id":"call1","name":"read_file","arguments":"{\"path\":\"你好.rs\"}"}
    ])
}

#[test]
fn fragmented_utf8_stream_yields_only_complete_valid_calls() {
    let stream = terminal(output());
    let mut decoder = StreamDecoder::default();
    for chunk in stream.as_bytes().chunks(3) {
        decoder.push(chunk).unwrap();
    }
    let response = decoder.finish().unwrap();
    assert_eq!(response.calls().unwrap()[0].arguments["path"], "你好.rs");
    assert_eq!(response.usage.tokens(), 70);
    assert_eq!(response.usage.cached, 10);
}

#[test]
fn partial_or_failed_stream_never_becomes_executable_output() {
    let mut decoder = StreamDecoder::default();
    decoder
        .push(b"data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\"}\n\n")
        .unwrap();
    assert!(decoder.finish().is_err());
    let mut decoder = StreamDecoder::default();
    let failed =
        json!({"type":"response.incomplete","response":{"status":"incomplete","output":output()}});
    let error = decoder.push(format!("data: {failed}\n\n").as_bytes());
    assert!(error.is_err() || decoder.finish().is_err());
}

#[test]
fn invalid_complete_tool_arguments_are_rejected() {
    let mut decoder = StreamDecoder::default();
    let raw =
        terminal(json!([{"type":"function_call","call_id":"x","name":"shell","arguments":"{"}]));
    match decoder.push(raw.as_bytes()) {
        Err(_) => (),
        Ok(_) => assert!(decoder.finish().and_then(|r| r.calls()).is_err()),
    }
}

#[test]
fn incomplete_or_invalid_tools_keep_reported_usage_without_becoming_executable() {
    let incomplete = json!({"type":"response.incomplete","response":{"status":"incomplete","model":"fixture","usage":{"input_tokens":120,"output_tokens":40,"input_tokens_details":{"cached_tokens":20}},"output":[{"type":"function_call","call_id":"partial","name":"shell","arguments":"{"}]}});
    let mut decoder = StreamDecoder::default();
    let error = decoder
        .push(format!("data: {incomplete}\n\n").as_bytes())
        .unwrap_err();
    let failure = error
        .downcast_ref::<loh::model::ReportedFailure>()
        .expect("reported usage remains available for settlement");
    assert_eq!(failure.usage.input, 120);
    assert_eq!(failure.usage.output, 40);
    assert_eq!(failure.usage.cached, 20);
    let mut decoder = StreamDecoder::default();
    let error = decoder
        .push(
            terminal(
                json!([{"type":"function_call","call_id":"bad","name":"shell","arguments":"{"}]),
            )
            .as_bytes(),
        )
        .unwrap_err();
    assert!(error.is::<loh::model::ReportedFailure>());
}

#[test]
fn repeated_call_ids_and_non_object_arguments_are_not_executable() {
    for calls in [
        json!([
            {"type":"function_call","call_id":"same","name":"shell","arguments":"{}"},
            {"type":"function_call","call_id":"same","name":"shell","arguments":"{}"}
        ]),
        json!([{ "type":"function_call","call_id":"call","name":"shell","arguments":"[1,2]" }]),
    ] {
        let mut decoder = StreamDecoder::default();
        let result = decoder
            .push(terminal(calls).as_bytes())
            .and_then(|_| decoder.finish());
        assert!(result.is_err());
    }
}

#[test]
fn compaction_waits_until_tool_results_are_recorded() {
    let mut context = Context::default();
    context.append(output().as_array().unwrap());
    let original = context.items.clone();
    assert!(context.compact("summary").is_err());
    assert_eq!(context.items, original);
    context.tool_result("call1", &json!({"content":"code"}));
    assert_eq!(context.items[0]["type"], "reasoning");
    context.compact("verified summary").unwrap();
    assert_eq!(context.items.len(), 1);
}

async fn server(body: String) -> (String, tokio::task::JoinHandle<Value>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        let (head, length) = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap();
                break (end + 4, length);
            }
        };
        while request.len() < head + length {
            let count = socket.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
        }
        let parsed = serde_json::from_slice(&request[head..head + length]).unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        parsed
    });
    (url, task)
}

fn request() -> Request {
    Request {
        instructions: "build and verify".into(),
        input: vec![json!({"role":"user","content":"hello"})],
        tools: vec![],
        max_output: 128,
    }
}

#[tokio::test]
async fn real_http_transport_preserves_history_and_extracts_usage() {
    let (url, server) = server(terminal(output())).await;
    let provider = DeepSeek::new(url, "deepseek-flash".into(), "test-only-key".into()).unwrap();
    let (events, _) = mpsc::unbounded_channel();
    let response = provider
        .respond(request(), CancellationToken::new(), events)
        .await
        .unwrap();
    assert_eq!(response.calls().unwrap().len(), 1);
    let sent = server.await.unwrap();
    assert_eq!(sent["model"], "deepseek-flash");
    assert_eq!(sent["reasoning"]["effort"], "high");
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["input"][0]["content"], "hello");
    assert!(sent.get("previous_response_id").is_none());
}

#[tokio::test]
async fn cancellation_interrupts_a_silent_provider() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = DeepSeek::new(
        format!("http://{}", listener.local_addr().unwrap()),
        "deepseek-flash".into(),
        "fake".into(),
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let child = cancel.clone();
    let (events, _) = mpsc::unbounded_channel();
    let call = tokio::spawn(async move { provider.respond(request(), child, events).await });
    let connection =
        tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept()).await;
    assert!(
        connection.is_ok(),
        "provider must actually start the request"
    );
    let _connection = connection.unwrap().unwrap();
    cancel.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), call)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().to_string().contains("cancel"));
}
