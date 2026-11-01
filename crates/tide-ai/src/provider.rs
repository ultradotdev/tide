//! Direct provider adapters with streaming.
//!
//! All four providers share one canonical event stream so the chat service and
//! UI do not need protocol-specific branches. Requests are built explicitly per
//! transport; responses are translated from SSE payloads.

use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroizing;

/// Longest SSE/NDJSON line accepted before the stream is treated as corrupt.
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Gemini,
    Ollama,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "google",
            Self::Ollama => "ollama",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            "google" | "gemini" => Some(Self::Gemini),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            Self::Ollama => "http://localhost:11434",
        }
    }

    pub fn requires_api_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatTurn {
    pub role: Role,
    pub text: String,
}

impl ChatTurn {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            text: text.into(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatRequest {
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub messages: Vec<ChatTurn>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Canonical tool definitions, transported in each provider's native shape.
    #[serde(default)]
    pub tools: Vec<ToolSchema>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    #[default]
    EndTurn,
    MaxTokens,
    ToolUse,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        id: String,
        model: String,
    },
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    MessageStop {
        stop_reason: StopReason,
    },
}

#[derive(Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub api_key: Option<Zeroizing<String>>,
    pub base_url: Option<String>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub connect_timeout: Duration,
    /// Longest wait for response headers or between two stream chunks. There
    /// is no total deadline, so long generations are never cut off.
    pub idle_timeout: Duration,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("kind", &self.kind)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("connect_timeout", &self.connect_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .finish()
    }
}

impl ProviderConfig {
    pub fn new(kind: ProviderKind, model: impl Into<String>) -> Self {
        Self {
            kind,
            api_key: None,
            base_url: None,
            model: model.into(),
            max_tokens: None,
            temperature: None,
            connect_timeout: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(300),
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(Zeroizing::new(api_key.into()));
        self
    }

    pub fn with_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    fn resolved_base_url(&self) -> &str {
        self.base_url
            .as_deref()
            .unwrap_or_else(|| self.kind.default_base_url())
    }

    fn api_key(&self) -> &str {
        self.api_key.as_deref().map_or("", String::as_str)
    }
}

pub struct Provider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl Provider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        if config.kind.requires_api_key()
            && config
                .api_key
                .as_deref()
                .is_none_or(|key| key.trim().is_empty())
        {
            return Err(anyhow!(
                "{} requires an API key (store it as a Tide secret or set the provider environment variable)",
                config.kind.as_str()
            ));
        }
        let client = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .build()
            .context("build HTTP client")?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    /// Stream a completion, calling `on_event` for each canonical event.
    ///
    /// Errors are flattened to one message with credentials redacted, since
    /// they are shown in the UI and persisted in the chat transcript.
    pub async fn stream<F>(&self, request: &ChatRequest, on_event: F) -> Result<StopReason>
    where
        F: FnMut(StreamEvent),
    {
        self.stream_inner(request, on_event).await.map_err(|error| {
            let known = self.config.api_key.as_deref().map(String::as_str);
            anyhow!(redact_secrets(&format!("{error:#}"), known.as_slice()))
        })
    }

