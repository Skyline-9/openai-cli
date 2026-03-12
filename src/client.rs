use std::io::{self, Write};
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 1000;

// --- errors ---

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),
    #[error("Server error: {0}")]
    Server(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Empty response: {0}")]
    EmptyResponse(String),
}

fn classify_status(status: reqwest::StatusCode, detail: String) -> ApiError {
    match status.as_u16() {
        401 => ApiError::Auth(detail),
        429 => ApiError::RateLimit(detail),
        400 | 404 | 422 => ApiError::InvalidRequest(detail),
        _ if status.is_server_error() => ApiError::Server(detail),
        _ => ApiError::Network(detail),
    }
}

fn is_retryable(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

#[derive(Deserialize)]
struct ApiErrorBody {
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

fn format_api_error(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<ApiErrorBody>(body)
        && let Some(detail) = parsed.error
    {
        let msg = detail.message.unwrap_or_else(|| "unknown".to_string());
        let typ = detail
            .error_type
            .map(|t| format!(" ({t})"))
            .unwrap_or_default();
        return format!("{msg}{typ}");
    }
    format!("HTTP {status}")
}

// --- models ---

#[derive(Clone, Debug, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Debug)]
struct ResponsesRequest {
    model: String,
    input: Vec<Message>,
    instructions: String,
    max_output_tokens: u32,
    temperature: f64,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Tool>,
}

#[derive(Serialize, Debug, Clone)]
struct Tool {
    #[serde(rename = "type")]
    tool_type: String,
}

#[derive(Serialize, Debug, Clone)]
struct ReasoningConfig {
    effort: String,
}

#[derive(Deserialize, Debug)]
struct SseEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<String>,
    text: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OutputFormat {
    Text,
    Json,
}

pub struct ClientConfig<'a> {
    pub api_key: &'a str,
    pub api_url: &'a str,
    pub model: &'a str,
    pub max_output_tokens: u32,
    pub temperature: f64,
    pub instructions: &'a str,
    pub format: OutputFormat,
    pub reasoning: Option<&'a str>,
    pub web_search: bool,
}

// --- core ---

pub async fn generate_response(
    prompt: &str,
    history: &[Message],
    config: &ClientConfig<'_>,
) -> Result<String, ApiError> {
    let mut input: Vec<Message> = history.to_vec();
    input.push(Message {
        role: "user".into(),
        content: prompt.into(),
    });

    let stream_mode = config.format == OutputFormat::Text;

    let payload = ResponsesRequest {
        model: config.model.into(),
        input,
        instructions: config.instructions.into(),
        max_output_tokens: config.max_output_tokens,
        temperature: config.temperature,
        stream: stream_mode,
        reasoning: config
            .reasoning
            .map(|e| ReasoningConfig { effort: e.into() }),
        tools: if config.web_search {
            vec![Tool {
                tool_type: "web_search_preview".into(),
            }]
        } else {
            vec![]
        },
    };

    debug!(
        model = config.model,
        stream = stream_mode,
        "sending request"
    );

    let client = reqwest::Client::new();
    let mut last_err = None;

    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            let backoff = INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1);
            warn!(attempt, max = MAX_RETRIES, backoff_ms = backoff, "retrying");
            tokio::time::sleep(Duration::from_millis(backoff)).await;
        }

        let response = match client
            .post(config.api_url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(ApiError::Network(format!("request failed: {e}")));
                continue;
            }
        };

        debug!(status = %response.status(), "received response");

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let err = classify_status(status, format_api_error(status, &body));

            if is_retryable(status) && attempt < MAX_RETRIES {
                warn!(%err, "retryable error");
                last_err = Some(err);
                continue;
            }
            return Err(err);
        }

        return if stream_mode {
            read_streaming_response(response).await
        } else {
            read_json_response(response).await
        };
    }

    Err(last_err.unwrap_or_else(|| ApiError::Network("all retry attempts exhausted".into())))
}

// --- response readers ---

async fn read_streaming_response(response: reqwest::Response) -> Result<String, ApiError> {
    let mut full_text = String::new();
    let mut buf = String::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::Network(format!("stream: {e}")))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buf.find("\n\n") {
            let block = buf[..pos].to_string();
            buf = buf[pos + 2..].to_string();

            let Some(data) = parse_sse_data(&block) else {
                continue;
            };

            if let Ok(evt) = serde_json::from_str::<SseEvent>(&data) {
                match evt.event_type.as_str() {
                    "response.output_text.delta" => {
                        if let Some(delta) = &evt.delta {
                            print!("{delta}");
                            let _ = io::stdout().flush();
                            full_text.push_str(delta);
                        }
                    }
                    "response.output_text.done" => {
                        if let Some(text) = &evt.text {
                            debug!(len = text.len(), "stream complete");
                            full_text = text.clone();
                        }
                    }
                    _ => debug!(event = evt.event_type, "ignored SSE event"),
                }
            }
        }
    }

    println!();

    if full_text.is_empty() {
        return Err(ApiError::EmptyResponse("no output text in stream".into()));
    }
    Ok(full_text.trim().to_string())
}

