use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod helpers {
    use std::process::{Command, Stdio};

    pub fn run_cli(args: &[&str], env_vars: &[(&str, &str)]) -> (String, String, i32) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_openai"));
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("OPENAI_API_KEY")
            .env_remove("OPENAI_API_URL")
            .env_remove("RUST_LOG");
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
        let output = cmd.output().expect("failed to run binary");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);
        (stdout, stderr, code)
    }

    pub fn run_cli_with_stdin(
        args: &[&str],
        env_vars: &[(&str, &str)],
        stdin_data: &str,
    ) -> (String, String, i32) {
        use std::io::Write;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_openai"));
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("OPENAI_API_KEY")
            .env_remove("OPENAI_API_URL")
            .env_remove("RUST_LOG");
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("failed to run binary");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin_data.as_bytes())
            .unwrap();
        let output = child.wait_with_output().expect("failed to wait");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);
        (stdout, stderr, code)
    }
}

fn sse_response(text: &str) -> String {
    let delta_event = format!(
        "event: response.output_text.delta\n\
         data: {{\"type\":\"response.output_text.delta\",\"delta\":\"{text}\"}}\n\n"
    );
    let done_event = format!(
        "event: response.output_text.done\n\
         data: {{\"type\":\"response.output_text.done\",\"text\":\"{text}\"}}\n\n"
    );
    let completed_event = "event: response.completed\n\
         data: {\"type\":\"response.completed\"}\n\n";
    format!("{delta_event}{done_event}{completed_event}")
}

fn json_response(text: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "resp_test",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        }]
    })
}

fn mock_env_strs(server: &MockServer) -> [(&'static str, String); 2] {
    [
        ("OPENAI_API_KEY", "test-key-123".into()),
        ("OPENAI_API_URL", server.uri()),
    ]
}

// --- prompt input tests ---

#[tokio::test]
async fn test_complete_with_prompt_flag() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-key-123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("Hello from mock"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) = helpers::run_cli(&["complete", "-p", "test prompt"], &env_refs);

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("Hello from mock"), "stdout was: {stdout}");
}

#[tokio::test]
async fn test_complete_with_positional_prompt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("Positional works"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) =
        helpers::run_cli(&["complete", "test prompt positional"], &env_refs);

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("Positional works"), "stdout was: {stdout}");
}

#[tokio::test]
async fn test_complete_with_file_ref() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("File ref works"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let tmp = std::env::temp_dir().join("openai_cli_test_file_ref.txt");
    std::fs::write(&tmp, "prompt from file").unwrap();

    let at_path = format!("@{}", tmp.to_str().unwrap());
    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) = helpers::run_cli(&["complete", "-p", &at_path], &env_refs);

    std::fs::remove_file(&tmp).ok();

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("File ref works"), "stdout was: {stdout}");
}

#[tokio::test]
async fn test_complete_with_file_flag() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("File flag works"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let tmp = std::env::temp_dir().join("openai_cli_test_file_flag.txt");
    std::fs::write(&tmp, "prompt from --file").unwrap();

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) =
        helpers::run_cli(&["complete", "--file", tmp.to_str().unwrap()], &env_refs);

    std::fs::remove_file(&tmp).ok();

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("File flag works"), "stdout was: {stdout}");
}

#[tokio::test]
async fn test_complete_with_stdin_ref() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("Stdin works"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) =
        helpers::run_cli_with_stdin(&["complete", "-p", "@-"], &env_refs, "prompt from stdin");

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("Stdin works"), "stdout was: {stdout}");
}

// --- output format tests ---

#[tokio::test]
async fn test_complete_json_mode() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json_response("JSON output test")))
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) =
        helpers::run_cli(&["--json", "complete", "-p", "test prompt"], &env_refs);

    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("invalid JSON output");
    assert_eq!(parsed["text"], "JSON output test");
    assert!(parsed["raw"].is_object());
}

#[tokio::test]
async fn test_json_short_flag() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json_response("short flag")))
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) = helpers::run_cli(&["-j", "complete", "-p", "test"], &env_refs);

    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("invalid JSON output");
    assert_eq!(parsed["text"], "short flag");
}

// --- alias tests ---

#[tokio::test]
async fn test_ask_alias() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("Alias works"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) = helpers::run_cli(&["ask", "-p", "test"], &env_refs);

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("Alias works"), "stdout was: {stdout}");
}

// --- error tests ---

#[tokio::test]
async fn test_missing_api_key() {
    let (_, stderr, code) = helpers::run_cli(
        &["complete", "-p", "test"],
        &[("OPENAI_API_KEY", ""), ("OPENAI_API_URL", "http://unused")],
    );

    assert_ne!(code, 0);
    assert!(stderr.contains("OPENAI_API_KEY"), "stderr was: {stderr}");
}

#[tokio::test]
async fn test_stdin_fallback_empty() {
    // When stdin is piped but empty, an empty prompt is sent to the API
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("empty stdin"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, _stderr, code) =
        helpers::run_cli_with_stdin(&["complete"], &env_refs, "piped prompt");

    assert_eq!(code, 0);
    assert!(stdout.contains("empty stdin"), "stdout was: {stdout}");
}

#[tokio::test]
async fn test_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": {
                "message": "Incorrect API key provided",
                "type": "invalid_api_key"
            }
        })))
        .mount(&server)
        .await;

    let env = [
        ("OPENAI_API_KEY", "bad-key".to_string()),
        ("OPENAI_API_URL", server.uri()),
    ];
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (_, stderr, code) = helpers::run_cli(&["complete", "-p", "test"], &env_refs);

    assert_ne!(code, 0);
    assert!(
        stderr.contains("Authentication error"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("Incorrect API key provided"),
        "stderr was: {stderr}"
    );
}

#[tokio::test]
async fn test_rate_limit_retries() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "error": {
                "message": "Rate limit reached",
                "type": "rate_limit_exceeded"
            }
        })))
        .expect(4) // initial + 3 retries
        .mount(&server)
        .await;

    let env = [
        ("OPENAI_API_KEY", "test-key".to_string()),
        ("OPENAI_API_URL", server.uri()),
    ];
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (_, stderr, code) = helpers::run_cli(&["complete", "-p", "test"], &env_refs);

    assert_ne!(code, 0);
    assert!(stderr.contains("Rate limit"), "stderr was: {stderr}");
}

// --- verbose flag test ---

#[tokio::test]
async fn test_verbose_flag() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("verbose test"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let env = mock_env_strs(&server);
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (stdout, stderr, code) = helpers::run_cli(&["-v", "complete", "-p", "test"], &env_refs);

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("verbose test"), "stdout was: {stdout}");
    // verbose mode should produce debug output on stderr
    assert!(
        stderr.contains("sending request") || stderr.contains("DEBUG"),
        "stderr should have debug output: {stderr}"
    );
}

// --- token warning test ---

#[tokio::test]
async fn test_token_warning() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response("token test"))
                .append_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let env = [("OPENAI_API_URL", server.uri())];
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (_, stderr, code) =
        helpers::run_cli(&["-t", "inline-key", "complete", "-p", "test"], &env_refs);

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        stderr.contains("visible in the process list"),
        "stderr was: {stderr}"
    );
}