    async fn stream_inner<F>(&self, request: &ChatRequest, mut on_event: F) -> Result<StopReason>
    where
        F: FnMut(StreamEvent),
    {
        let kind = self.config.kind.as_str();
        let idle = self.config.idle_timeout;
        let (request_builder, translate): (
            reqwest::RequestBuilder,
            fn(&Value) -> Vec<StreamEvent>,
        ) = match self.config.kind {
            ProviderKind::Anthropic => (self.anthropic_request(request), translate_anthropic),
            ProviderKind::OpenAi => (self.openai_request(request), translate_openai),
            ProviderKind::Gemini => (self.gemini_request(request), translate_gemini),
            ProviderKind::Ollama => (self.ollama_request(request), translate_ollama),
        };

        on_event(StreamEvent::MessageStart {
            id: String::new(),
            model: self.config.model.clone(),
        });

        let response = tokio::time::timeout(idle, request_builder.send())
            .await
            .map_err(|_| anyhow!("{kind} did not respond within {}s", idle.as_secs_f32()))?
            .map_err(reqwest::Error::without_url)
            .context("send provider request")?;
        let status = response.status();
        if !status.is_success() {
            let body = tokio::time::timeout(idle, response.text())
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let body = body.chars().take(500).collect::<String>();
            return Err(anyhow!("{kind} returned {status}: {body}"));
        }

        let mut stream = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut stop_reason = StopReason::EndTurn;
        loop {
            let chunk = match tokio::time::timeout(idle, stream.next()).await {
                Err(_) => {
                    return Err(anyhow!(
                        "{kind} stream stalled: no data for {}s",
                        idle.as_secs_f32()
                    ));
                }
                Ok(None) => break,
                Ok(Some(chunk)) => chunk
                    .map_err(reqwest::Error::without_url)
                    .context("read provider stream")?,
            };
            for payload in decoder.push(&chunk)? {
                if payload == "[DONE]" {
                    on_event(StreamEvent::MessageStop { stop_reason });
                    return Ok(stop_reason);
                }
                let value: Value = match serde_json::from_str(&payload) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if let Some(message) = stream_error(&value) {
                    return Err(anyhow!("{kind} stream error: {message}"));
                }
                for event in translate(&value) {
                    if let StreamEvent::MessageStop { stop_reason: stop } = &event {
                        stop_reason = *stop;
                    }
                    on_event(event);
                }
            }
        }
        on_event(StreamEvent::MessageStop { stop_reason });
        Ok(stop_reason)
    }

    fn anthropic_request(&self, request: &ChatRequest) -> reqwest::RequestBuilder {
        let mut body = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens.or(request.max_tokens).unwrap_or(4096),
            "stream": true,
            "messages": request.messages.iter().map(|turn| json!({
                "role": match turn.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": turn.text,
            })).collect::<Vec<_>>(),
        });
        if let Some(system) = request.system.as_deref().filter(|text| !text.is_empty()) {
            body["system"] = json!(system);
        }
        if let Some(temperature) = self.config.temperature.or(request.temperature) {
            body["temperature"] = json!(temperature);
        }
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.input_schema,
                        })
                    })
                    .collect(),
            );
        }
        let builder = self
            .client
            .post(format!("{}/v1/messages", self.config.resolved_base_url()))
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        with_secret_header(builder, "x-api-key", self.config.api_key())
    }

    fn openai_request(&self, request: &ChatRequest) -> reqwest::RequestBuilder {
        let mut messages = Vec::new();
        if let Some(system) = request.system.as_deref().filter(|text| !text.is_empty()) {
            messages.push(json!({ "role": "system", "content": system }));
        }
        for turn in &request.messages {
            messages.push(json!({
                "role": match turn.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": turn.text,
            }));
        }
        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(max_tokens) = self.config.max_tokens.or(request.max_tokens) {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(temperature) = self.config.temperature.or(request.temperature) {
            body["temperature"] = json!(temperature);
        }
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema,
                            }
                        })
                    })
                    .collect(),
            );
        }
        self.client
            .post(format!(
                "{}/chat/completions",
                self.config.resolved_base_url()
            ))
            .bearer_auth(self.config.api_key())
            .json(&body)
    }

    fn gemini_request(&self, request: &ChatRequest) -> reqwest::RequestBuilder {
        let mut body = json!({
            "contents": request.messages.iter().map(|turn| json!({
                "role": match turn.role { Role::User => "user", Role::Assistant => "model" },
                "parts": [{ "text": turn.text }],
            })).collect::<Vec<_>>(),
            "generationConfig": {},
        });
        if let Some(system) = request.system.as_deref().filter(|text| !text.is_empty()) {
            body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
        }
        if let Some(max_tokens) = self.config.max_tokens.or(request.max_tokens) {
            body["generationConfig"]["maxOutputTokens"] = json!(max_tokens);
        }
        if let Some(temperature) = self.config.temperature.or(request.temperature) {
            body["generationConfig"]["temperature"] = json!(temperature);
        }
        if !request.tools.is_empty() {
            body["tools"] = json!([{
                "functionDeclarations": request.tools.iter().map(|tool| json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                })).collect::<Vec<_>>()
            }]);
        }
        let builder = self
            .client
            .post(format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                self.config.resolved_base_url(),
                self.config.model,
            ))
            .json(&body);
        with_secret_header(builder, "x-goog-api-key", self.config.api_key())
    }

    fn ollama_request(&self, request: &ChatRequest) -> reqwest::RequestBuilder {
        let mut messages = Vec::new();
        if let Some(system) = request.system.as_deref().filter(|text| !text.is_empty()) {
            messages.push(json!({ "role": "system", "content": system }));
        }
        for turn in &request.messages {
            messages.push(json!({
                "role": match turn.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": turn.text,
            }));
        }
        let mut options = serde_json::Map::new();
        if let Some(max_tokens) = self.config.max_tokens.or(request.max_tokens) {
            options.insert("num_predict".to_owned(), json!(max_tokens));
        }
        if let Some(temperature) = self.config.temperature.or(request.temperature) {
            options.insert("temperature".to_owned(), json!(temperature));
        }
        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
            "stream": true,
        });
        if !options.is_empty() {
            body["options"] = Value::Object(options);
        }
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema,
                            }
                        })
                    })
                    .collect(),
            );
        }
        self.client
            .post(format!("{}/api/chat", self.config.resolved_base_url()))
            .json(&body)
    }
}

