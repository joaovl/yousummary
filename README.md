# youSummary
<img width="1158" height="564" alt="image" src="https://github.com/user-attachments/assets/66cfe134-c827-4de1-bf52-644082bf1ff1" />


**Make YouTube videos readable**

A modern, local-first CLI tool written in Rust to extract transcripts from YouTube and transform them into concise summaries and key points.

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org/)

## What is youSummary?

`yousummary` is a command-line tool that converts YouTube videos into clean Markdown summaries using transcripts and Large Language Models.

Give it a video URL, a playlist, or a file containing multiple URLs, and `yousummary` will fetch the transcript, summarize the content in manageable chunks, and generate a readable Markdown document with a summary and optional key points.

**Two-step workflow:** Use `fetch` to save video metadata and transcript as JSON, then `summarize` later with any LLM. This allows you to collect data on a lightweight machine and process it elsewhere.

By default, `yousummary` is **local-first and privacy-friendly** when used with Ollama, while still supporting cloud models when needed.

## Features

- **Fetch command**: Save video metadata and transcript as JSON for later processing
- Generate clean **Markdown summaries** with optional key points
- Summarize from URL, pre-fetched JSON, playlist, or batch of URLs
- Optimized for **Small Language Models (SLMs)**, which are often sufficient for high-quality summarization
- Works with **Ollama** (local) and cloud providers (OpenAI, Anthropic, Groq, Together, OpenRouter)
- **Multi-language summaries**: specify output language
- Handles **long videos** via transcript chunking with overlap
- Skips reprocessing when a summary already exists (use `--force` to regenerate)
- Built-in **web UI** for interactive summarization

## Installation

### Prerequisites

