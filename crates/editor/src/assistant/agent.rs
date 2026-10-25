//! The assistant's loop on a thread of its own (docs/editor.md, The assistant): it sends
//! the thread to the Messages API, shows what streams back, runs the tool calls through
//! the editor's own tool channel (the one MCP uses, so every edit is an undo step on the
//! editor's thread), asks the user first for the tools that write files or spend money,
//! and goes round until the model ends its turn, the user stops it, or something fails.

use super::api::{self, Delta, Message, Settings, Sse};
use crate::mcp::{self, Call};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Rounds of tool calls one message may take before the loop stops and says so.
const MAX_ROUNDS: usize = 60;
/// The most of a tool's answer the model reads; the rest is cut with a note.
const MAX_RESULT: usize = 60_000;

/// Where requests go and with what key.
#[derive(Clone)]
pub struct Endpoint {
    pub url: String,
    pub key: String,
}

/// What the loop reports to the view.
pub enum Update {
    /// Streamed text, a progress note, or a tool call starting.
    Delta(Delta),
    /// A message for the thread's history: an assistant turn or the tool results.
    Turn(Value),
    /// A tool that writes files or spends money waits for the user's answer.
    Approve {
        name: String,
        input: Value,
        reply: mpsc::Sender<bool>,
    },
    /// A response's token counts and the model that answered.
    Usage { model: String, usage: [u64; 4] },
    /// The loop is over: `None` when the model finished, else why it stopped.
    Finished(Option<String>),
}

/// The tools offered: the editor's MCP tools, in the API's shape (inputs stream as they
/// are written; the dispatch validates them).
pub fn tools() -> Vec<Value> {
    mcp::definitions()
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description.unwrap_or_default(),
                "input_schema": Value::Object((*tool.input_schema).clone()),
                "eager_input_streaming": true,
            })
        })
        .collect()
}

/// Starts the loop on `messages` (the thread so far, ending with the user's message).
pub fn start(
    endpoint: Endpoint,
    settings: Settings,
    system: String,
    messages: Vec<Value>,
    calls: mpsc::Sender<Call>,
    updates: mpsc::Sender<Update>,
    stop: Arc<AtomicBool>,
) {
    let spawned = std::thread::Builder::new()
        .name("editor assistant".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = updates.send(Update::Finished(Some(error.to_string())));
                    return;
                }
            };
            let outcome = runtime.block_on(run(
                &endpoint, &settings, &system, messages, &calls, &updates, &stop,
            ));
            let _ = updates.send(Update::Finished(outcome.err()));
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "editor: assistant thread");
    }
}

