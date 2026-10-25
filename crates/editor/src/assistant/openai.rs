//! OpenAI's Responses API, the assistant's second provider (docs/editor.md, The
//! assistant). Threads are kept in one form, the Anthropic Messages shape the view
//! reads; this module turns a thread into Responses input, and a streamed response back
//! into that shape, with OpenAI's own output items kept beside the blocks (an `openai`
//! block) so they go back verbatim on the next request: reasoning items, encrypted as
//! the API returns them when nothing is stored on its side, must accompany the function
//! calls they led to.

use super::api::{self, Delta, Message, Settings};
use serde_json::{Value, json};

/// The Responses endpoint.
pub const URL: &str = "https://api.openai.com/v1/responses";

/// The request body: the system prompt as instructions, the tools as functions, the
/// thread as input items, streamed, nothing stored on OpenAI's side.
pub fn body(settings: &Settings, system: &str, tools: &[Value], messages: &[Value]) -> Value {
    let tools: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "name": t["name"],
                "description": t["description"],
                "parameters": t["input_schema"],
                // The editor's schemas have optional fields, which strict mode refuses;
                // the dispatch validates every call instead.
                "strict": false,
            })
        })
        .collect();
    json!({
        "model": settings.model(),
        "instructions": system,
        "input": input(messages),
        "tools": tools,
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "reasoning": { "effort": settings.effort, "summary": "auto" },
        "max_output_tokens": api::MAX_TOKENS,
        "parallel_tool_calls": true,
    })
}

