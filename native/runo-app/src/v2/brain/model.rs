//! The small model on the person's own computer, for when Smart Peek is on.
//!
//! Squad Code doesn't link a model runtime in. It talks to one the person
//! runs, over the OpenAI-style chat API that llama.cpp's `llama-server`,
//! Ollama and LM Studio all serve on localhost. Squad's own model (a small
//! open model fine-tuned on Squad's jobs, see `training/`) is served the same
//! way, so swapping models is a setting, not a rebuild.
//!
//! Every answer is asked for as JSON with a fixed shape and checked before
//! use: an answer that doesn't parse, names a bot that isn't on the crew, or
//! takes longer than [`TIMEOUT_SECS`] is thrown away and the rules decide.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::settings::AppSettings;

/// Where `llama-server` listens by default (`llama-server --port 8088`).
pub const DEFAULT_URL: &str = "http://127.0.0.1:8088/v1";
/// A local model that can't answer in this long isn't worth waiting for.
pub const TIMEOUT_SECS: u32 = 4;

/// A model to ask: where it's served and its name there.
#[derive(Clone, Debug)]
pub struct Local {
    pub url: String,
    pub model: String,
}

impl Local {
    /// The person's local model, when Smart Peek is on.
    pub fn from_settings(st: &AppSettings) -> Option<Local> {
        if st.brain_model.trim().is_empty() {
            return None;
        }
        let url = if st.brain_url.trim().is_empty() {
            DEFAULT_URL.to_string()
        } else {
            st.brain_url.trim().trim_end_matches('/').to_string()
        };
        // Settings saved before the rename still name Squad's own model "fin".
        let model = if super::helper::is_ours(&st.brain_model) && st.brain_url.trim().is_empty() {
            super::helper::MODEL.to_string()
        } else {
            st.brain_model.trim().to_string()
        };
        Some(Local { url, model })
    }

    /// One chat turn, asked for as JSON matching `schema`; the reply's text.
    pub fn chat(&self, system: &str, user: &str, schema: Option<Value>) -> Option<String> {
        let mut body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "temperature": 0.1,
            "max_tokens": 160,
            "stream": false,
            // Qwen3.5's chat template thinks first unless told not to, and the
            // thinking eats the whole answer budget before any JSON comes out.
            // llama-server passes these to the template; others ignore them.
            "chat_template_kwargs": {"enable_thinking": false}
        });
        if let Some(schema) = schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {"name": "answer", "schema": schema, "strict": true}
            });
        }
        let url = format!("{}/chat/completions", self.url);
        let mut child = Command::new("curl")
            .args(["-sS", "-m", &TIMEOUT_SECS.to_string()])
            .args([
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                "@-",
            ])
            .arg(&url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(body.to_string().as_bytes());
        }
        let out = child.wait_with_output().ok()?;
        if !out.status.success() {
            return None;
        }
        let v: Value = serde_json::from_slice(&out.stdout).ok()?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
    }

    /// Whether anything answers at the address.
    pub fn alive(&self) -> bool {
        Command::new("curl")
            .args(["-sS", "-m", "2", "-w", "%{http_code}"])
            .args(["-o", if cfg!(windows) { "NUL" } else { "/dev/null" }])
            .arg(format!("{}/models", self.url))
            .output()
            .ok()
            .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "200")
    }
}
