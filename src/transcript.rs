use crate::error::YouSummaryError;
use regex::Regex;
use reqwest::Client;
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
pub struct TranscriptFetcher {
    client: Client,
}

impl TranscriptFetcher {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    /// Fetch transcript for a video, optionally in a specific language
    pub async fn fetch(
        &self,
        video_id: &str,
        language: Option<&str>,
    ) -> Result<String, YouSummaryError> {
        info!("Fetching transcript for video: {}", video_id);

        // First, get the video page to extract caption tracks
        let caption_tracks = self.get_caption_tracks(video_id).await?;

        if caption_tracks.is_empty() {
            return Err(YouSummaryError::TranscriptNotFound(video_id.to_string()));
        }

        debug!("Found {} caption tracks", caption_tracks.len());

        // Find the best matching track
        let track = self.select_best_track(&caption_tracks, language)?;

        // Fetch the actual transcript
        let entries = self.fetch_transcript_from_url(&track.base_url).await?;

        // Convert entries to plain text
        let text = entries
            .iter()
            .map(|e| e.text.clone())
            .collect::<Vec<_>>()
            .join(" ");

        // Clean up the text
        let text = clean_transcript_text(&text);

        info!(
            "Successfully fetched transcript ({} characters)",
            text.len()
        );
        Ok(text)
    }

    /// Fetch transcript with timing information
    #[allow(dead_code)]
    pub async fn fetch_with_timestamps(
        &self,
        video_id: &str,
        language: Option<&str>,
    ) -> Result<Vec<TranscriptEntry>, YouSummaryError> {
        let caption_tracks = self.get_caption_tracks(video_id).await?;

        if caption_tracks.is_empty() {
            return Err(YouSummaryError::TranscriptNotFound(video_id.to_string()));
        }

        let track = self.select_best_track(&caption_tracks, language)?;
        self.fetch_transcript_from_url(&track.base_url).await
    }

    /// Get available caption tracks for a video using YouTube's innertube API
    async fn get_caption_tracks(&self, video_id: &str) -> Result<Vec<CaptionTrack>, YouSummaryError> {
        // Try innertube player API first (more reliable)
        if let Ok(tracks) = self.get_caption_tracks_innertube(video_id).await {
            if !tracks.is_empty() {
                return Ok(tracks);
            }
        }

        // Fall back to HTML parsing
        debug!("Innertube API failed, falling back to HTML parsing");
        let url = format!("https://www.youtube.com/watch?v={}", video_id);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(YouSummaryError::VideoNotFound(video_id.to_string()));
        }

