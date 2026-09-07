use crate::config::SummaryLength;
use crate::error::YouSummaryError;
use crate::llm::{LlmClient, LlmProvider};
use crate::summarizer::Summarizer;
use crate::transcript::TranscriptFetcher;
use crate::web::start_server;
use crate::youtube::{extract_video_id, fetch_video_metadata, resolve_target, VideoInfo, VideoMetadata};
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};
use url::Url;

#[derive(Parser)]
#[command(name = "yousummary")]
#[command(author, version, about = "Fetch YouTube video metadata/transcripts and generate AI summaries.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Fetch video metadata and transcript, save as JSON
    Fetch(FetchArgs),
    /// Summarize a YouTube video, playlist, or batch of URLs
    Summarize(SummarizeArgs),
    /// Start a local web server for interactive summarization
    Serve(ServeArgs),
}

#[derive(Parser)]
pub struct FetchArgs {
    /// YouTube video URL to fetch
    #[arg(required = true)]
    pub url: String,

    /// Output file path (default: {video_id}.json in output directory)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output directory for saved JSON files
    #[arg(short = 'd', long)]
    pub output_dir: Option<PathBuf>,

    /// Preferred language for transcript
    #[arg(short = 'L', long, default_value = "en")]
    pub language: String,
}

#[derive(Parser)]
pub struct SummarizeArgs {
    /// YouTube video URL, playlist URL, or path to a file containing URLs
    #[arg(required = true)]
    pub target: String,

    /// LLM model to use (e.g., 'anthropic/claude-sonnet-5', 'ollama/llama3.2:3b')
    #[arg(default_value = "anthropic/claude-sonnet-5")]
    pub model: String,

    /// Summary length: short, medium, or long
    #[arg(short, long, default_value = "medium")]
    pub length: SummaryLength,

    /// Output language for the summary (e.g., 'en', 'it', 'es')
    #[arg(short = 'L', long, default_value = "en")]
    pub language: String,

    /// Generate key points only (no full summary)
    #[arg(long, default_value = "false")]
    pub key_points_only: bool,

    /// Print to stdout only, don't save to file
    #[arg(long, default_value = "false")]
    pub print_only: bool,

    /// Output directory for saved summaries
    #[arg(short = 'd', long)]
    pub output_dir: Option<PathBuf>,

    /// Output file path (overrides output_dir, for single video only)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Ollama host URL (for ollama models)
    #[arg(long, default_value = "http://localhost:11434")]
    pub host: String,

    /// Force regeneration even if summary exists
    #[arg(short, long, default_value = "false")]
    pub force: bool,

    /// Include key points in addition to summary
    #[arg(long, default_value = "false")]
    pub with_key_points: bool,

    /// Chunk size in words for transcript splitting
    #[arg(long, default_value = "2000")]
    pub chunk_size: usize,

    /// API key for cloud providers (can also use env vars)
    #[arg(long, env = "LLM_API_KEY")]
    pub api_key: Option<String>,
}

#[derive(Parser)]
pub struct ServeArgs {
    /// Host address to bind to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on
    #[arg(short, long, default_value = "8000")]
    pub port: u16,

    /// Default Ollama host URL
    #[arg(long, default_value = "http://localhost:11434")]
    pub ollama_host: String,

    /// Default model to use
    #[arg(long, default_value = "anthropic/claude-sonnet-5")]
    pub default_model: String,
}

