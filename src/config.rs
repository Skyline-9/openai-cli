use std::env;

pub const DEFAULT_MODEL: &str = "gpt-5.2";
pub const MAX_OUTPUT_TOKENS: u32 = 16384;
pub const TEMPERATURE: f64 = 0.23;
pub const SYSTEM_MESSAGE: &str = "You are a helpful assistant.";
pub const DEFAULT_API_URL: &str = "https://api.openai.com/v1/responses";

pub fn resolve_api_key(cli_token: Option<&str>) -> Option<String> {
    cli_token
        .map(|s| s.to_string())
        .or_else(|| env::var("OPENAI_API_KEY").ok().filter(|s| !s.is_empty()))
}

pub fn resolve_api_url() -> String {
    env::var("OPENAI_API_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_API_URL.to_string())
}
