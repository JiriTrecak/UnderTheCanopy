//! The assistant's providers and the Claude Messages API over HTTPS (docs/editor.md, The
//! assistant; OpenAI's Responses API is `super::openai`). Rust has no
//! official Anthropic SDK, so this speaks the documented wire format: a streamed
//! `POST /v1/messages` whose server-sent events are folded back into the response's
//! content blocks. Blocks come back exactly as the API sent them (text, thinking with its
//! signature, tool calls, fallback markers) and go into the history unchanged, so the
//! conversation stays append-only.

use serde_json::{Value, json};

/// The Messages endpoint.
pub const URL: &str = "https://api.anthropic.com/v1/messages";
/// The API version every request names.
pub const VERSION: &str = "2026-10-21";
/// Betas: refusals retried on Anthropic's recommended model (`fallbacks: "default"`), and
/// short progress notes between tool calls (`thinking.display: "updates"`).
pub const BETAS: &str = "server-side-fallback-2026-07-01,thinking-display-updates-2026-08-18";
/// Room for a turn's output (a stream, so no request timeout to fear).
pub const MAX_TOKENS: u32 = 64_000;

/// A model offered: id, label, and dollars per million input, output and cache-read
/// tokens.
pub type Model = (&'static str, &'static str, [f64; 3]);

/// Anthropic's models, the default first.
pub const MODELS: [Model; 2] = [
    ("claude-opus-5-5", "Claude Opus 5.5", [4.0, 20.0, 0.20]),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5", [2.0, 10.0, 0.20]),
];

/// OpenAI's, the default first (its cache reads at a tenth of the input rate).
pub const OPENAI_MODELS: [Model; 2] = [
    ("gpt-6-astra", "GPT-6 Astra", [10.0, 50.0, 1.0]),
    ("gpt-6.1-sol", "GPT-6.1 Sol", [2.0, 10.0, 0.20]),
];

/// How hard the model thinks: effort levels both providers take.
pub const EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];

/// Who answers: Anthropic's Claude (the default) or OpenAI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provider {
    #[default]
    Anthropic,
    OpenAi,
}

impl Provider {
    pub const ALL: [Self; 2] = [Self::Anthropic, Self::OpenAi];

    pub fn id(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::OpenAi => "OpenAI",
        }
    }

    pub fn models(self) -> &'static [Model] {
        match self {
            Self::Anthropic => &MODELS,
            Self::OpenAi => &OPENAI_MODELS,
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Self::Anthropic => URL,
            Self::OpenAi => super::openai::URL,
        }
    }

    /// The environment variable a key may come from.
    pub fn key_variable(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
        }
    }

    /// The file in the assistant's folder that keeps an entered key.
    pub fn key_file(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic-key",
            Self::OpenAi => "openai-key",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Anthropic => 0,
            Self::OpenAi => 1,
        }
    }
}

/// One request's settings: the provider, the model chosen for each provider, the effort.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub provider: Provider,
    pub models: [String; 2],
    pub effort: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            models: [MODELS[0].0.to_string(), OPENAI_MODELS[0].0.to_string()],
            effort: "high".to_string(),
        }
    }
}

impl Settings {
    /// The model the provider answers with.
    pub fn model(&self) -> &str {
        &self.models[self.provider.index()]
    }

    pub fn set_model(&mut self, provider: Provider, model: &str) {
        self.models[provider.index()] = model.to_string();
    }

    pub fn model_of(&self, provider: Provider) -> &str {
        &self.models[provider.index()]
    }

    /// The chosen model's label (its id when it is not one offered).
    pub fn model_label(&self) -> String {
        let id = self.model();
        self.provider
            .models()
            .iter()
            .find(|m| m.0 == id)
            .map_or(id.to_string(), |m| m.1.to_string())
    }
}

/// A thread's messages as Anthropic takes them: OpenAI's own items stay behind.
pub fn messages(thread: &[Value]) -> Vec<Value> {
    thread
        .iter()
        .filter_map(|m| {
            let mut m = m.clone();
            if let Some(blocks) = m["content"].as_array_mut() {
                blocks.retain(|b| b["type"] != "openai");
                if blocks.is_empty() {
                    return None;
                }
            }
            Some(m)
        })
        .collect()
}

/// The request body: a stream with the stable system prompt and tools first (the cache
/// reuses them), the whole history, and top-level caching that moves its breakpoint to
/// the latest turn.
pub fn body(settings: &Settings, system: &str, tools: &[Value], messages: &[Value]) -> Value {
    json!({
        "model": settings.model(),
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": [{ "type": "text", "text": system }],
        "tools": tools,
        "messages": self::messages(messages),
        "thinking": { "type": "adaptive", "display": "updates" },
        "output_config": { "effort": settings.effort },
        "fallbacks": "default",
        "cache_control": { "type": "ephemeral" },
    })
}