fn with_secret_header(
    builder: reqwest::RequestBuilder,
    name: &'static str,
    value: &str,
) -> reqwest::RequestBuilder {
    match reqwest::header::HeaderValue::from_str(value) {
        Ok(mut header) => {
            header.set_sensitive(true);
            builder.header(name, header)
        }
        Err(_) => builder.header(name, value),
    }
}

/// Error carried inside a stream: Anthropic `{"type":"error"}` events and the
/// `{"error":…}` objects OpenAI, Gemini, and Ollama emit mid-stream.
fn stream_error(value: &Value) -> Option<String> {
    let error = if value.get("type").and_then(Value::as_str) == Some("error") {
        value.get("error").unwrap_or(&Value::Null)
    } else {
        value.get("error").filter(|error| !error.is_null())?
    };
    Some(match error {
        Value::String(message) => message.clone(),
        Value::Object(_) => {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            match error
                .get("type")
                .or_else(|| error.get("status"))
                .and_then(Value::as_str)
            {
                Some(kind) => format!("{kind}: {message}"),
                None => message.to_owned(),
            }
        }
        _ => "unknown error".to_owned(),
    })
}

const REDACTED: &str = "[redacted]";

/// Remove credentials from text bound for the UI or a transcript: every known
/// secret value, plus any token following `key=` or `Bearer `.
pub fn redact_secrets(text: &str, known: &[&str]) -> String {
    let mut text = text.to_owned();
    for secret in known.iter().filter(|secret| secret.len() >= 4) {
        text = text.replace(secret, REDACTED);
    }
    for marker in ["key=", "bearer "] {
        text = redact_after(&text, marker);
    }
    text
}

