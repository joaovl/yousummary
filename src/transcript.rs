use crate::error::YouSummaryError;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Represents a single transcript entry with timing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub text: String,
    pub start: f64,
    pub duration: f64,
}

/// Fetcher for YouTube transcripts
pub struct TranscriptFetcher {}

impl TranscriptFetcher {
    pub fn new() -> Self {
        Self {}
    }

    /// Fetch transcript for a video, optionally in a specific language
    pub async fn fetch(
        &self,
        video_id: &str,
        language: Option<&str>,
    ) -> Result<String, YouSummaryError> {
        let entries = self.fetch_with_timestamps(video_id, language).await?;

        let text = entries
            .iter()
            .map(|e| e.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let text = clean_transcript_text(&text);

        info!(
            "Successfully fetched transcript ({} characters)",
            text.len()
        );
        Ok(text)
    }

    /// Fetch transcript with timing information.
    ///
    /// YouTube's player API no longer serves caption tracks to anonymous HTTP
    /// clients (every innertube client answers UNPLAYABLE or LOGIN_REQUIRED),
    /// so subtitles come from yt-dlp, which handles PO tokens, clients and
    /// cookies for us. `YT_PROXY` and `YT_POT_BASE_URL` are passed through when
    /// set, matching what ops/yt_analyst already expects.
    pub async fn fetch_with_timestamps(
        &self,
        video_id: &str,
        language: Option<&str>,
    ) -> Result<Vec<TranscriptEntry>, YouSummaryError> {
        info!("Fetching transcript for video: {}", video_id);

        let language = language.unwrap_or("en");
        let dir = tempfile::Builder::new()
            .prefix("yousummary-subs-")
            .tempdir()
            .map_err(|e| {
                YouSummaryError::TranscriptFetchError(format!("Failed to create temp dir: {}", e))
            })?;
        let template = dir.path().join("sub");

        let proxy = non_empty(std::env::var("YT_PROXY").ok());
        let pot = non_empty(std::env::var("YT_POT_BASE_URL").ok());
        let js_runtime = non_empty(std::env::var("YT_JS_RUNTIME").ok());
        let args = ytdlp_args(
            video_id,
            language,
            &template,
            proxy.as_deref(),
            pot.as_deref(),
            js_runtime.as_deref(),
        );

        debug!("Running yt-dlp {}", args.join(" "));

        let output = tokio::process::Command::new("yt-dlp")
            .args(&args)
            .output()
            .await
            .map_err(|e| {
                YouSummaryError::TranscriptFetchError(format!("Failed to run yt-dlp: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr
                .lines()
                .filter(|l| l.starts_with("ERROR:"))
                .last()
                .unwrap_or_else(|| stderr.trim())
                .to_string();
            return Err(YouSummaryError::TranscriptFetchError(format!(
                "yt-dlp failed for {}: {}",
                video_id,
                if detail.is_empty() { "no output".to_string() } else { detail }
            )));
        }

        let subtitle = find_subtitle_file(dir.path()).ok_or_else(|| {
            warn!("yt-dlp produced no subtitle file for {}", video_id);
            YouSummaryError::TranscriptNotFound(video_id.to_string())
        })?;

        let json = std::fs::read_to_string(&subtitle).map_err(|e| {
            YouSummaryError::TranscriptFetchError(format!("Failed to read subtitles: {}", e))
        })?;

        let entries = self.parse_transcript_json3(&json)?;
        if entries.is_empty() {
            return Err(YouSummaryError::TranscriptFetchError(
                "No text entries found in transcript".to_string(),
            ));
        }
        Ok(entries)
    }

    fn parse_transcript_json3(&self, json: &str) -> Result<Vec<TranscriptEntry>, YouSummaryError> {
        let mut entries = Vec::new();

        // Parse the JSON
        let data: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| YouSummaryError::ParseError(format!("Failed to parse JSON: {}", e)))?;

        // Extract events array
        if let Some(events) = data.get("events").and_then(|e| e.as_array()) {
            for event in events {
                // Get timing info
                let start_ms = event.get("tStartMs").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let duration_ms = event.get("dDurationMs").and_then(|v| v.as_f64()).unwrap_or(0.0);

                // Extract text segments
                if let Some(segs) = event.get("segs").and_then(|s| s.as_array()) {
                    let mut text_parts = Vec::new();
                    for seg in segs {
                        if let Some(utf8) = seg.get("utf8").and_then(|u| u.as_str()) {
                            // Skip newline-only segments
                            if utf8 != "\n" {
                                text_parts.push(utf8.to_string());
                            }
                        }
                    }

                    if !text_parts.is_empty() {
                        let text = text_parts.join("");
                        if !text.trim().is_empty() {
                            entries.push(TranscriptEntry {
                                text: text.trim().to_string(),
                                start: start_ms / 1000.0,
                                duration: duration_ms / 1000.0,
                            });
                        }
                    }
                }
            }
        }

        if entries.is_empty() {
            return Err(YouSummaryError::TranscriptFetchError(
                "No text entries found in JSON3 transcript".to_string(),
            ));
        }

        debug!("Parsed {} entries from JSON3 format", entries.len());
        Ok(entries)
    }

}

impl Default for TranscriptFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Clean up transcript text
fn clean_transcript_text(text: &str) -> String {
    // Replace newlines with spaces
    let text = text.replace('\n', " ");

    // Replace multiple spaces with single space
    let space_re = Regex::new(r"\s+").unwrap();
    let text = space_re.replace_all(&text, " ");

    // Trim
    text.trim().to_string()
}

/// Compose files often set a variable to "" rather than omitting it; an empty
/// value means "not configured", not "pass an empty flag".
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// Build the yt-dlp argument list for a subtitle-only download.
fn ytdlp_args(
    video_id: &str,
    language: &str,
    output_template: &std::path::Path,
    proxy: Option<&str>,
    pot_base_url: Option<&str>,
    js_runtime: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--skip-download".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--write-subs".into(),
        "--write-auto-subs".into(),
        "--sub-langs".into(),
        format!("{}.*,{}", language, language),
        "--sub-format".into(),
        "json3".into(),
        "-o".into(),
        output_template.to_string_lossy().to_string(),
    ];

    if let Some(proxy) = proxy {
        args.push("--proxy".into());
        args.push(proxy.to_string());
    }
    if let Some(base) = pot_base_url {
        args.push("--extractor-args".into());
        args.push(format!("youtube:getpot_bgutil_baseurl={}", base));
    }
    if let Some(runtime) = js_runtime {
        args.push("--js-runtimes".into());
        args.push(runtime.to_string());
    }

    args.push(format!("https://www.youtube.com/watch?v={}", video_id));
    args
}

/// yt-dlp names the file `<template>.<lang>.json3`; take whichever it wrote.
fn find_subtitle_file(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("json3"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ytdlp_args_request_json3_subtitles_for_the_language() {
        let args = ytdlp_args("abc123", "en", std::path::Path::new("/tmp/out"), None, None, None);
        assert!(args.contains(&"--write-auto-subs".to_string()));
        assert!(args.contains(&"--sub-format".to_string()));
        assert!(args.contains(&"json3".to_string()));
        assert!(args.contains(&"en.*,en".to_string()));
        assert!(args.contains(&"https://www.youtube.com/watch?v=abc123".to_string()));
        assert!(!args.contains(&"--proxy".to_string()));
    }

    #[test]
    fn blank_env_values_are_treated_as_unset() {
        assert_eq!(non_empty(Some("  ".to_string())), None);
        assert_eq!(non_empty(Some(String::new())), None);
        assert_eq!(non_empty(Some("http://p:1".to_string())), Some("http://p:1".to_string()));
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn ytdlp_args_select_the_js_runtime_when_configured() {
        let args = ytdlp_args(
            "abc123",
            "en",
            std::path::Path::new("/tmp/out"),
            None,
            None,
            Some("node"),
        );
        assert!(args.join(" ").contains("--js-runtimes node"));
    }

    #[test]
    fn ytdlp_args_include_proxy_and_pot_provider_when_configured() {
        let args = ytdlp_args(
            "abc123",
            "en",
            std::path::Path::new("/tmp/out"),
            Some("http://127.0.0.1:8899"),
            Some("http://bgutil-pot:4416"),
            None,
        );
        let joined = args.join(" ");
        assert!(joined.contains("--proxy http://127.0.0.1:8899"));
        assert!(joined.contains("getpot_bgutil_baseurl=http://bgutil-pot:4416"));
    }

    #[test]
    fn test_clean_transcript_text() {
        let input = "Hello   world\n\nThis  is   a test";
        let expected = "Hello world This is a test";
        assert_eq!(clean_transcript_text(input), expected);
    }
}