/// Splits a byte stream into server-sent events' `data` payloads.
#[derive(Debug, Default)]
pub struct Sse {
    buffer: String,
}

impl Sse {
    /// Takes the next chunk and returns the events it completed, parsed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Value> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        // Events end with a blank line; `\r\n` endings are normalised first.
        if self.buffer.contains('\r') {
            self.buffer = self.buffer.replace("\r\n", "\n");
        }
        let mut out = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let event: String = self.buffer.drain(..end + 2).collect();
            let data: Vec<&str> = event
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(str::trim_start)
                .collect();
            if data.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str(&data.join("\n")) {
                out.push(value);
            }
        }
        out
    }
}

/// What a streamed event added, for the view to show as it arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum Delta {
    Text(String),
    /// A progress note (a `thinking` block's summary between tool calls).
    Note(String),
    /// A tool call began.
    Tool(String),
}

/// The response being assembled from its events.
#[derive(Debug, Default, Clone)]
pub struct Message {
    pub content: Vec<Value>,
    /// The partial JSON of each tool call's input, by block index.
    inputs: Vec<Option<String>>,
    pub stop_reason: Option<String>,
    /// Token counts: input, output, cache reads, cache writes.
    pub usage: [u64; 4],
    /// The model that answered (a fallback may have taken over).
    pub model: Option<String>,
}

impl Message {
    /// Folds one event in. An `error` event fails the whole message.
    pub fn feed(&mut self, event: &Value) -> Result<Option<Delta>, String> {
        let kind = event["type"].as_str().unwrap_or_default();
        match kind {
            "message_start" => {
                self.model = event["message"]["model"].as_str().map(str::to_string);
                self.usage_from(&event["message"]["usage"]);
            }
            "content_block_start" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                let mut block = event["content_block"].clone();
                let tool = block["type"] == "tool_use";
                if tool {
                    block["input"] = json!({});
                }
                while self.content.len() <= index {
                    self.content.push(Value::Null);
                    self.inputs.push(None);
                }
                self.content[index] = block;
                if tool {
                    self.inputs[index] = Some(String::new());
                    let name = self.content[index]["name"].as_str().unwrap_or_default();
                    return Ok(Some(Delta::Tool(name.to_string())));
                }
            }
            "content_block_delta" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                let delta = &event["delta"];
                let Some(block) = self.content.get_mut(index) else {
                    return Ok(None);
                };
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or_default();
                        append(block, "text", text);
                        return Ok(Some(Delta::Text(text.to_string())));
                    }
                    "thinking_delta" => {
                        let text = delta["thinking"].as_str().unwrap_or_default();
                        append(block, "thinking", text);
                        if !text.is_empty() {
                            return Ok(Some(Delta::Note(text.to_string())));
                        }
                    }
                    "signature_delta" => {
                        block["signature"] = delta["signature"].clone();
                    }
                    "input_json_delta" => {
                        if let Some(Some(partial)) = self.inputs.get_mut(index) {
                            partial.push_str(delta["partial_json"].as_str().unwrap_or_default());
                        }
                    }
                    // Other deltas (citations, …) are kept with their block as they come.
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                if let (Some(Some(partial)), Some(block)) =
                    (self.inputs.get(index).cloned(), self.content.get_mut(index))
                {
                    let text = if partial.trim().is_empty() {
                        "{}"
                    } else {
                        partial.as_str()
                    };
                    match serde_json::from_str::<Value>(text) {
                        Ok(input) => block["input"] = input,
                        // Kept for the tool's answer: the call is refused, not run.
                        Err(_) => block["input"] = json!({ "__invalid_json": partial }),
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(reason.to_string());
                }
                self.usage_from(&event["usage"]);
            }
            "error" => {
                let message = event["error"]["message"]
                    .as_str()
                    .unwrap_or("the API reported an error");
                return Err(message.to_string());
            }
            _ => {}
        }
        Ok(None)
    }

    fn usage_from(&mut self, usage: &Value) {
        for (i, key) in [
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ]
        .iter()
        .enumerate()
        {
            if let Some(n) = usage[key].as_u64() {
                self.usage[i] = n;
            }
        }
    }

    /// The assistant turn for the history: every block as it arrived.
    pub fn turn(&self) -> Value {
        let content: Vec<Value> = self
            .content
            .iter()
            .filter(|b| !b.is_null())
            .cloned()
            .collect();
        json!({ "role": "assistant", "content": content })
    }

    /// The tool calls to run: id, name and input.
    pub fn tool_calls(&self) -> Vec<(String, String, Value)> {
        self.content
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| {
                (
                    b["id"].as_str().unwrap_or_default().to_string(),
                    b["name"].as_str().unwrap_or_default().to_string(),
                    b["input"].clone(),
                )
            })
            .collect()
    }
}