fn redact_after(text: &str, marker: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find(marker) {
        let start = cursor + found + marker.len();
        let end = text[start..]
            .find(|ch: char| {
                ch.is_whitespace() || matches!(ch, '&' | '"' | '\'' | ')' | ',' | ';' | '<' | '>')
            })
            .map_or(text.len(), |offset| start + offset);
        out.push_str(&text[cursor..start]);
        if end > start {
            out.push_str(REDACTED);
        }
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Incremental SSE payload decoder with UTF-8 boundary handling.
#[derive(Default)]
pub struct SseDecoder {
    bytes: Vec<u8>,
    buffer: String,
}

impl SseDecoder {
    /// Feed a raw byte chunk; returns complete `data:` payloads. Invalid UTF-8
    /// becomes U+FFFD; a line longer than `MAX_LINE_BYTES` is an error.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>> {
        self.bytes.extend_from_slice(chunk);
        let mut consumed = 0;
        while consumed < self.bytes.len() {
            match std::str::from_utf8(&self.bytes[consumed..]) {
                Ok(text) => {
                    self.buffer.push_str(text);
                    consumed = self.bytes.len();
                }
                Err(error) => {
                    let valid = consumed + error.valid_up_to();
                    self.buffer
                        .push_str(&String::from_utf8_lossy(&self.bytes[consumed..valid]));
                    match error.error_len() {
                        Some(invalid) => {
                            self.buffer.push(char::REPLACEMENT_CHARACTER);
                            consumed = valid + invalid;
                        }
                        None => {
                            consumed = valid;
                            break;
                        }
                    }
                }
            }
        }
        self.bytes.drain(..consumed);
        let mut payloads = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            if index > MAX_LINE_BYTES {
                self.buffer.clear();
                return Err(anyhow!(
                    "provider stream line exceeded {MAX_LINE_BYTES} bytes"
                ));
            }
            let line = self.buffer[..index].trim_end_matches('\r').to_owned();
            self.buffer.drain(..=index);
            if let Some(data) = line.strip_prefix("data:") {
                payloads.push(data.trim_start().to_owned());
            } else if line.trim_start().starts_with('{') {
                // Ollama streams newline-delimited JSON rather than SSE frames.
                payloads.push(line.trim_start().to_owned());
            }
        }
        if self.buffer.len() > MAX_LINE_BYTES {
            self.buffer.clear();
            return Err(anyhow!(
                "provider stream line exceeded {MAX_LINE_BYTES} bytes"
            ));
        }
        Ok(payloads)
    }
}

pub fn translate_anthropic(value: &Value) -> Vec<StreamEvent> {
    match value.get("type").and_then(Value::as_str) {
        Some("content_block_delta") => match value.get("delta") {
            Some(delta) => match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => delta
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| {
                        vec![StreamEvent::TextDelta {
                            text: text.to_owned(),
                        }]
                    })
                    .unwrap_or_default(),
                Some("thinking_delta") => delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .map(|text| {
                        vec![StreamEvent::ReasoningDelta {
                            text: text.to_owned(),
                        }]
                    })
                    .unwrap_or_default(),
                Some("input_json_delta") => delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .map(|text| {
                        vec![StreamEvent::ToolUse {
                            id: String::new(),
                            name: String::new(),
                            input_json: text.to_owned(),
                        }]
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            },
            None => Vec::new(),
        },
        Some("content_block_start") => {
            let block = &value["content_block"];
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                vec![StreamEvent::ToolUse {
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    input_json: String::new(),
                }]
            } else {
                Vec::new()
            }
        }
        Some("message_delta") => {
            let mut events = Vec::new();
            if let Some(usage) = value.get("usage") {
                events.push(StreamEvent::Usage {
                    input_tokens: usage
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    output_tokens: usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                });
            }
            if let Some(reason) = value
                .get("delta")
                .and_then(|delta| delta.get("stop_reason"))
                .and_then(Value::as_str)
            {
                events.push(StreamEvent::MessageStop {
                    stop_reason: map_stop_reason(reason),
                });
            }
            events
        }
        _ => Vec::new(),
    }
}

pub fn translate_openai(value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        events.push(StreamEvent::Usage {
            input_tokens: usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        });
    }
    if let Some(choice) = value.get("choices").and_then(|c| c.get(0)) {
        let delta = &choice["delta"];
        if let Some(text) = delta.get("content").and_then(Value::as_str)
            && !text.is_empty()
        {
            events.push(StreamEvent::TextDelta {
                text: text.to_owned(),
            });
        }
        if let Some(text) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            && !text.is_empty()
        {
            events.push(StreamEvent::ReasoningDelta {
                text: text.to_owned(),
            });
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let function = &call["function"];
                if function.get("name").is_some() || function.get("arguments").is_some() {
                    events.push(StreamEvent::ToolUse {
                        id: call
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        name: function
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        input_json: function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    });
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            events.push(StreamEvent::MessageStop {
                stop_reason: map_stop_reason(reason),
            });
        }
    }
    events
}