/// JSON output format for the fetch command
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchOutput {
    pub metadata: VideoMetadata,
    pub transcript: TranscriptOutput,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptOutput {
    pub language: String,
    pub chunks: Vec<String>,
    pub word_count: usize,
}

pub async fn handle_fetch(args: FetchArgs) -> Result<()> {
    info!("Fetching video data...");

    // Parse URL and extract video ID
    let url = Url::parse(&args.url)
        .map_err(|_| YouSummaryError::InvalidUrl(args.url.clone()))?;

    let video_id = extract_video_id(&url)
        .ok_or_else(|| YouSummaryError::InvalidUrl(args.url.clone()))?;

    info!("Video ID: {}", video_id);

    // Fetch metadata
    info!("Fetching metadata...");
    let metadata = fetch_video_metadata(&video_id).await?;
    info!("Title: {}", metadata.title);

    // Fetch transcript
    info!("Fetching transcript...");
    let transcript_fetcher = TranscriptFetcher::new();
    let full_text = transcript_fetcher
        .fetch(&video_id, Some(&args.language))
        .await?;

    // Split into ~250 word chunks
    let words: Vec<&str> = full_text.split_whitespace().collect();
    let word_count = words.len();
    let chunk_size = 250;
    let chunks: Vec<String> = words
        .chunks(chunk_size)
        .map(|chunk| chunk.join(" "))
        .collect();

    info!("Fetched transcript: {} words in {} chunks", word_count, chunks.len());

    // Determine output path (before moving metadata)
    let output_path = if let Some(path) = args.output {
        path
    } else {
        let output_dir = args.output_dir.clone().unwrap_or_else(|| {
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("yousummary")
        });
        std::fs::create_dir_all(&output_dir)?;

        // Create filename: "channel - title.json"
        let channel = metadata.channel.as_deref().unwrap_or("Unknown");
        let filename = format!("{} - {}.json", channel, metadata.title);
        let filename = sanitize_filename(&filename);
        output_dir.join(filename)
    };

    // Build output
    let output = FetchOutput {
        metadata,
        transcript: TranscriptOutput {
            language: args.language.clone(),
            chunks,
            word_count,
        },
    };

    // Write JSON
    let json = serde_json::to_string_pretty(&output)?;
    std::fs::write(&output_path, &json)?;

    info!("Saved to: {}", output_path.display());
    println!("Saved video data to: {}", output_path.display());

    Ok(())
}

pub async fn handle_summarize(args: SummarizeArgs) -> Result<()> {
    info!("Starting summarization...");

    // Parse the LLM provider
    let provider = LlmProvider::from_model_string(&args.model, &args.host, args.api_key.as_deref())?;
    let llm_client = LlmClient::new(provider);

    // Determine output directory
    let output_dir = args.output_dir.clone().unwrap_or_else(|| {
        dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("yousummary")
    });

    if !args.print_only {
        std::fs::create_dir_all(&output_dir)?;
    }

    // Check if target is a JSON file from the fetch command
    let target_path = std::path::Path::new(&args.target);
    if target_path.exists() && target_path.extension().map_or(false, |ext| ext == "json") {
        info!("Loading pre-fetched data from JSON file...");
        let json_content = std::fs::read_to_string(target_path)?;
        let fetch_output: FetchOutput = serde_json::from_str(&json_content)
            .map_err(|e| YouSummaryError::ParseError(format!("Invalid JSON file: {}", e)))?;

        let summarizer = Summarizer::new(
            llm_client,
            args.chunk_size,
            args.length,
            args.language.clone(),
        );

        // Create VideoInfo from metadata
        let video = VideoInfo {
            video_id: fetch_output.metadata.video_id.clone(),
            title: fetch_output.metadata.title.clone(),
            channel: fetch_output.metadata.channel.clone(),
            duration: fetch_output.metadata.duration_formatted.clone(),
        };

        // Reconstruct full text from chunks
        let full_text = fetch_output.transcript.chunks.join(" ");

        return process_video_from_json(
            &video,
            &full_text,
            &summarizer,
            &args,
            &output_dir,
        ).await;
    }

    // Resolve the target (single video, playlist, or file)
    let targets = resolve_target(&args.target).await?;

    if targets.is_empty() {
        return Err(YouSummaryError::NoVideosFound.into());
    }

    info!("Found {} video(s) to process", targets.len());

    // Create transcript fetcher and summarizer
    let transcript_fetcher = TranscriptFetcher::new();
    let summarizer = Summarizer::new(
        llm_client,
        args.chunk_size,
        args.length,
        args.language.clone(),
    );

    for target in targets {
        match process_video(
            &target,
            &transcript_fetcher,
            &summarizer,
            &args,
            &output_dir,
        )
        .await
        {
            Ok(_) => info!("Successfully processed: {}", target.title),
            Err(e) => warn!("Failed to process {}: {}", target.video_id, e),
        }
    }

    Ok(())
}

async fn process_video(
    video: &VideoInfo,
    transcript_fetcher: &TranscriptFetcher,
    summarizer: &Summarizer,
    args: &SummarizeArgs,
    output_dir: &PathBuf,
) -> Result<()> {
    // Determine output file path
    let output_file = if let Some(ref path) = args.output {
        path.clone()
    } else {
        output_dir.join(format!("{}.md", sanitize_filename(&video.title)))
    };

    // Check if summary already exists
    if output_file.exists() && !args.force {
        info!("Summary already exists for '{}', skipping (use --force to regenerate)", video.title);
        return Ok(());
    }

    info!("Processing: {}", video.title);

    // Fetch transcript
    let transcript = transcript_fetcher
        .fetch(&video.video_id, Some(&args.language))
        .await?;

    if transcript.is_empty() {
        return Err(YouSummaryError::TranscriptNotFound(video.video_id.clone()).into());
    }

    // Generate summary
    let result = if args.key_points_only {
        summarizer.generate_key_points(&transcript).await?
    } else if args.with_key_points {
        let summary = summarizer.summarize(&transcript).await?;
        let key_points = summarizer.generate_key_points(&summary).await?;
        format!("{}\n\n## Key Points\n\n{}", summary, key_points)
    } else {
        summarizer.summarize(&transcript).await?
    };

    // Format as Markdown
    let markdown = format_markdown(video, &result, args.key_points_only);

    // Output
    if args.print_only {
        println!("{}", markdown);
    } else {
        std::fs::write(&output_file, &markdown)?;
        info!("Saved summary to: {}", output_file.display());
        println!("{}", markdown);
    }

    Ok(())
}

/// Process a video from pre-fetched JSON data (from the fetch command)
async fn process_video_from_json(
    video: &VideoInfo,
    transcript: &str,
    summarizer: &Summarizer,
    args: &SummarizeArgs,
    output_dir: &PathBuf,
) -> Result<()> {
    // Determine output file path
    let output_file = if let Some(ref path) = args.output {
        path.clone()
    } else {
        output_dir.join(format!("{}.md", sanitize_filename(&video.title)))
    };

    // Check if summary already exists
    if output_file.exists() && !args.force {
        info!("Summary already exists for '{}', skipping (use --force to regenerate)", video.title);
        return Ok(());
    }

    info!("Processing from JSON: {}", video.title);
    info!("Transcript length: {} characters", transcript.len());

    if transcript.is_empty() {
        return Err(YouSummaryError::TranscriptNotFound(video.video_id.clone()).into());
    }

    // Generate summary
    let result = if args.key_points_only {
        summarizer.generate_key_points(transcript).await?
    } else if args.with_key_points {
        let summary = summarizer.summarize(transcript).await?;
        let key_points = summarizer.generate_key_points(&summary).await?;
        format!("{}\n\n## Key Points\n\n{}", summary, key_points)
    } else {
        summarizer.summarize(transcript).await?
    };

    // Format as Markdown
    let markdown = format_markdown(video, &result, args.key_points_only);

    // Output
    if args.print_only {
        println!("{}", markdown);
    } else {
        std::fs::write(&output_file, &markdown)?;
        info!("Saved summary to: {}", output_file.display());
        println!("{}", markdown);
    }

    Ok(())
}

fn format_markdown(video: &VideoInfo, content: &str, key_points_only: bool) -> String {
    let section_title = if key_points_only {
        "Key Points"
    } else {
        "Summary"
    };

    format!(
        r#"# {}

**Source:** [YouTube](https://www.youtube.com/watch?v={})
**Channel:** {}
**Generated:** {}

## {}

{}
"#,
        video.title,
        video.video_id,
        video.channel.as_deref().unwrap_or("Unknown"),
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        section_title,
        content
    )
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub async fn handle_serve(args: ServeArgs) -> Result<()> {
    info!("Starting web server on {}:{}", args.host, args.port);
    start_server(&args.host, args.port, &args.ollama_host, &args.default_model).await
}