- **Rust 1.75+** (install via rustup - see below)
- **Git** (to clone the repository)
- An LLM endpoint (for summarization):
  - **Ollama** (local): Install [Ollama](https://ollama.ai/) and pull a model
  - **Cloud APIs**: Set the appropriate environment variables

### Building on Linux (Ubuntu/Debian/Mint)

1. Install system dependencies:
```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev git curl
```

2. Install Rust:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
Press Enter to accept defaults, then reload your shell:
```bash
source ~/.cargo/env
```

3. Verify Rust is installed:
```bash
rustc --version
cargo --version
```

4. Clone and build:
```bash
git clone https://github.com/joaovl/yousummary
cd yousummary
cargo build --release
```

5. The binary is at `target/release/yousummary`. Run it:
```bash
./target/release/yousummary --help
```

### Building on Windows

1. Install Visual Studio Build Tools:
   - Download from: https://visualstudio.microsoft.com/visual-cpp-build-tools/
   - Run the installer and select "Desktop development with C++"
   - This provides the MSVC compiler needed by Rust

2. Install Rust:
   - Download from: https://rustup.rs/
   - Run `rustup-init.exe`
   - Press Enter to accept defaults
   - Restart your terminal/PowerShell

3. Verify Rust is installed (open new PowerShell):
```powershell
rustc --version
cargo --version
```

4. Install Git (if not installed):
   - Download from: https://git-scm.com/download/win

5. Clone and build (in PowerShell):
```powershell
git clone https://github.com/joaovl/yousummary
cd yousummary
cargo build --release
```

6. The binary is at `target\release\yousummary.exe`. Run it:
```powershell
.\target\release\yousummary.exe --help
```

### Optional: Install globally

After building, you can install the binary to your PATH:

Linux:
```bash
cargo install --path .
# or manually:
sudo cp target/release/yousummary /usr/local/bin/
```

Windows (PowerShell as Admin):
```powershell
cargo install --path .
# or manually copy to a folder in your PATH
```

## Quick Start

Show available commands:

```bash
yousummary --help
```

### Fetch video data (metadata + transcript)

```bash
# Fetch and save as JSON (default: ~/Documents/yousummary/Channel - Title.json)
yousummary fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ'

# Specify output file
yousummary fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' -o video.json

# Fetch with specific language transcript
yousummary fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' -L es
```

### Summarize a YouTube video

```bash
# Using Ollama (local)
yousummary summarize 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' 'ollama/llama3.2:3b'

# Using OpenAI
export OPENAI_API_KEY='your-key'
yousummary summarize 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' 'openai/gpt-4o-mini'

# Using Anthropic
export ANTHROPIC_API_KEY='your-key'
yousummary summarize 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' 'anthropic/claude-3-haiku-20240307'

# Save to a specific file
yousummary summarize 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' 'ollama/llama3.2:3b' -o my-summary.md
```

### Summarize from pre-fetched JSON

```bash
# First, fetch the data
yousummary fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ'

# Later, summarize from the JSON file
yousummary summarize "~/Documents/yousummary/Channel - Title.json" 'ollama/llama3.2:3b'
```

### Generate key points only

```bash
yousummary summarize 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' \
  'ollama/llama3.2:3b' \
  --length short \
  --language it \
  --key-points-only \
  --print-only
```

### Summarize a playlist or batch of URLs

```bash
# Playlist
yousummary summarize 'https://www.youtube.com/playlist?list=PLxxxxxxxx' 'ollama/llama3.2:3b'

# Batch file (one URL per line)
yousummary summarize urls.txt 'ollama/llama3.2:3b'
```

## Web UI

Start a local web server for interactive summarization:

```bash
yousummary serve
```

Then open `http://127.0.0.1:8000` in your browser.

Options:

```bash
yousummary serve --host 127.0.0.1 --port 8000
yousummary serve --host 0.0.0.0 --port 8000  # Expose on your LAN
```

## CLI Reference

### `fetch` command

```
yousummary fetch <URL> [OPTIONS]

Arguments:
  <URL>  YouTube video URL to fetch

Options:
  -o, --output <OUTPUT>          Output file path (default: Channel - Title.json)
  -d, --output-dir <OUTPUT_DIR>  Output directory for saved JSON files
  -L, --language <LANGUAGE>      Preferred language for transcript [default: en]
```

### `summarize` command

```
yousummary summarize <TARGET> <MODEL> [OPTIONS]

Arguments:
  <TARGET>  YouTube URL, playlist URL, JSON file, or file containing URLs
  <MODEL>   LLM model [default: claude-cli/sonnet] (e.g., 'anthropic/claude-sonnet-5')

Options:
  -l, --length <LENGTH>        Summary length: short, medium, long [default: medium]
  -L, --language <LANGUAGE>    Output language (e.g., 'en', 'it', 'es') [default: en]
      --key-points-only        Generate key points only (no full summary)
      --with-key-points        Include key points in addition to summary
      --print-only             Print to stdout only, don't save to file
  -d, --output-dir <DIR>       Output directory for saved summaries
  -o, --output <FILE>          Output file path (for single video)
      --host <HOST>            Ollama host URL [default: http://localhost:11434]
  -f, --force                  Force regeneration even if summary exists
      --chunk-size <SIZE>      Chunk size in words [default: 2000]
      --api-key <KEY>          API key for cloud providers (can also use env vars)
```

### `serve` command

```
yousummary serve [OPTIONS]

Options:
      --host <HOST>           Host address to bind to [default: 127.0.0.1]
  -p, --port <PORT>           Port to listen on [default: 8000]
      --ollama-host <HOST>    Default Ollama host URL [default: http://localhost:11434]
      --default-model <MODEL> Default model to use [default: claude-cli/sonnet]
```

## JSON Output Format

The `fetch` command outputs JSON with the following structure:

```json
{
  "metadata": {
    "video_id": "L6ush2x6tB4",
    "title": "Video Title",
    "channel": "Channel Name",
    "channel_id": "UC...",
    "duration_seconds": 1205,
    "duration_formatted": "20:05",
    "view_count": 688790,
    "description": "Video description...",
    "thumbnail_url": "https://i.ytimg.com/...",
    "url": "https://www.youtube.com/watch?v=...",
    "fetched_at": "2026-01-02T19:01:15+00:00"
  },
  "transcript": {
    "language": "en",
    "chunks": [
      "First ~250 words of transcript...",
      "Next ~250 words...",
      "..."
    ],
    "word_count": 4637
  }
}
```

The transcript is split into ~250 word chunks for easy processing.

## Supported LLM Providers

| Provider | Model Format | Environment Variable |
|----------|--------------|---------------------|
| Claude CLI (subscription) | `claude-cli/sonnet` | - (uses `claude` CLI auth) |
| Ollama (local) | `ollama/model:tag` | - |
| OpenAI | `openai/gpt-4o-mini` | `OPENAI_API_KEY` |
| Anthropic | `anthropic/claude-3-haiku-20240307` | `ANTHROPIC_API_KEY` |
| Groq | `groq/llama-3.1-70b-versatile` | `GROQ_API_KEY` |
| Together | `together/meta-llama/Llama-3-70b-chat-hf` | `TOGETHER_API_KEY` |
| OpenRouter | `openrouter/openai/gpt-4o-mini` | `OPENROUTER_API_KEY` |

## How It Works

1. **Fetch** - Extract video metadata and transcript from YouTube
2. **Chunk** - Split transcript into word-based chunks with overlap
3. **Summarize** - Send each chunk to the LLM
4. **Merge** - Combine chunk summaries into a coherent result
5. **Output** - Save as Markdown and/or print to stdout

This approach scales smoothly from short clips to multi-hour videos.

## Project Structure

```
yousummary/
├── Cargo.toml          # Dependencies and metadata
├── README.md           # This file
├── LICENSE             # MIT License
└── src/
    ├── main.rs         # Entry point
    ├── cli.rs          # Command-line interface
    ├── config.rs       # Configuration types
    ├── error.rs        # Error handling
    ├── llm.rs          # LLM provider integrations
    ├── summarizer.rs   # Chunking and summarization logic
    ├── transcript.rs   # YouTube transcript fetching
    ├── web.rs          # Web UI server
    └── youtube.rs      # YouTube URL parsing and video info
```

## Privacy

When using Ollama, all processing happens locally on your machine. No data is sent to external servers.

When using cloud providers, transcripts are sent to the respective API for processing. Review each provider's privacy policy.

## Development

```bash
# Run tests
cargo test

# Run with debug logging
RUST_LOG=yousummary=debug cargo run -- summarize 'URL' 'ollama/llama3.2:3b'

# Build release version
cargo build --release

# Run clippy for linting
cargo clippy

# Format code
cargo fmt
```

## License

MIT. See [LICENSE](LICENSE) for details.