pub fn translate_gemini(value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    if let Some(usage) = value.get("usageMetadata") {
        events.push(StreamEvent::Usage {
            input_tokens: usage
                .get("promptTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: usage
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        });
    }
    if let Some(candidate) = value.get("candidates").and_then(|c| c.get(0)) {
        if let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                let thought = part
                    .get("thought")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if let Some(text) = part.get("text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    events.push(if thought {
                        StreamEvent::ReasoningDelta {
                            text: text.to_owned(),
                        }
                    } else {
                        StreamEvent::TextDelta {
                            text: text.to_owned(),
                        }
                    });
                }
                if let Some(call) = part.get("functionCall") {
                    events.push(StreamEvent::ToolUse {
                        id: String::new(),
                        name: call
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        input_json: call
                            .get("args")
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    });
                }
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            events.push(StreamEvent::MessageStop {
                stop_reason: map_stop_reason(reason),
            });
        }
    }
    events
}

pub fn translate_ollama(value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    let message = &value["message"];
    if let Some(text) = message.get("content").and_then(Value::as_str)
        && !text.is_empty()
    {
        events.push(StreamEvent::TextDelta {
            text: text.to_owned(),
        });
    }
    if let Some(text) = message.get("thinking").and_then(Value::as_str)
        && !text.is_empty()
    {
        events.push(StreamEvent::ReasoningDelta {
            text: text.to_owned(),
        });
    }
    if value.get("done").and_then(Value::as_bool).unwrap_or(false) {
        events.push(StreamEvent::Usage {
            input_tokens: value
                .get("prompt_eval_count")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: value.get("eval_count").and_then(Value::as_u64).unwrap_or(0),
        });
        events.push(StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        });
    }
    events
}