/// A thread as Responses input items.
pub fn input(messages: &[Value]) -> Vec<Value> {
    let mut items = Vec::new();
    for message in messages {
        let role = message["role"].as_str().unwrap_or_default();
        let blocks: Vec<Value> = match &message["content"] {
            Value::String(s) => vec![json!({ "type": "text", "text": s })],
            Value::Array(blocks) => blocks.clone(),
            _ => Vec::new(),
        };
        match role {
            "user" => {
                let mut text = Vec::new();
                for block in &blocks {
                    match block["type"].as_str() {
                        Some("text") => text.push(json!({
                            "type": "input_text",
                            "text": block["text"],
                        })),
                        Some("tool_result") => items.push(call_output(block)),
                        _ => {}
                    }
                }
                if !text.is_empty() {
                    items.push(json!({ "role": "user", "content": text }));
                }
            }
            "assistant" => {
                // A turn OpenAI wrote goes back exactly as it came.
                if let Some(own) = blocks.iter().find(|b| b["type"] == "openai") {
                    items.extend(own["items"].as_array().cloned().unwrap_or_default());
                    continue;
                }
                // A turn another provider wrote: its text and its calls (its thinking
                // belongs to that provider and stays behind).
                for block in &blocks {
                    match block["type"].as_str() {
                        Some("text") => items.push(json!({
                            "role": "assistant",
                            "content": [{ "type": "output_text", "text": block["text"] }],
                        })),
                        Some("tool_use") => items.push(json!({
                            "type": "function_call",
                            "call_id": block["id"],
                            "name": block["name"],
                            "arguments": block["input"].to_string(),
                        })),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    items
}

/// A tool's answer as a `function_call_output`: a picture as an image, anything else as
/// text, a failure said so.
fn call_output(block: &Value) -> Value {
    let id = &block["tool_use_id"];
    let error = block["is_error"] == true;
    let output = match &block["content"] {
        Value::String(s) if error => Value::String(format!("Error: {s}")),
        Value::String(s) => Value::String(s.clone()),
        Value::Array(parts) => {
            let converted: Vec<Value> = parts
                .iter()
                .filter_map(|p| match p["type"].as_str() {
                    Some("image") => Some(json!({
                        "type": "input_image",
                        "image_url": format!(
                            "data:{};base64,{}",
                            p["source"]["media_type"].as_str().unwrap_or("image/png"),
                            p["source"]["data"].as_str().unwrap_or_default()
                        ),
                    })),
                    Some("text") => Some(json!({ "type": "input_text", "text": p["text"] })),
                    _ => None,
                })
                .collect();
            Value::Array(converted)
        }
        other => Value::String(other.to_string()),
    };
    json!({ "type": "function_call_output", "call_id": id, "output": output })
}

/// A streamed response being read.
#[derive(Debug, Default)]
pub struct Stream {
    /// Output items as they finished, in order (the completed response's list wins).
    done: Vec<Value>,
    finished: Option<Value>,
    incomplete: Option<String>,
}

impl Stream {
    pub fn feed(&mut self, event: &Value) -> Result<Option<Delta>, String> {
        match event["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => {
                let text = event["delta"].as_str().unwrap_or_default();
                return Ok(Some(Delta::Text(text.to_string())));
            }
            "response.reasoning_summary_text.delta" => {
                let text = event["delta"].as_str().unwrap_or_default();
                if !text.is_empty() {
                    return Ok(Some(Delta::Note(text.to_string())));
                }
            }
            "response.output_item.added" => {
                if event["item"]["type"] == "function_call" {
                    let name = event["item"]["name"].as_str().unwrap_or_default();
                    return Ok(Some(Delta::Tool(name.to_string())));
                }
            }
            "response.output_item.done" => self.done.push(event["item"].clone()),
            "response.completed" => self.finished = Some(event["response"].clone()),
            "response.incomplete" => {
                let reason = event["response"]["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("incomplete");
                self.incomplete = Some(reason.to_string());
                self.finished = Some(event["response"].clone());
            }
            "response.failed" => {
                let message = event["response"]["error"]["message"]
                    .as_str()
                    .unwrap_or("the response failed");
                return Err(message.to_string());
            }
            "error" => {
                let message = event["message"]
                    .as_str()
                    .or_else(|| event["error"]["message"].as_str())
                    .unwrap_or("the API reported an error");
                return Err(message.to_string());
            }
            _ => {}
        }
        Ok(None)
    }

    /// The response in the thread's form, once it has completed.
    pub fn finish(self) -> Option<Message> {
        let response = self.finished?;
        let output = response["output"]
            .as_array()
            .filter(|o| !o.is_empty())
            .cloned()
            .unwrap_or(self.done);
        let mut message = Message::default();
        let mut calls = false;
        for item in &output {
            match item["type"].as_str() {
                Some("message") => {
                    let text: String = item["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|c| c["type"] == "output_text")
                        .filter_map(|c| c["text"].as_str())
                        .collect();
                    if !text.is_empty() {
                        message
                            .content
                            .push(json!({ "type": "text", "text": text }));
                    }
                }
                Some("reasoning") => {
                    let summary: Vec<&str> = item["summary"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| s["text"].as_str())
                        .collect();
                    if !summary.is_empty() {
                        message
                            .content
                            .push(json!({ "type": "thinking", "thinking": summary.join("\n\n") }));
                    }
                }
                Some("function_call") => {
                    calls = true;
                    let arguments = item["arguments"].as_str().unwrap_or("{}");
                    let input = serde_json::from_str::<Value>(if arguments.trim().is_empty() {
                        "{}"
                    } else {
                        arguments
                    })
                    .unwrap_or_else(|_| json!({ "__invalid_json": arguments }));
                    message.content.push(json!({
                        "type": "tool_use",
                        "id": item["call_id"],
                        "name": item["name"],
                        "input": input,
                    }));
                }
                _ => {}
            }
        }
        // OpenAI's own items, for the next request.
        message
            .content
            .push(json!({ "type": "openai", "items": output }));
        message.stop_reason = Some(
            match self.incomplete.as_deref() {
                Some("max_output_tokens") => "max_tokens",
                Some(_) if !calls => "end_turn",
                _ if calls => "tool_use",
                _ => "end_turn",
            }
            .to_string(),
        );
        let usage = &response["usage"];
        let cached = usage["input_tokens_details"]["cached_tokens"]
            .as_u64()
            .unwrap_or(0);
        message.usage = [
            usage["input_tokens"]
                .as_u64()
                .unwrap_or(0)
                .saturating_sub(cached),
            usage["output_tokens"].as_u64().unwrap_or(0),
            cached,
            0,
        ];
        message.model = response["model"].as_str().map(str::to_string);
        Some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_becomes_input_items_and_openai_turns_go_back_verbatim() {
        let reasoning =
            json!({"type":"reasoning","id":"rs_1","encrypted_content":"enc","summary":[]});
        let call = json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"status","arguments":"{}"});
        let messages = vec![
            json!({"role":"user","content":[{"type":"text","text":"<editor-context>\nEditor: Maps\n</editor-context>"},{"type":"text","text":"Check it"}]}),
            json!({"role":"assistant","content":[
                {"type":"tool_use","id":"call_1","name":"status","input":{}},
                {"type":"openai","items":[reasoning, call]}
            ]}),
            json!({"role":"user","content":[
                {"type":"tool_result","tool_use_id":"call_1","content":"{\"ok\":true}"},
            ]}),
            // A turn Claude wrote earlier in the same thread.
            json!({"role":"assistant","content":[
                {"type":"thinking","thinking":"…","signature":"s"},
                {"type":"text","text":"Fine."},
                {"type":"tool_use","id":"toolu_9","name":"map_picture","input":{"map":"a"}}
            ]}),
            json!({"role":"user","content":[
                {"type":"tool_result","tool_use_id":"toolu_9","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}}]},
            ]}),
        ];
        let items = input(&messages);
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[0]["content"][1]["text"], "Check it");
        assert_eq!(items[1]["encrypted_content"], "enc");
        assert_eq!(items[2]["call_id"], "call_1");
        assert_eq!(
            items[3],
            json!({"type":"function_call_output","call_id":"call_1","output":"{\"ok\":true}"})
        );
        assert_eq!(items[4]["content"][0]["text"], "Fine.");
        assert_eq!(items[5]["call_id"], "toolu_9");
        assert_eq!(
            items[6]["output"][0]["image_url"],
            "data:image/png;base64,AAAA"
        );
        assert_eq!(items.len(), 7);
    }

    #[test]
    fn a_streamed_response_reads_back_as_text_calls_and_its_own_items() {
        let mut stream = Stream::default();
        let events = [
            json!({"type":"response.created","response":{"id":"r"}}),
            json!({"type":"response.reasoning_summary_text.delta","delta":"Looking at the map."}),
            json!({"type":"response.output_item.added","item":{"type":"function_call","name":"map_info"}}),
            json!({"type":"response.function_call_arguments.delta","delta":"{\"map\":"}),
            json!({"type":"response.output_text.delta","delta":"Let me look."}),
            json!({"type":"response.completed","response":{"model":"gpt-6-astra","output":[
                {"type":"reasoning","id":"rs","encrypted_content":"e","summary":[{"type":"summary_text","text":"Looking at the map."}]},
                {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Let me look."}]},
                {"type":"function_call","call_id":"call_7","name":"map_info","arguments":"{\"map\":\"frozen-dream\"}"}
            ],"usage":{"input_tokens":1000,"output_tokens":50,"input_tokens_details":{"cached_tokens":800}}}}),
        ];
        let mut deltas = Vec::new();
        for e in &events {
            if let Some(d) = stream.feed(e).unwrap() {
                deltas.push(d);
            }
        }
        assert!(deltas.contains(&Delta::Tool("map_info".into())));
        assert!(deltas.contains(&Delta::Note("Looking at the map.".into())));
        let message = stream.finish().expect("completed");
        assert_eq!(message.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(message.usage, [200, 50, 800, 0]);
        let calls = message.tool_calls();
        assert_eq!(calls[0].0, "call_7");
        assert_eq!(calls[0].2["map"], "frozen-dream");
        let turn = message.turn();
        assert_eq!(turn["content"][0]["type"], "thinking");
        assert_eq!(turn["content"][1]["text"], "Let me look.");
        assert_eq!(
            turn["content"].as_array().unwrap().last().unwrap()["type"],
            "openai"
        );
    }

    #[test]
    fn failures_and_length_limits_come_through() {
        let mut stream = Stream::default();
        let failed = stream
            .feed(&json!({"type":"response.failed","response":{"error":{"message":"bad key"}}}));
        assert_eq!(failed, Err("bad key".into()));
        let mut stream = Stream::default();
        stream
            .feed(&json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"},"output":[]}}))
            .unwrap();
        assert_eq!(
            stream.finish().unwrap().stop_reason.as_deref(),
            Some("max_tokens")
        );
    }
}
