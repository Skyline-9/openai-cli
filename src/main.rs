mod client;
mod config;

use std::io::{self, IsTerminal, Read, Write};

use clap::{Parser, Subcommand};
use tracing::debug;

use client::{ApiError, ClientConfig, Message, OutputFormat, generate_response};
use config::{DEFAULT_MODEL, MAX_OUTPUT_TOKENS, SYSTEM_MESSAGE, TEMPERATURE};

#[derive(Parser)]
#[command(
    name = "openai",
    version,
    about = "CLI for interacting with OpenAI's Responses API."
)]
struct Cli {
    /// OpenAI model to use
    #[arg(short, long, default_value = DEFAULT_MODEL)]
    model: String,

    /// Maximum number of tokens in the response
    #[arg(short = 'k', long = "max-tokens", default_value_t = MAX_OUTPUT_TOKENS)]
    max_tokens: u32,

    /// Temperature for response generation
    #[arg(long, default_value_t = TEMPERATURE)]
    temperature: f64,

    /// OpenAI API token (overrides OPENAI_API_KEY; visible in process list)
    #[arg(short, long)]
    token: Option<String>,

    /// Output raw JSON instead of streaming text
    #[arg(long, short = 'j')]
    json: bool,

    /// Reasoning effort for reasoning models (low, medium, high)
    #[arg(short, long, value_name = "EFFORT")]
    reasoning: Option<String>,

    /// System prompt / instructions (overrides default)
    #[arg(short, long, value_name = "PROMPT")]
    system: Option<String>,

    /// Disable web search (enabled by default)
    #[arg(long = "no-web")]
    no_web: bool,

    /// Enable verbose/debug logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a prompt and get a completion.
    #[command(visible_alias = "ask", alias = "a")]
    Complete {
        /// Prompt text, or @<file>/@- for file/stdin
        #[arg(short, long)]
        prompt: Option<String>,

        /// Read prompt from a file
        #[arg(short, long, value_name = "PATH")]
        file: Option<String>,

        /// Open $EDITOR to compose the prompt
        #[arg(short, long)]
        editor: bool,

        /// Copy the response to the clipboard
        #[arg(short, long)]
        copy: bool,

        /// Positional prompt (alternative to -p)
        #[arg(value_name = "PROMPT")]
        positional_prompt: Option<String>,
    },
    /// Start interactive REPL with conversation history.
    #[command(visible_alias = "chat", alias = "r")]
    Repl,
}

fn read_stdin() -> Result<String, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("reading stdin: {e}"))?;
    Ok(buf)
}

fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))
}

/// Precedence: -p > positional > --file > --editor > stdin (pipe)
fn resolve_prompt(
    flag: Option<String>,
    positional: Option<String>,
    file: Option<String>,
    use_editor: bool,
) -> Result<String, String> {
    if let Some(raw) = flag.or(positional) {
        return match raw.strip_prefix('@') {
            Some("-") => read_stdin(),
            Some(path) => read_file(path),
            None => Ok(raw),
        };
    }
    if let Some(path) = file {
        return read_file(&path);
    }
    if use_editor {
        return open_editor();
    }
    if !io::stdin().is_terminal() {
        return read_stdin();
    }
    Err(
        "no prompt provided; use -p \"...\", pass as argument, --file, --editor, or pipe stdin"
            .into(),
    )
}

fn open_editor() -> Result<String, String> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    let tmp = std::env::temp_dir().join("openai-cli-prompt.txt");
    std::fs::write(&tmp, "").map_err(|e| format!("creating temp file: {e}"))?;

    let status = std::process::Command::new(&editor)
        .arg(&tmp)
        .status()
        .map_err(|e| format!("launching {editor}: {e}"))?;

    if !status.success() {
        return Err(format!("{editor} exited with {status}"));
    }

    let content = std::fs::read_to_string(&tmp).map_err(|e| format!("reading temp file: {e}"))?;
    std::fs::remove_file(&tmp).ok();

    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        return Err("editor prompt was empty".into());
    }
    Ok(trimmed)
}

fn copy_to_clipboard(text: &str) {
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
        Ok(()) => eprintln!("(copied to clipboard)"),
        Err(e) => eprintln!("clipboard error: {e}"),
    }
}

fn die(err: impl std::fmt::Display) -> ! {
    eprintln!("{err}");
    std::process::exit(1);
}

async fn run_complete(
    prompt: &str,
    history: &mut Vec<Message>,
    config: &ClientConfig<'_>,
    copy: bool,
) -> Result<(), ApiError> {
    let result = generate_response(prompt, history, config).await?;
    if copy {
        copy_to_clipboard(&result);
    }
    history.push(Message {
        role: "user".into(),
        content: prompt.into(),
    });
    history.push(Message {
        role: "assistant".into(),
        content: result,
    });
    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let filter = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .with_target(false)
        .without_time()
        .with_writer(io::stderr)
        .init();

    if cli.token.is_some() {
        eprintln!("warning: --token is visible in the process list; prefer OPENAI_API_KEY env var");
    }

    let api_key = config::resolve_api_key(cli.token.as_deref()).unwrap_or_else(|| {
        die("Authentication error: OPENAI_API_KEY is not set and no --token provided")
    });
    let api_url = config::resolve_api_url();
    debug!(api_url, "resolved API endpoint");

    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    };

    let config = ClientConfig {
        api_key: &api_key,
        api_url: &api_url,
        model: &cli.model,
        max_output_tokens: cli.max_tokens,
        temperature: cli.temperature,
        instructions: cli.system.as_deref().unwrap_or(SYSTEM_MESSAGE),
        format,
        reasoning: cli.reasoning.as_deref(),
        web_search: !cli.no_web,
    };

    let mut history: Vec<Message> = Vec::new();

    match cli.command {
        Commands::Complete {
            prompt,
            positional_prompt,
            file,
            editor,
            copy,
        } => {
            let prompt = resolve_prompt(prompt, positional_prompt, file, editor)
                .unwrap_or_else(|e| die(format!("Error: {e}")));

            if let Err(e) = run_complete(&prompt, &mut history, &config, copy).await {
                die(e);
            }
        }
        Commands::Repl => {
            println!("Interactive shell started. Using model: {}", cli.model);
            println!(
                "Max tokens: {}, Temperature: {}",
                cli.max_tokens, cli.temperature
            );
            println!("Type 'exit' or use Ctrl-D to exit.");

            loop {
                print!("\nPrompt: ");
                if io::stdout().flush().is_err() {
                    break;
                }

                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }

                let prompt = input.trim();
                if prompt.is_empty() {
                    continue;
                }
                if prompt.eq_ignore_ascii_case("exit") {
                    break;
                }

                println!();
                if let Err(e) = run_complete(prompt, &mut history, &config, false).await {
                    eprintln!("{e}");
                }
            }
            println!("Interactive shell ended.");
        }
    }
}