fn client(endpoint: &Endpoint) -> Result<reqwest::Client, String> {
    let tls = canopy_net::tls::client_config().map_err(|e| e.to_string())?;
    // A local endpoint (the tests' stand-in for the API) may be plain HTTP.
    let local = endpoint.url.starts_with("http://127.0.0.1");
    reqwest::Client::builder()
        .use_preconfigured_tls((*tls).clone())
        .https_only(!local)
        .connect_timeout(Duration::from_secs(30))
        .user_agent(concat!("UnderTheCanopy-editor/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

async fn run(
    endpoint: &Endpoint,
    settings: &Settings,
    system: &str,
    mut messages: Vec<Value>,
    calls: &mpsc::Sender<Call>,
    updates: &mpsc::Sender<Update>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let client = client(endpoint)?;
    let tools = tools();
    for _ in 0..MAX_ROUNDS {
        let message = match stream(
            &client, endpoint, settings, system, &tools, &messages, updates, stop,
        )
        .await?
        {
            Some(message) => message,
            // Stopped while streaming: the partial turn is dropped.
            None => return Err("Stopped.".into()),
        };
        let turn = message.turn();
        messages.push(turn.clone());
        let _ = updates.send(Update::Turn(turn));
        let _ = updates.send(Update::Usage {
            model: message
                .model
                .clone()
                .unwrap_or_else(|| settings.model.clone()),
            usage: message.usage,
        });
        match message.stop_reason.as_deref() {
            Some("tool_use") => {}
            // A server tool paused a long turn: send it back to go on.
            Some("pause_turn") => continue,
            Some("refusal") => {
                return Err("The model declined this request (and no fallback took it on).".into());
            }
            Some("max_tokens") => return Err("The answer reached its length limit.".into()),
            _ => return Ok(()),
        }
        let mut results = Vec::new();
        for (id, name, input) in message.tool_calls() {
            if stop.load(Ordering::Relaxed) {
                results.push(result(
                    &id,
                    Err("Stopped by the user before it ran.".into()),
                ));
                continue;
            }
            let answer = run_tool(&name, input, calls, updates, stop).await;
            results.push(result(&id, answer));
        }
        let turn = json!({ "role": "user", "content": results });
        messages.push(turn.clone());
        let _ = updates.send(Update::Turn(turn));
        if stop.load(Ordering::Relaxed) {
            return Err("Stopped.".into());
        }
    }
    Err(format!(
        "Stopped after {MAX_ROUNDS} rounds of tool calls; say how to go on."
    ))
}

/// One streamed response, or `None` when the user stopped it.
#[allow(clippy::too_many_arguments)]
async fn stream(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    settings: &Settings,
    system: &str,
    tools: &[Value],
    messages: &[Value],
    updates: &mpsc::Sender<Update>,
    stop: &AtomicBool,
) -> Result<Option<Message>, String> {
    let body = api::body(settings, system, tools, messages);
    let mut response = client
        .post(&endpoint.url)
        .header("x-api-key", &endpoint.key)
        .header("anthropic-version", api::VERSION)
        .header("anthropic-beta", api::BETAS)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("Cannot reach the Claude API: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or(text);
        return Err(match status.as_u16() {
            401 => "The API key was refused (401). Check it in the assistant's settings.".into(),
            429 => format!("Rate limited (429): {message}"),
            529 => "The API is overloaded (529); try again shortly.".into(),
            code => format!("The API answered {code}: {message}"),
        });
    }
    let mut sse = Sse::default();
    let mut message = Message::default();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let chunk = tokio::time::timeout(Duration::from_secs(120), response.chunk())
            .await
            .map_err(|_| "The stream went silent for two minutes.".to_string())?
            .map_err(|e| format!("The stream broke: {e}"))?;
        let Some(chunk) = chunk else {
            break;
        };
        for event in sse.push(&chunk) {
            if let Some(delta) = message.feed(&event)? {
                let _ = updates.send(Update::Delta(delta));
            }
        }
    }
    if message.stop_reason.is_none() {
        return Err("The stream ended before the answer did.".into());
    }
    Ok(Some(message))
}

/// Runs one tool call on the editor's thread, asking the user first when it writes files
/// or spends money.
async fn run_tool(
    name: &str,
    input: Value,
    calls: &mpsc::Sender<Call>,
    updates: &mpsc::Sender<Update>,
    stop: &AtomicBool,
) -> Result<Value, String> {
    if let Some(raw) = input.get("__invalid_json") {
        return Err(format!(
            "The tool's input was not valid JSON (it may have been cut off): {}",
            raw.as_str()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    if mcp::needs_approval(name, &input) {
        let (reply, answer) = mpsc::channel();
        let _ = updates.send(Update::Approve {
            name: name.to_string(),
            input: input.clone(),
            reply,
        });
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err("Stopped by the user before it ran.".into());
            }
            match answer.try_recv() {
                Ok(true) => break,
                Ok(false) => return Err("The user declined this call.".into()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("The user declined this call.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }
    let tool = mcp::parse(name, input)?;
    let long = matches!(
        tool,
        mcp::Tool::Cook | mcp::Tool::Sound(_) | mcp::Tool::LabRun(_)
    );
    let (reply, answer) = tokio::sync::oneshot::channel();
    calls
        .send(Call { tool, reply })
        .map_err(|_| "The editor is closed.".to_string())?;
    let wait = Duration::from_secs(if long { 900 } else { 90 });
    match tokio::time::timeout(wait, answer).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(_)) => Err("The editor dropped the call.".into()),
        Err(_) => Err("The editor did not answer in time.".into()),
    }
}

/// A tool's answer as a `tool_result` block: a picture as an image, anything else as
/// JSON text (cut at [`MAX_RESULT`]), a failure marked as an error.
fn result(id: &str, answer: Result<Value, String>) -> Value {
    match answer {
        Ok(Value::Object(object)) if object.get("png").is_some_and(Value::is_string) => json!({
            "type": "tool_result",
            "tool_use_id": id,
            "content": [{
                "type": "image",
                "source": { "type": "base64", "media_type": "image/png", "data": object["png"] },
            }],
        }),
        Ok(value) => {
            let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
            if text.len() > MAX_RESULT {
                let mut cut = MAX_RESULT;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
                text.push_str("\n… (cut here: the answer is longer; ask for less, such as one editor's elements or one document)");
            }
            json!({ "type": "tool_result", "tool_use_id": id, "content": text })
        }
        Err(message) => json!({
            "type": "tool_result",
            "tool_use_id": id,
            "content": message,
            "is_error": true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A stand-in for the Messages API on loopback: answers each request with the next
    /// scripted stream, and keeps the bodies it received.
    fn fake_api(script: Vec<Vec<Value>>) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/v1/messages",
            listener.local_addr().unwrap().port()
        );
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for events in script {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                let mut request = Vec::new();
                let mut buf = [0u8; 65536];
                // Headers, then the body by its length.
                let (head_end, length) = loop {
                    let n = socket.read(&mut buf).unwrap();
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while request.len() < head_end + length {
                    let n = socket.read(&mut buf).unwrap();
                    request.extend_from_slice(&buf[..n]);
                }
                let body: Value =
                    serde_json::from_slice(&request[head_end..head_end + length]).unwrap();
                kept.lock().unwrap().push(body);
                let mut sse = String::new();
                for e in events {
                    sse.push_str(&format!(
                        "event: {}\ndata: {}\n\n",
                        e["type"].as_str().unwrap(),
                        e
                    ));
                }
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    sse.len(),
                    sse
                );
                socket.write_all(reply.as_bytes()).unwrap();
            }
        });
        (url, seen)
    }

    fn turn(blocks: Vec<Value>, stop: &str) -> Vec<Value> {
        let mut events = vec![
            json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":10}}}),
        ];
        for (i, block) in blocks.into_iter().enumerate() {
            let mut start = block.clone();
            if start["type"] == "text" {
                start["text"] = json!("");
            }
            if start["type"] == "tool_use" {
                start["input"] = json!({});
            }
            events.push(json!({"type":"content_block_start","index":i,"content_block":start}));
            if block["type"] == "text" {
                events.push(json!({"type":"content_block_delta","index":i,"delta":{"type":"text_delta","text":block["text"]}}));
            }
            if block["type"] == "tool_use" {
                events.push(json!({"type":"content_block_delta","index":i,"delta":{"type":"input_json_delta","partial_json":block["input"].to_string()}}));
            }
            events.push(json!({"type":"content_block_stop","index":i}));
        }
        events.push(json!({"type":"message_delta","delta":{"stop_reason":stop},"usage":{"output_tokens":5}}));
        events.push(json!({"type":"message_stop"}));
        events
    }

    #[test]
    fn the_loop_runs_tools_on_the_editor_thread_asks_before_saving_and_keeps_history_whole() {
        let (url, seen) = fake_api(vec![
            turn(
                vec![
                    json!({"type":"text","text":"Checking."}),
                    json!({"type":"tool_use","id":"t1","name":"status","input":{}}),
                    json!({"type":"tool_use","id":"t2","name":"save","input":{}}),
                ],
                "tool_use",
            ),
            turn(vec![json!({"type":"text","text":"Done."})], "end_turn"),
        ]);
        let (calls, editor) = mpsc::channel::<Call>();
        let (updates, view) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        start(
            Endpoint {
                url,
                key: "test-key".into(),
            },
            Settings::default(),
            "system".into(),
            vec![json!({"role":"user","content":"Save it"})],
            calls,
            updates,
            stop,
        );
        // The editor's side: answer the one tool call that runs (status); decline save.
        let mut turns = Vec::new();
        let mut ran = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let finished = loop {
            assert!(std::time::Instant::now() < deadline, "the loop finished");
            while let Ok(call) = editor.try_recv() {
                ran.push(format!("{:?}", call.tool));
                let _ = call.reply.send(Ok(json!({"unsaved": []})));
            }
            let Ok(update) = view.recv_timeout(Duration::from_millis(20)) else {
                continue;
            };
            match update {
                Update::Approve { name, reply, .. } => {
                    assert_eq!(name, "save");
                    reply.send(false).unwrap();
                }
                Update::Turn(t) => turns.push(t),
                Update::Finished(outcome) => break outcome,
                _ => {}
            }
        };
        assert_eq!(finished, None);
        assert_eq!(ran, vec!["Status".to_string()]);
        // assistant (two calls), user (both results), assistant (done)
        assert_eq!(turns.len(), 3);
        let results = &turns[1]["content"];
        assert_eq!(results[0]["tool_use_id"], "t1");
        assert_eq!(results[1]["is_error"], true);
        // The second request carried the whole history and the tools.
        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[1]["messages"].as_array().unwrap().len(), 3);
        assert!(
            bodies[1]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == "edit")
        );
    }

    #[test]
    fn every_tool_offered_is_one_the_dispatch_knows() {
        for tool in tools() {
            let name = tool["name"].as_str().unwrap();
            if let Err(error) = mcp::parse(name, json!({})) {
                assert!(!error.starts_with("No tool"), "{name}: {error}");
            }
            assert!(tool["input_schema"]["type"] == "object", "{name}");
        }
    }
}