async fn read_json_response(response: reqwest::Response) -> Result<String, ApiError> {
    let body = response
        .text()
        .await
        .map_err(|e| ApiError::Network(format!("reading body: {e}")))?;

    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| ApiError::Parse(format!("{e}")))?;

    let text = extract_output_text(&parsed).unwrap_or_default();

    let output = serde_json::json!({ "text": text.trim(), "raw": parsed });
    let json_str =
        serde_json::to_string_pretty(&output).map_err(|e| ApiError::Parse(format!("{e}")))?;
    println!("{json_str}");

    if text.trim().is_empty() {
        return Err(ApiError::EmptyResponse("no output text in response".into()));
    }
    Ok(text.trim().to_string())
}

// --- helpers ---

fn parse_sse_data(block: &str) -> Option<String> {
    let data: String = block
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .collect();
    if data.is_empty() || data == "[DONE]" {
        None
    } else {
        Some(data)
    }
}

fn extract_output_text(parsed: &serde_json::Value) -> Option<String> {
    parsed["output"].as_array()?.iter().find_map(|item| {
        item["content"]
            .as_array()?
            .iter()
            .find_map(|c| c["text"].as_str().map(|s| s.to_string()))
    })
}

// --- tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_data_delta() {
        let block = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}";
        let data = parse_sse_data(block).unwrap();
        let evt: SseEvent = serde_json::from_str(&data).unwrap();
        assert_eq!(evt.event_type, "response.output_text.delta");
        assert_eq!(evt.delta.unwrap(), "Hello");
    }

    #[test]
    fn test_parse_sse_data_done() {
        let block = "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"text\":\"Hello world\"}";
        let data = parse_sse_data(block).unwrap();
        let evt: SseEvent = serde_json::from_str(&data).unwrap();
        assert_eq!(evt.event_type, "response.output_text.done");
        assert_eq!(evt.text.unwrap(), "Hello world");
    }

    #[test]
    fn test_parse_sse_data_empty() {
        assert!(parse_sse_data("event: response.created").is_none());
    }

    #[test]
    fn test_parse_sse_data_done_marker() {
        assert!(parse_sse_data("data: [DONE]").is_none());
    }

    #[test]
    fn test_classify_status_auth() {
        assert!(matches!(
            classify_status(reqwest::StatusCode::UNAUTHORIZED, "x".into()),
            ApiError::Auth(_)
        ));
    }

    #[test]
    fn test_classify_status_rate_limit() {
        assert!(matches!(
            classify_status(reqwest::StatusCode::TOO_MANY_REQUESTS, "x".into()),
            ApiError::RateLimit(_)
        ));
    }

    #[test]
    fn test_classify_status_bad_request() {
        assert!(matches!(
            classify_status(reqwest::StatusCode::BAD_REQUEST, "x".into()),
            ApiError::InvalidRequest(_)
        ));
    }

    #[test]
    fn test_classify_status_server() {
        assert!(matches!(
            classify_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "x".into()),
            ApiError::Server(_)
        ));
    }

    #[test]
    fn test_is_retryable() {
        assert!(is_retryable(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable(reqwest::StatusCode::BAD_GATEWAY));
        assert!(!is_retryable(reqwest::StatusCode::BAD_REQUEST));
        assert!(!is_retryable(reqwest::StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn test_format_api_error_structured() {
        let body = r#"{"error":{"message":"You exceeded your quota","type":"insufficient_quota"}}"#;
        let msg = format_api_error(reqwest::StatusCode::TOO_MANY_REQUESTS, body);
        assert_eq!(msg, "You exceeded your quota (insufficient_quota)");
    }

    #[test]
    fn test_format_api_error_unstructured() {
        let msg = format_api_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "garbage");
        assert_eq!(msg, "HTTP 500 Internal Server Error");
    }

    #[test]
    fn test_error_display() {
        let err = ApiError::Auth("bad key".into());
        assert_eq!(err.to_string(), "Authentication error: bad key");
    }

    #[test]
    fn test_extract_output_text() {
        let v = serde_json::json!({"output": [{"content": [{"text": "hello"}]}]});
        assert_eq!(extract_output_text(&v).unwrap(), "hello");
    }

    #[test]
    fn test_extract_output_text_missing() {
        let v = serde_json::json!({"output": []});
        assert!(extract_output_text(&v).is_none());
    }

    #[tokio::test]
    async fn test_generate_response_empty_key() {
        let cfg = ClientConfig {
            api_key: "",
            api_url: "http://unused",
            model: "test",
            max_output_tokens: 100,
            temperature: 0.5,
            instructions: "test",
            format: OutputFormat::Text,
            reasoning: None,
            web_search: false,
        };
        let result = generate_response("hi", &[], &cfg).await;
        assert!(result.is_err());
    }
}