fn append(block: &mut Value, field: &str, text: &str) {
    let current = block[field].as_str().unwrap_or_default().to_string();
    block[field] = Value::String(current + text);
}

/// What `usage` cost in dollars for `model`.
pub fn cost(model: &str, usage: [u64; 4]) -> f64 {
    let rates = MODELS
        .iter()
        .chain(OPENAI_MODELS.iter())
        .find(|(id, _, _)| model.starts_with(id))
        .map(|(_, _, r)| *r)
        .unwrap_or(MODELS[0].2);
    // Cache writes (5-minute) cost 1.25 times the input rate.
    (usage[0] as f64 * rates[0]
        + usage[1] as f64 * rates[1]
        + usage[2] as f64 * rates[2]
        + usage[3] as f64 * rates[0] * 1.25)
        / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(events: &[Value]) -> Vec<u8> {
        let mut out = String::new();
        for e in events {
            out.push_str(&format!(
                "event: {}\ndata: {}\n\n",
                e["type"].as_str().unwrap(),
                e
            ));
        }
        out.into_bytes()
    }

    #[test]
    fn a_streamed_turn_folds_into_its_blocks_in_any_chunking() {
        let events = [
            json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":120,"cache_read_input_tokens":4000}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Reading the unit."}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Raising "}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"its health."}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"edit","input":{}}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"label\": \"hp\", "}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"commands\": []}"}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":77}}),
            json!({"type":"message_stop"}),
        ];
        let bytes = stream(&events);
        for chunk in [1usize, 7, 64, bytes.len()] {
            let mut sse = Sse::default();
            let mut message = Message::default();
            let mut deltas = Vec::new();
            for piece in bytes.chunks(chunk) {
                for event in sse.push(piece) {
                    if let Some(d) = message.feed(&event).unwrap() {
                        deltas.push(d);
                    }
                }
            }
            let turn = message.turn();
            assert_eq!(turn["content"][0]["thinking"], "Reading the unit.");
            assert_eq!(turn["content"][0]["signature"], "sig");
            assert_eq!(turn["content"][1]["text"], "Raising its health.");
            assert_eq!(
                turn["content"][2]["input"],
                json!({"label":"hp","commands":[]})
            );
            assert_eq!(message.stop_reason.as_deref(), Some("tool_use"));
            assert_eq!(message.usage, [120, 77, 4000, 0]);
            assert_eq!(message.tool_calls()[0].1, "edit");
            assert!(deltas.contains(&Delta::Tool("edit".into())));
            assert!(deltas.contains(&Delta::Note("Reading the unit.".into())));
        }
    }

    #[test]
    fn broken_tool_input_is_kept_for_the_answer_and_errors_fail_the_turn() {
        let mut message = Message::default();
        message
            .feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t","name":"edit","input":{}}}))
            .unwrap();
        message
            .feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"label\": "}}))
            .unwrap();
        message
            .feed(&json!({"type":"content_block_stop","index":0}))
            .unwrap();
        assert!(message.content[0]["input"]["__invalid_json"].is_string());
        let error = message.feed(
            &json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}),
        );
        assert_eq!(error, Err("Overloaded".into()));
    }

    #[test]
    fn a_thread_openai_wrote_part_of_reaches_anthropic_without_its_items() {
        let thread = [
            json!({"role":"user","content":"hi"}),
            json!({"role":"assistant","content":[
                {"type":"text","text":"Hello."},
                {"type":"openai","items":[{"type":"reasoning","encrypted_content":"x"}]}
            ]}),
            json!({"role":"assistant","content":[{"type":"openai","items":[]}]}),
        ];
        let sent = body(&Settings::default(), "s", &[], &thread)["messages"].clone();
        assert_eq!(sent.as_array().unwrap().len(), 2);
        assert_eq!(sent[1]["content"], json!([{"type":"text","text":"Hello."}]));
    }

    #[test]
    fn the_body_puts_stable_parts_first_and_asks_for_fallbacks() {
        let body = body(
            &Settings::default(),
            "system",
            &[json!({"name":"status"})],
            &[],
        );
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body.get("temperature").is_none());
        let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        let at = |k: &str| keys.iter().position(|x| *x == k).unwrap();
        assert!(at("system") < at("messages") && at("tools") < at("messages"));
    }
}
