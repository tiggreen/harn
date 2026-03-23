use crate::analysis::system_prompt;
use crate::config::LlmProvider;
use crate::{boxed_error, AppResult};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisReport {
    pub harness_score: i64,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub generated_configs: Vec<GeneratedConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Finding {
    #[serde(default = "default_scope")]
    pub scope: String,
    pub title: String,
    pub severity: String,
    pub confidence: String,
    pub evidence: String,
    pub story: String,
    pub fix: String,
    pub impact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeneratedConfig {
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_target")]
    pub target: String,
    pub title: String,
    pub action: String,
    #[serde(default)]
    pub target_text: String,
    #[serde(default)]
    pub new_text: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub expected_impact: String,
}

fn default_scope() -> String {
    "project".to_string()
}

fn default_target() -> String {
    "agents_md".to_string()
}

pub fn analyze(
    provider: LlmProvider,
    api_key: &str,
    model: &str,
    stage1_payload: &Value,
) -> AppResult<AnalysisReport> {
    match provider {
        LlmProvider::Anthropic => analyze_anthropic(api_key, model, stage1_payload),
        LlmProvider::OpenAI => analyze_openai(api_key, model, stage1_payload),
    }
}

fn analyze_anthropic(
    api_key: &str,
    model: &str,
    stage1_payload: &Value,
) -> AppResult<AnalysisReport> {
    let client = Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&json!({
            "model": model,
            "max_tokens": 4096,
            "temperature": 0.2,
            "system": system_prompt::SYSTEM_PROMPT,
            "messages": [{
                "role": "user",
                "content": format!(
                    "Analyze this harness data and return strict JSON only.\n\n{}",
                    serde_json::to_string_pretty(stage1_payload)?
                )
            }]
        }))
        .send()?;

    if !response.status().is_success() {
        return Err(boxed_error(format!(
            "Anthropic API returned {}",
            response.status()
        )));
    }

    let body: Value = response.json()?;
    let text = body
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    if text.trim().is_empty() {
        return Err(boxed_error("Anthropic API returned an empty response"));
    }

    let json_text = extract_json_object(&text)
        .ok_or_else(|| boxed_error("Anthropic API response did not contain a valid JSON object"))?;
    let report: AnalysisReport = serde_json::from_str(&json_text)?;
    Ok(report)
}

fn analyze_openai(
    api_key: &str,
    model: &str,
    stage1_payload: &Value,
) -> AppResult<AnalysisReport> {
    let client = Client::new();
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("content-type", "application/json")
        .json(&json!({
            "model": model,
            "max_tokens": 4096,
            "temperature": 0.2,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt::SYSTEM_PROMPT
                },
                {
                    "role": "user",
                    "content": format!(
                        "Analyze this harness data and return strict JSON only.\n\n{}",
                        serde_json::to_string_pretty(stage1_payload)?
                    )
                }
            ]
        }))
        .send()?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(boxed_error(format!(
            "OpenAI API returned {status}: {body}"
        )));
    }

    let body: Value = response.json()?;
    let text = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if text.trim().is_empty() {
        return Err(boxed_error("OpenAI API returned an empty response"));
    }

    let json_text = extract_json_object(&text)
        .ok_or_else(|| boxed_error("OpenAI API response did not contain a valid JSON object"))?;
    let report: AnalysisReport = serde_json::from_str(&json_text)?;
    Ok(report)
}

fn extract_json_object(text: &str) -> Option<String> {
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}')?;
    Some(cleaned[start..=end].to_string())
}
