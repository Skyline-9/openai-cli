<p align="center">
  <h1 align="center">openai-cli</h1>
  <p align="center">
    A fast, minimal command-line client for the OpenAI Responses API, written in Rust.
  </p>
</p>

<p align="center">
  <a href="https://github.com/Skyline-9/openai-cli/releases"><img src="https://img.shields.io/badge/version-0.3.1-blue" alt="Version"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/license-MIT-green" alt="License"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.80%2B-orange" alt="Rust"></a>
</p>

---

## Features

- **Direct prompts** -- inline text, files, stdin, or `$EDITOR`
- **Interactive REPL** -- multi-turn conversations with history
- **Streaming** -- tokens print as they arrive
- **JSON mode** -- machine-readable output with `--json` / `-j`
- **Clipboard** -- copy responses with `--copy` / `-c`
- **Retry logic** -- automatic exponential backoff on rate limits and server errors
- **Reasoning effort** -- `--reasoning` / `-r` for reasoning models (o3, o4-mini)
- **Web search** -- enabled by default; the model searches when helpful, disable with `--no-web`
- **System prompt** -- `--system` / `-s` to override default instructions per request
- **Structured logging** -- `--verbose` / `-v` or `RUST_LOG=debug` for tracing output
- **Single binary** -- no runtime dependencies

## Installation

### Pre-built binary

| Platform | Architecture | Download |
|----------|-------------|----------|
| macOS    | Apple Silicon (arm64) | [openai-darwin-arm64](https://github.com/Skyline-9/openai-cli/releases/download/Release/openai-darwin-arm64) |
| Linux    | x86-64 | [openai-linux-amd64](https://github.com/Skyline-9/openai-cli/releases/download/Release/openai-linux-amd64) |
| Windows  | x86-64 | [openai-windows-amd64.exe](https://github.com/Skyline-9/openai-cli/releases/download/Release/openai-windows-amd64.exe) |

```sh
# macOS
curl -Lo openai https://github.com/Skyline-9/openai-cli/releases/download/v0.3.1/openai-darwin-arm64
chmod +x openai && mv openai ~/.local/bin/

# Linux
curl -Lo openai https://github.com/Skyline-9/openai-cli/releases/download/v0.3.1/openai-linux-amd64
chmod +x openai && mv openai ~/.local/bin/
```

### From source

```sh
git clone https://github.com/Skyline-9/openai-cli.git
cd openai-cli
cargo build --release
ln -sf "$(pwd)/target/release/openai" ~/.local/bin/openai
```

Requires Rust 1.80+ and an [OpenAI API key](https://platform.openai.com/account/api-keys).

## Quick start

```sh
export OPENAI_API_KEY="sk-..."
```

### Inline prompt

```sh
openai ask -p "Explain quicksort in one sentence"
```

### From a file

```sh
openai complete --file prompt.txt
# or
openai complete -p @prompt.txt
```

### Heredoc

```sh
openai ask -p @- <<'PROMPT'
Explain quicksort in one sentence,
and then give a one-line pseudocode summary.
PROMPT
```

### Open your editor

```sh
openai ask --editor
```

### Interactive REPL

```sh
openai chat
```

## Usage

```
Usage: openai [OPTIONS] <COMMAND>

Commands:
  complete  Send a prompt and get a completion [aliases: ask]
  repl      Start interactive REPL with conversation history [aliases: chat]

Options:
  -m, --model <MODEL>              OpenAI model to use [default: gpt-5.2]
  -k, --max-tokens <MAX_TOKENS>    Maximum number of tokens in the response [default: 16384]
      --temperature <TEMPERATURE>  Temperature for response generation [default: 0.23]
  -t, --token <TOKEN>              OpenAI API token (overrides OPENAI_API_KEY)
  -j, --json                       Output raw JSON instead of streaming text
  -r, --reasoning <EFFORT>         Reasoning effort: low, medium, high (for o3/o4-mini)
  -s, --system <PROMPT>            System prompt / instructions (overrides default)
      --no-web                     Disable web search (enabled by default)
  -v, --verbose                    Enable verbose/debug logging
  -h, --help                       Print help
  -V, --version                    Print version
```

### Prompt input precedence

`-p` > positional arg > `--file` > `--editor` > stdin (pipe)

### Examples

```sh
# Different model
openai -m gpt-4o ask -p "Hello"

# High creativity
openai --temperature 0.9 ask -p "Write a haiku about Rust"

# JSON output piped to jq
openai -j ask -p "What is 2+2?" | jq .text

# Copy response to clipboard
openai ask -c -p "Draft a commit message for this diff"

# Custom system prompt
openai -s "You are a Python expert. Reply with code only." ask -p "fizzbuzz"

# Reasoning model with high effort
openai -m o4-mini -r high ask -p "Prove that sqrt(2) is irrational"

# Debug logging
openai -v ask -p "test"
```

## Configuration

| Environment Variable | Description                   | Default                                |
|----------------------|-------------------------------|----------------------------------------|
| `OPENAI_API_KEY`     | API key (required)            | --                                     |
| `OPENAI_API_URL`     | Override the API endpoint     | `https://api.openai.com/v1/responses`  |
| `RUST_LOG`           | Logging filter (e.g. `debug`) | `warn`                                 |
| `EDITOR` / `VISUAL`  | Editor for `--editor` flag    | `vi`                                   |

## CI

The project uses a GitHub Actions workflow that runs `cargo fmt --check`, `cargo clippy`, and `cargo test` on every push.

## License

[MIT](LICENSE)