fn map_stop_reason(reason: &str) -> StopReason {
    match reason {
        "max_tokens" | "length" | "MAX_TOKENS" => StopReason::MaxTokens,
        "tool_use" | "tool_calls" | "function_call" => StopReason::ToolUse,
        "cancelled" => StopReason::Cancelled,
        "error" => StopReason::Error,
        _ => StopReason::EndTurn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serve_once(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let request = read_request(&mut socket);
            socket.write_all(response.as_bytes()).unwrap();
            socket.flush().unwrap();
            request
        });
        (address, handle)
    }

    fn read_request(socket: &mut std::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = socket.read(&mut chunk).unwrap_or(0);
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(header_end) = find_header_end(&buffer) {
                let headers = String::from_utf8_lossy(&buffer[..header_end]).to_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buffer.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buffer).into_owned()
    }

    /// Serve a close-delimited event stream, sleeping before each chunk.
    fn serve_paced(chunks: Vec<(Duration, &'static str)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_request(&mut socket);
            let head =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
            if socket.write_all(head.as_bytes()).is_err() {
                return;
            }
            for (delay, chunk) in chunks {
                std::thread::sleep(delay);
                if socket.write_all(chunk.as_bytes()).is_err() {
                    return;
                }
                let _ = socket.flush();
            }
        });
        address
    }

    fn run_stream(
        config: ProviderConfig,
        request: &ChatRequest,
    ) -> (Vec<StreamEvent>, Result<StopReason>) {
        let provider = Provider::new(config).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut events = Vec::new();
        let result = runtime.block_on(provider.stream(request, |event| events.push(event)));
        (events, result)
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn sse(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn collect(
        response: &'static str,
        config: ProviderConfig,
        request: ChatRequest,
    ) -> (Vec<StreamEvent>, String) {
        let (address, handle) = serve_once(response);
        let config = config.with_base_url(address);
        let provider = Provider::new(config).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut events = Vec::new();
        runtime.block_on(async {
            provider
                .stream(&request, |event| events.push(event))
                .await
                .unwrap();
        });
        (events, handle.join().unwrap())
    }

    #[test]
    fn anthropic_stream_is_translated() {
        let body = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_file\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}\n\n",
        );
        let response: &'static str = Box::leak(sse(body).into_boxed_str());
        let (events, raw) = collect(
            response,
            ProviderConfig::new(ProviderKind::Anthropic, "claude-test").with_api_key("key"),
            ChatRequest {
                messages: vec![ChatTurn::user("hi")],
                ..ChatRequest::default()
            },
        );
        assert!(raw.to_lowercase().contains("x-api-key: key"));
        assert!(raw.contains("\"model\":\"claude-test\""));
        assert!(events.contains(&StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }));
        assert!(events.contains(&StreamEvent::ReasoningDelta {
            text: "hmm".to_owned()
        }));
        assert!(events.iter().any(
            |event| matches!(event, StreamEvent::ToolUse { name, .. } if name == "read_file")
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::Usage {
                input_tokens: 7,
                output_tokens: 3
            }
        )));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::MessageStop {
                stop_reason: StopReason::MaxTokens
            })
        ));
    }

    #[test]
    fn openai_stream_is_translated() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        );
        let response: &'static str = Box::leak(sse(body).into_boxed_str());
        let (events, raw) = collect(
            response,
            ProviderConfig::new(ProviderKind::OpenAi, "gpt-test").with_api_key("key"),
            ChatRequest {
                messages: vec![ChatTurn::user("hi")],
                ..ChatRequest::default()
            },
        );
        assert!(raw.to_lowercase().contains("authorization: bearer key"));
        assert!(raw.contains("\"stream\":true"));
        assert!(events.contains(&StreamEvent::TextDelta {
            text: "Hel".to_owned()
        }));
        assert!(events.contains(&StreamEvent::TextDelta {
            text: "lo".to_owned()
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::Usage {
                input_tokens: 2,
                output_tokens: 2
            }
        )));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn
            })
        ));
    }

    #[test]
    fn gemini_stream_is_translated() {
        let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"think\",\"thought\":true},{\"text\":\"answer\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":4,\"candidatesTokenCount\":6}}\n\n";
        let response: &'static str = Box::leak(sse(body).into_boxed_str());
        let (events, raw) = collect(
            response,
            ProviderConfig::new(ProviderKind::Gemini, "gemini-test").with_api_key("key"),
            ChatRequest {
                messages: vec![ChatTurn::user("hi")],
                ..ChatRequest::default()
            },
        );
        assert!(raw.contains("streamGenerateContent?alt=sse"));
        assert!(raw.to_lowercase().contains("x-goog-api-key: key"));
        assert!(!raw.contains("key=key"));
        assert!(events.contains(&StreamEvent::ReasoningDelta {
            text: "think".to_owned()
        }));
        assert!(events.contains(&StreamEvent::TextDelta {
            text: "answer".to_owned()
        }));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn
            })
        ));
    }

    #[test]
    fn ollama_stream_is_translated() {
        let body = concat!(
            "{\"message\":{\"content\":\"local\"},\"done\":false}\n",
            "{\"message\":{\"content\":\"\"},\"done\":true,\"prompt_eval_count\":3,\"eval_count\":4}\n",
        );
        let response: &'static str = Box::leak(sse(body).into_boxed_str());
        let (events, raw) = collect(
            response,
            ProviderConfig::new(ProviderKind::Ollama, "llama3.2"),
            ChatRequest {
                messages: vec![ChatTurn::user("hi")],
                ..ChatRequest::default()
            },
        );
        assert!(raw.contains("/api/chat"));
        assert!(events.contains(&StreamEvent::TextDelta {
            text: "local".to_owned()
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::Usage {
                input_tokens: 3,
                output_tokens: 4
            }
        )));
    }

    #[test]
    fn sse_decoder_handles_split_utf8_and_chunk_boundaries() {
        let mut decoder = SseDecoder::default();
        let bytes = "data: {\"text\":\"héllo\"}\n\ndata: [DONE]\n\n".as_bytes();
        let (first, second) = bytes.split_at(12);
        assert!(decoder.push(first).unwrap().is_empty());
        let payloads = decoder.push(second).unwrap();
        assert!(payloads.iter().any(|payload| payload.contains("héllo")));
        assert!(payloads.contains(&"[DONE]".to_owned()));
    }

    #[test]
    fn arbitrary_stream_bytes_never_panic() {
        let mut decoder = SseDecoder::default();
        let mut state = 0x9e37_79b9_u32;
        for _ in 0..200 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let length = (state as usize % 96) + 1;
            let bytes = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    // Bits 16..24 of the generator state.
                    state.to_le_bytes()[2]
                })
                .collect::<Vec<_>>();
            let _ = decoder.push(&bytes);
        }
    }

    #[test]
    fn provider_errors_are_sanitized_to_status_and_body() {
        let response: &'static str = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 11\r\nConnection: close\r\n\r\nbad request";
        let (address, handle) = serve_once(response);
        let provider = Provider::new(
            ProviderConfig::new(ProviderKind::OpenAi, "gpt-test")
                .with_api_key("key")
                .with_base_url(address),
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(async {
                provider
                    .stream(&ChatRequest::default(), |_| {})
                    .await
                    .unwrap_err()
            })
            .to_string();
        handle.join().unwrap();
        assert!(error.contains("401"));
        assert!(error.contains("bad request"));
    }

    #[test]
    fn tool_schemas_are_translated_for_each_transport() {
        let schema = ToolSchema {
            name: "read_file".to_owned(),
            description: "Read a file".to_owned(),
            input_schema: json!({"type": "object"}),
        };
        let request = ChatRequest {
            messages: vec![ChatTurn::user("hi")],
            tools: vec![schema],
            ..ChatRequest::default()
        };
        let cases = [
            (
                ProviderKind::Anthropic,
                "data: {\"type\":\"message_stop\"}\n\n",
                "input_schema",
            ),
            (ProviderKind::OpenAi, "data: [DONE]\n\n", "parameters"),
            (ProviderKind::Gemini, "data: {}\n\n", "functionDeclarations"),
            (ProviderKind::Ollama, "{}\n", "parameters"),
        ];
        for (kind, body, expected) in cases {
            let response: &'static str = Box::leak(sse(body).into_boxed_str());
            let mut config = ProviderConfig::new(kind, "model");
            if kind.requires_api_key() {
                config = config.with_api_key("key");
            }
            let (events, raw) = collect(response, config, request.clone());
            assert!(
                raw.contains(expected),
                "{} request did not contain {expected}: {raw}",
                kind.as_str()
            );
            assert!(!events.is_empty());
        }
    }

    #[test]
    fn sse_decoder_replaces_invalid_utf8_instead_of_stalling() {
        let mut decoder = SseDecoder::default();
        let first = decoder.push(b"data: {\"text\":\"a\xffb\"}\n").unwrap();
        let second = decoder.push(b"data: [DONE]\n").unwrap();
        let all = [first, second].concat();
        assert_eq!(all, vec!["{\"text\":\"a\u{FFFD}b\"}", "[DONE]"]);
        assert!(decoder.bytes.is_empty());
        assert!(decoder.push(b"data: \xc3").unwrap().is_empty());
        assert_eq!(decoder.push(b"\xa9\n").unwrap(), vec!["é"]);
    }

    #[test]
    fn sse_decoder_bounds_an_unterminated_line() {
        let mut decoder = SseDecoder::default();
        let chunk = vec![b'a'; 1024 * 1024];
        let mut error = None;
        for _ in 0..16 {
            if let Err(overflow) = decoder.push(&chunk) {
                error = Some(overflow);
                break;
            }
        }
        assert!(error.unwrap().to_string().contains("exceeded"));
        assert!(decoder.buffer.len() <= MAX_LINE_BYTES);
    }

    #[test]
    fn sse_decoder_bounds_a_terminated_line_delivered_in_one_chunk() {
        let mut decoder = SseDecoder::default();
        let mut chunk = b"data: ".to_vec();
        chunk.resize(MAX_LINE_BYTES + 16, b'a');
        chunk.push(b'\n');
        let error = decoder.push(&chunk).unwrap_err();
        assert!(error.to_string().contains("exceeded"));
        assert!(decoder.buffer.is_empty());
    }

    #[test]
    fn gemini_key_is_not_leaked_when_the_connection_fails() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let key = "AIzaSy-test-gemini-secret-0123";
        let (_, result) = run_stream(
            ProviderConfig::new(ProviderKind::Gemini, "gemini-test")
                .with_api_key(key)
                .with_base_url(address),
            &ChatRequest::default(),
        );
        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("send provider request"), "{message}");
        assert!(!message.contains(key), "{message}");
        assert!(!message.contains("key="), "{message}");
    }

    #[test]
    fn redactor_strips_query_keys_bearer_tokens_and_known_values() {
        let text = "request for url (http://h/m:x?key=AIzaRAW&alt=sse): Authorization: Bearer sk-raw-token; echoed sk-known-value";
        let redacted = redact_secrets(text, &["sk-known-value", ""]);
        for secret in ["AIzaRAW", "sk-raw-token", "sk-known-value"] {
            assert!(!redacted.contains(secret), "{redacted}");
        }
        assert!(redacted.contains("key=[redacted]&alt=sse"));
        assert!(redacted.contains("Bearer [redacted];"));
        assert_eq!(redact_secrets("no secrets here", &[]), "no secrets here");
    }

    #[test]
    fn provider_config_debug_redacts_the_api_key() {
        let config = ProviderConfig::new(ProviderKind::OpenAi, "m").with_api_key("sk-debug-secret");
        let debug = format!("{config:?}");
        assert!(!debug.contains("sk-debug-secret"), "{debug}");
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn a_stream_longer_than_the_idle_timeout_succeeds_while_chunks_arrive() {
        let pace = Duration::from_millis(150);
        let address = serve_paced(
            (0..8)
                .map(|_| {
                    (
                        pace,
                        "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
                    )
                })
                .chain([(pace, "data: [DONE]\n\n")])
                .collect(),
        );
        let (events, result) = run_stream(
            ProviderConfig::new(ProviderKind::OpenAi, "gpt-test")
                .with_api_key("key")
                .with_base_url(address)
                .with_idle_timeout(Duration::from_millis(600)),
            &ChatRequest::default(),
        );
        assert_eq!(result.unwrap(), StopReason::EndTurn);
        let deltas = events
            .iter()
            .filter(|event| matches!(event, StreamEvent::TextDelta { .. }))
            .count();
        assert_eq!(deltas, 8);
    }

    #[test]
    fn a_stalled_stream_fails_after_the_idle_timeout() {
        let address = serve_paced(vec![
            (
                Duration::ZERO,
                "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
            ),
            (Duration::from_secs(5), "data: [DONE]\n\n"),
        ]);
        let started = std::time::Instant::now();
        let (events, result) = run_stream(
            ProviderConfig::new(ProviderKind::OpenAi, "gpt-test")
                .with_api_key("key")
                .with_base_url(address)
                .with_idle_timeout(Duration::from_millis(300)),
            &ChatRequest::default(),
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains("stalled"), "{message}");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(events.contains(&StreamEvent::TextDelta {
            text: "x".to_owned()
        }));
    }

    #[test]
    fn anthropic_error_events_surface_as_errors() {
        let body = concat!(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let response: &'static str = Box::leak(sse(body).into_boxed_str());
        let (address, handle) = serve_once(response);
        let (events, result) = run_stream(
            ProviderConfig::new(ProviderKind::Anthropic, "claude-test")
                .with_api_key("key")
                .with_base_url(address),
            &ChatRequest::default(),
        );
        handle.join().unwrap();
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("overloaded_error: Overloaded"),
            "{message}"
        );
        assert!(events.contains(&StreamEvent::TextDelta {
            text: "partial".to_owned()
        }));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, StreamEvent::MessageStop { .. }))
        );
    }

    #[test]
    fn mid_stream_error_objects_from_other_providers_are_errors() {
        assert_eq!(
            stream_error(
                &json!({"error": {"code": 429, "message": "quota", "status": "RESOURCE_EXHAUSTED"}})
            ),
            Some("RESOURCE_EXHAUSTED: quota".to_owned())
        );
        assert_eq!(
            stream_error(&json!({"error": "model not found"})),
            Some("model not found".to_owned())
        );
        assert_eq!(stream_error(&json!({"error": null, "choices": []})), None);
        assert_eq!(stream_error(&json!({"type": "content_block_delta"})), None);
    }

    #[test]
    fn missing_api_keys_are_rejected_before_network_access() {
        assert!(Provider::new(ProviderConfig::new(ProviderKind::OpenAi, "m")).is_err());
        assert!(Provider::new(ProviderConfig::new(ProviderKind::Ollama, "m")).is_ok());
    }
}
