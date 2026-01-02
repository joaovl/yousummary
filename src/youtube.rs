use crate::error::YouSummaryError;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub video_id: String,
    pub title: String,
    pub channel: Option<String>,
    pub duration: Option<String>,
}

/// Extended video metadata for the fetch command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub video_id: String,
    pub title: String,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub duration_seconds: Option<u64>,
    pub duration_formatted: Option<String>,
    pub view_count: Option<u64>,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub url: String,
    pub fetched_at: String,
}

/// Resolve the target into a list of video info structs
pub async fn resolve_target(target: &str) -> Result<Vec<VideoInfo>, YouSummaryError> {
    // Check if it's a file path
    if Path::new(target).exists() {
        return resolve_file(target).await;
    }

    // Try to parse as URL
    if let Ok(url) = Url::parse(target) {
        return resolve_url(&url).await;
    }

    // Try as a video ID directly
    if is_valid_video_id(target) {
        let info = fetch_video_info(target).await?;
        return Ok(vec![info]);
    }

    Err(YouSummaryError::InvalidUrl(target.to_string()))
}

async fn resolve_url(url: &Url) -> Result<Vec<VideoInfo>, YouSummaryError> {
    let host = url.host_str().unwrap_or("");

    // Check if it's a YouTube URL
    if !host.contains("youtube.com") && !host.contains("youtu.be") {
        return Err(YouSummaryError::InvalidUrl(url.to_string()));
    }

    // Check for playlist
    if let Some(playlist_id) = extract_playlist_id(url) {
        return fetch_playlist_videos(&playlist_id).await;
    }

    // Extract video ID
    if let Some(video_id) = extract_video_id(url) {
        let info = fetch_video_info(&video_id).await?;
        return Ok(vec![info]);
    }

    Err(YouSummaryError::InvalidUrl(url.to_string()))
}

async fn resolve_file(path: &str) -> Result<Vec<VideoInfo>, YouSummaryError> {
    let content = std::fs::read_to_string(path)?;
    let mut videos = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Ok(url) = Url::parse(line) {
            if let Some(video_id) = extract_video_id(&url) {
                match fetch_video_info(&video_id).await {
                    Ok(info) => videos.push(info),
                    Err(e) => {
                        debug!("Failed to fetch info for {}: {}", video_id, e);
                    }
                }
            }
        }
    }

    Ok(videos)
}

/// Extract video ID from various YouTube URL formats
pub fn extract_video_id(url: &Url) -> Option<String> {
    let host = url.host_str()?;

    // Handle youtu.be short URLs
    if host == "youtu.be" {
        let path = url.path();
        let id = path.trim_start_matches('/');
        if is_valid_video_id(id) {
            return Some(id.to_string());
        }
    }

    // Handle youtube.com URLs
    if host.contains("youtube.com") {
        // Check for /watch?v= format
        for (key, value) in url.query_pairs() {
            if key == "v" && is_valid_video_id(&value) {
                return Some(value.to_string());
            }
        }

        // Check for /embed/ format
        if url.path().starts_with("/embed/") {
            let id = url.path().trim_start_matches("/embed/");
            let id = id.split('/').next().unwrap_or(id);
            let id = id.split('?').next().unwrap_or(id);
            if is_valid_video_id(id) {
                return Some(id.to_string());
            }
        }

        // Check for /v/ format
        if url.path().starts_with("/v/") {
            let id = url.path().trim_start_matches("/v/");
            let id = id.split('/').next().unwrap_or(id);
            if is_valid_video_id(id) {
                return Some(id.to_string());
            }
        }

        // Check for /shorts/ format
        if url.path().starts_with("/shorts/") {
            let id = url.path().trim_start_matches("/shorts/");
            let id = id.split('/').next().unwrap_or(id);
            if is_valid_video_id(id) {
                return Some(id.to_string());
            }
        }
    }

    None
}

/// Extract playlist ID from YouTube URL
fn extract_playlist_id(url: &Url) -> Option<String> {
    for (key, value) in url.query_pairs() {
        if key == "list" {
            return Some(value.to_string());
        }
    }
    None
}

/// Check if a string is a valid YouTube video ID (11 characters, alphanumeric + - _)
fn is_valid_video_id(id: &str) -> bool {
    id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Fetch video information from YouTube
pub async fn fetch_video_info(video_id: &str) -> Result<VideoInfo, YouSummaryError> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    // Fetch the watch page to extract title and channel
    let url = format!("https://www.youtube.com/watch?v={}", video_id);
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(YouSummaryError::VideoNotFound(video_id.to_string()));
    }

    let html = response.text().await?;

    // Extract title from the page
    let title = extract_title_from_html(&html).unwrap_or_else(|| format!("Video {}", video_id));
    let channel = extract_channel_from_html(&html);

    Ok(VideoInfo {
        video_id: video_id.to_string(),
        title,
        channel,
        duration: None,
    })
}