        let html = response.text().await?;
        self.extract_caption_tracks(&html)
    }

    /// Get caption tracks using YouTube's innertube player API (like yt-dlp does)
    async fn get_caption_tracks_innertube(&self, video_id: &str) -> Result<Vec<CaptionTrack>, YouSummaryError> {
        let api_url = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";

        // Use Android client context (works without authentication)
        let payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "ANDROID",
                    "clientVersion": "19.09.37",
                    "androidSdkVersion": 30,
                    "hl": "en",
                    "gl": "US",
                    "utcOffsetMinutes": 0
                }
            },
            "videoId": video_id,
            "playbackContext": {
                "contentPlaybackContext": {
                    "html5Preference": "HTML5_PREF_WANTS"
                }
            },
            "contentCheckOk": true,
            "racyCheckOk": true
        });

        debug!("Fetching captions via innertube API for video: {}", video_id);

        let response = self
            .client
            .post(api_url)
            .header("Content-Type", "application/json")
            .header("X-YouTube-Client-Name", "3")
            .header("X-YouTube-Client-Version", "19.09.37")
            .body(payload.to_string())
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(YouSummaryError::TranscriptFetchError(format!(
                "Innertube API returned HTTP {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response.json().await
            .map_err(|e| YouSummaryError::ParseError(format!("Failed to parse innertube response: {}", e)))?;

        // Extract caption tracks from the response
        let mut tracks = Vec::new();

        if let Some(captions) = json.get("captions") {
            if let Some(renderer) = captions.get("playerCaptionsTracklistRenderer") {
                if let Some(caption_tracks) = renderer.get("captionTracks").and_then(|t| t.as_array()) {
                    for track in caption_tracks {
                        if let Some(base_url) = track.get("baseUrl").and_then(|u| u.as_str()) {
                            let language_code = track
                                .get("languageCode")
                                .and_then(|l| l.as_str())
                                .unwrap_or("unknown")
                                .to_string();

                            let name = track
                                .get("name")
                                .and_then(|n| n.get("simpleText"))
                                .and_then(|s| s.as_str())
                                .unwrap_or(&language_code)
                                .to_string();

                            let is_auto_generated = track
                                .get("kind")
                                .and_then(|k| k.as_str())
                                .map(|k| k == "asr")
                                .unwrap_or(false);

                            tracks.push(CaptionTrack {
                                base_url: base_url.to_string(),
                                language_code,
                                name,
                                is_auto_generated,
                            });
                        }
                    }
                }
            }
        }

        if tracks.is_empty() {
            // Check if there are automatic captions available via translation
            debug!("No direct caption tracks, checking for translatable auto-captions");
        }

        debug!("Found {} caption tracks via innertube API", tracks.len());
        Ok(tracks)
    }

    /// Extract caption track info from the YouTube page HTML
    fn extract_caption_tracks(&self, html: &str) -> Result<Vec<CaptionTrack>, YouSummaryError> {
        // Look for the captions data in the player response
        let _captions_re = Regex::new(r#""captions"\s*:\s*\{[^}]*"playerCaptionsTracklistRenderer"\s*:\s*\{[^}]*"captionTracks"\s*:\s*\[([^\]]+)\]"#)
            .map_err(|e| YouSummaryError::ParseError(e.to_string()))?;

        // Alternative pattern for ytInitialPlayerResponse
        let player_response_re = Regex::new(r#"ytInitialPlayerResponse\s*=\s*(\{.+?\});"#)
            .map_err(|e| YouSummaryError::ParseError(e.to_string()))?;

        let mut tracks = Vec::new();

        // Try to find caption tracks in ytInitialPlayerResponse
        if let Some(caps) = player_response_re.captures(html) {
            let json_str = &caps[1];
            
            // Extract baseUrl and languageCode from the JSON
            let base_url_re = Regex::new(r#""baseUrl"\s*:\s*"([^"]+)""#).unwrap();
            let lang_re = Regex::new(r#""languageCode"\s*:\s*"([^"]+)""#).unwrap();
            let name_re = Regex::new(r#""name"\s*:\s*\{[^}]*"simpleText"\s*:\s*"([^"]+)""#).unwrap();
            let kind_re = Regex::new(r#""kind"\s*:\s*"([^"]+)""#).unwrap();

            // Find all caption track sections
            let track_section_re = Regex::new(r#"\{"baseUrl"[^}]+\}"#).unwrap();

            for track_match in track_section_re.find_iter(json_str) {
                let track_json = track_match.as_str();

                if let Some(url_caps) = base_url_re.captures(track_json) {
                    let base_url = url_caps[1].to_string().replace("\\u0026", "&");

                    let language_code = lang_re
                        .captures(track_json)
                        .map(|c| c[1].to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let name = name_re
                        .captures(track_json)
                        .map(|c| c[1].to_string())
                        .unwrap_or_else(|| language_code.clone());

                    let is_auto_generated = kind_re
                        .captures(track_json)
                        .map(|c| c[1].contains("asr"))
                        .unwrap_or(false);

                    tracks.push(CaptionTrack {
                        base_url,
                        language_code,
                        name,
                        is_auto_generated,
                    });
                }
            }
        }

        // Deduplicate tracks by language code
        tracks.sort_by(|a, b| {
            // Prefer non-auto-generated tracks
            match (a.is_auto_generated, b.is_auto_generated) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => a.language_code.cmp(&b.language_code),
            }
        });
        tracks.dedup_by(|a, b| a.language_code == b.language_code);

        Ok(tracks)
    }

    /// Select the best caption track based on language preference
    fn select_best_track<'a>(
        &'a self,
        tracks: &'a [CaptionTrack],
        preferred_language: Option<&str>,
    ) -> Result<&'a CaptionTrack, YouSummaryError> {
        if tracks.is_empty() {
            return Err(YouSummaryError::TranscriptNotFound(
                "No caption tracks available".to_string(),
            ));
        }

        // If a language is preferred, try to find it
        if let Some(lang) = preferred_language {
            // First try exact match
            if let Some(track) = tracks.iter().find(|t| t.language_code == lang) {
                return Ok(track);
            }

            // Try prefix match (e.g., "en" matches "en-US")
            if let Some(track) = tracks.iter().find(|t| t.language_code.starts_with(lang)) {
                return Ok(track);
            }

            warn!(
                "Preferred language '{}' not found, using first available",
                lang
            );
        }

        // Return the first non-auto-generated track if available
        if let Some(track) = tracks.iter().find(|t| !t.is_auto_generated) {
            return Ok(track);
        }

        // Fall back to any available track
        Ok(&tracks[0])
    }

    /// Fetch transcript from a caption track URL
    async fn fetch_transcript_from_url(
        &self,
        url: &str,
    ) -> Result<Vec<TranscriptEntry>, YouSummaryError> {
        // Ensure we use json3 format (replace any existing fmt parameter)
        let json_url = if url.contains("&fmt=") {
            // Replace existing format with json3
            Regex::new(r"&fmt=[^&]+")
                .unwrap()
                .replace(url, "&fmt=json3")
                .to_string()
        } else {
            format!("{}&fmt=json3", url)
        };

        debug!("Fetching transcript from: {}", &json_url[..json_url.len().min(100)]);

        let response = self.client.get(&json_url).send().await?;

        if !response.status().is_success() {
            return Err(YouSummaryError::TranscriptFetchError(format!(
                "Failed to fetch transcript: HTTP {}",
                response.status()
            )));
        }

        let content = response.text().await?;

        // Try JSON format first (json3), fall back to XML
        if content.trim().starts_with('{') {
            self.parse_transcript_json3(&content)
        } else {
            self.parse_transcript_xml(&content)
        }
    }

    /// Parse the transcript JSON3 format (YouTube's newer format)
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

    /// Parse the transcript XML format (legacy fallback)
    fn parse_transcript_xml(&self, xml: &str) -> Result<Vec<TranscriptEntry>, YouSummaryError> {
        let mut entries = Vec::new();

        // Parse <text start="X" dur="Y">content</text> elements
        let text_re = Regex::new(r#"<text\s+start="([^"]+)"\s+dur="([^"]+)"[^>]*>([^<]*)</text>"#)
            .map_err(|e| YouSummaryError::ParseError(e.to_string()))?;

        for caps in text_re.captures_iter(xml) {
            let start: f64 = caps[1].parse().unwrap_or(0.0);
            let duration: f64 = caps[2].parse().unwrap_or(0.0);
            let text = html_escape::decode_html_entities(&caps[3]).to_string();

            entries.push(TranscriptEntry {
                text,
                start,
                duration,
            });
        }

        if entries.is_empty() {
            return Err(YouSummaryError::TranscriptFetchError(
                "No text entries found in transcript".to_string(),
            ));
        }

        Ok(entries)
    }
}

impl Default for TranscriptFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a YouTube caption track
#[derive(Debug, Clone)]
struct CaptionTrack {
    base_url: String,
    language_code: String,
    #[allow(dead_code)]
    name: String,
    is_auto_generated: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_transcript_text() {
        let input = "Hello   world\n\nThis  is   a test";
        let expected = "Hello world This is a test";
        assert_eq!(clean_transcript_text(input), expected);
    }
}