/// Fetch extended video metadata using the innertube API
pub async fn fetch_video_metadata(video_id: &str) -> Result<VideoMetadata, YouSummaryError> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    let api_url = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";

    let payload = serde_json::json!({
        "context": {
            "client": {
                "clientName": "ANDROID",
                "clientVersion": "19.09.37",
                "androidSdkVersion": 30,
                "hl": "en",
                "gl": "US"
            }
        },
        "videoId": video_id
    });

    debug!("Fetching metadata via innertube API for video: {}", video_id);

    let response = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("X-YouTube-Client-Name", "3")
        .header("X-YouTube-Client-Version", "19.09.37")
        .body(payload.to_string())
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(YouSummaryError::VideoNotFound(video_id.to_string()));
    }

    let json: serde_json::Value = response.json().await
        .map_err(|e| YouSummaryError::ParseError(format!("Failed to parse innertube response: {}", e)))?;

    // Extract video details
    let video_details = json.get("videoDetails")
        .ok_or_else(|| YouSummaryError::ParseError("No videoDetails in response".to_string()))?;

    let title = video_details.get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown Title")
        .to_string();

    let channel = video_details.get("author")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let channel_id = video_details.get("channelId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let duration_seconds = video_details.get("lengthSeconds")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());

    let duration_formatted = duration_seconds.map(|secs| {
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;
        if hours > 0 {
            format!("{}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{}:{:02}", minutes, seconds)
        }
    });

    let view_count = video_details.get("viewCount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());

    let description = video_details.get("shortDescription")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let thumbnail_url = video_details.get("thumbnail")
        .and_then(|t| t.get("thumbnails"))
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());

    let fetched_at = chrono::Utc::now().to_rfc3339();

    Ok(VideoMetadata {
        video_id: video_id.to_string(),
        title,
        channel,
        channel_id,
        duration_seconds,
        duration_formatted,
        view_count,
        description,
        thumbnail_url,
        url: format!("https://www.youtube.com/watch?v={}", video_id),
        fetched_at,
    })
}

fn extract_title_from_html(html: &str) -> Option<String> {
    // Try to extract from og:title meta tag
    let og_title_re = Regex::new(r#"<meta\s+property="og:title"\s+content="([^"]+)""#).ok()?;
    if let Some(caps) = og_title_re.captures(html) {
        return Some(html_escape::decode_html_entities(&caps[1]).to_string());
    }

    // Try to extract from <title> tag
    let title_re = Regex::new(r#"<title>([^<]+)</title>"#).ok()?;
    if let Some(caps) = title_re.captures(html) {
        let title = caps[1].to_string();
        // Remove " - YouTube" suffix
        let title = title.trim_end_matches(" - YouTube").to_string();
        return Some(html_escape::decode_html_entities(&title).to_string());
    }

    None
}

fn extract_channel_from_html(html: &str) -> Option<String> {
    // Try to extract from link tag with channel info
    let channel_re = Regex::new(r#""ownerChannelName"\s*:\s*"([^"]+)""#).ok()?;
    if let Some(caps) = channel_re.captures(html) {
        return Some(html_escape::decode_html_entities(&caps[1]).to_string());
    }

    // Fallback: try og:site_name or author
    let author_re = Regex::new(r#"<link\s+itemprop="name"\s+content="([^"]+)""#).ok()?;
    if let Some(caps) = author_re.captures(html) {
        return Some(html_escape::decode_html_entities(&caps[1]).to_string());
    }

    None
}

/// Fetch videos from a playlist
async fn fetch_playlist_videos(playlist_id: &str) -> Result<Vec<VideoInfo>, YouSummaryError> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    let url = format!("https://www.youtube.com/playlist?list={}", playlist_id);
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(YouSummaryError::PlaylistError(format!(
            "Failed to fetch playlist: {}",
            playlist_id
        )));
    }

    let html = response.text().await?;

    // Extract video IDs from the playlist page
    let video_id_re = Regex::new(r#""videoId"\s*:\s*"([a-zA-Z0-9_-]{11})""#)
        .map_err(|e| YouSummaryError::ParseError(e.to_string()))?;

    let mut video_ids: Vec<String> = Vec::new();
    for caps in video_id_re.captures_iter(&html) {
        let id = caps[1].to_string();
        if !video_ids.contains(&id) {
            video_ids.push(id);
        }
    }

    if video_ids.is_empty() {
        return Err(YouSummaryError::PlaylistError(
            "No videos found in playlist".to_string(),
        ));
    }

    info!("Found {} videos in playlist", video_ids.len());

    // Fetch info for each video
    let mut videos = Vec::new();
    for video_id in video_ids.iter().take(50) {
        // Limit to 50 videos
        match fetch_video_info(video_id).await {
            Ok(info) => videos.push(info),
            Err(e) => {
                debug!("Failed to fetch info for {}: {}", video_id, e);
            }
        }
    }

    Ok(videos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_video_id() {
        let cases = vec![
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", Some("dQw4w9WgXcQ")),
            ("https://youtu.be/dQw4w9WgXcQ", Some("dQw4w9WgXcQ")),
            ("https://www.youtube.com/embed/dQw4w9WgXcQ", Some("dQw4w9WgXcQ")),
            ("https://www.youtube.com/v/dQw4w9WgXcQ", Some("dQw4w9WgXcQ")),
            ("https://www.youtube.com/shorts/dQw4w9WgXcQ", Some("dQw4w9WgXcQ")),
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLtest", Some("dQw4w9WgXcQ")),
            ("https://www.youtube.com/", None),
            ("https://example.com/video", None),
        ];

        for (url_str, expected) in cases {
            let url = Url::parse(url_str).unwrap();
            let result = extract_video_id(&url);
            assert_eq!(result.as_deref(), expected, "Failed for URL: {}", url_str);
        }
    }

    #[test]
    fn test_is_valid_video_id() {
        assert!(is_valid_video_id("dQw4w9WgXcQ"));
        assert!(is_valid_video_id("abcdefghijk"));
        assert!(is_valid_video_id("ABC-_123456"));
        assert!(!is_valid_video_id("short"));
        assert!(!is_valid_video_id("toolongvideoid123"));
        assert!(!is_valid_video_id("invalid!@#$"));
    }
}
