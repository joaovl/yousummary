use crate::config::SummaryLength;
use crate::error::YouSummaryError;
use crate::llm::{LlmClient, LlmProvider};
use crate::summarizer::Summarizer;
use crate::transcript::TranscriptFetcher;
use crate::youtube::{extract_video_id, fetch_video_info};
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;
use url::Url;

/// Shared state for the web server
struct AppState {
    ollama_host: String,
    default_model: String,
}

/// Request for summarization
#[derive(Debug, Deserialize)]
struct SummarizeRequest {
    url: String,
    model: Option<String>,
    length: Option<String>,
    language: Option<String>,
    key_points_only: Option<bool>,
    with_key_points: Option<bool>,
}

/// Response for summarization
#[derive(Debug, Serialize)]
struct SummarizeResponse {
    success: bool,
    title: Option<String>,
    channel: Option<String>,
    summary: Option<String>,
    error: Option<String>,
}

/// Start the web server
pub async fn start_server(
    host: &str,
    port: u16,
    ollama_host: &str,
    default_model: &str,
) -> Result<()> {
    let state = Arc::new(AppState {
        ollama_host: ollama_host.to_string(),
        default_model: default_model.to_string(),
    });

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/summarize", post(handle_summarize))
        .route("/api/health", get(health_check))
        .route("/api/ollama-status", get(check_ollama_status))
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Web UI available at http://{}", addr);
    info!("Press Ctrl+C to stop the server");

    axum::serve(listener, app).await?;

    Ok(())
}

/// Serve the main index page
async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// Check Ollama connection status
async fn check_ollama_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let url = format!("{}/api/tags", state.ollama_host);

    match client.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                // Try to get list of models
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    let models: Vec<String> = json
                        .get("models")
                        .and_then(|m| m.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_default();

                    Json(serde_json::json!({
                        "connected": true,
                        "host": state.ollama_host,
                        "models": models,
                        "default_model": state.default_model
                    }))
                } else {
                    Json(serde_json::json!({
                        "connected": true,
                        "host": state.ollama_host,
                        "models": [],
                        "default_model": state.default_model
                    }))
                }
            } else {
                Json(serde_json::json!({
                    "connected": false,
                    "host": state.ollama_host,
                    "error": format!("HTTP {}", response.status())
                }))
            }
        }
        Err(e) => {
            Json(serde_json::json!({
                "connected": false,
                "host": state.ollama_host,
                "error": e.to_string()
            }))
        }
    }
}

/// Handle summarization requests
async fn handle_summarize(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SummarizeRequest>,
) -> Response {
    match process_summarize_request(&state, request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            let response = SummarizeResponse {
                success: false,
                title: None,
                channel: None,
                summary: None,
                error: Some(e.to_string()),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

async fn process_summarize_request(
    state: &AppState,
    request: SummarizeRequest,
) -> Result<SummarizeResponse, YouSummaryError> {
    // Parse URL and extract video ID
    let url = Url::parse(&request.url).map_err(|_| YouSummaryError::InvalidUrl(request.url.clone()))?;

    let video_id = extract_video_id(&url)
        .ok_or_else(|| YouSummaryError::InvalidUrl(request.url.clone()))?;

    // Fetch video info
    let video_info = fetch_video_info(&video_id).await?;

    // Set up LLM client
    let model = request.model.unwrap_or_else(|| state.default_model.clone());
    let provider = LlmProvider::from_model_string(&model, &state.ollama_host, None)?;
    let llm_client = LlmClient::new(provider);

    // Parse length
    let length = match request.length.as_deref() {
        Some("short") => SummaryLength::Short,
        Some("long") => SummaryLength::Long,
        _ => SummaryLength::Medium,
    };

    let language = request.language.unwrap_or_else(|| "en".to_string());

    // Create summarizer
    let summarizer = Summarizer::new(llm_client, 2000, length, language.clone());

    // Fetch transcript
    let transcript_fetcher = TranscriptFetcher::new();
    let transcript = transcript_fetcher
        .fetch(&video_id, Some(&language))
        .await?;

    // Generate summary
    let key_points_only = request.key_points_only.unwrap_or(false);
    let with_key_points = request.with_key_points.unwrap_or(false);

    let summary = if key_points_only {
        summarizer.generate_key_points(&transcript).await?
    } else if with_key_points {
        let summary = summarizer.summarize(&transcript).await?;
        let key_points = summarizer.generate_key_points(&summary).await?;
        format!("{}\n\n## Key Points\n\n{}", summary, key_points)
    } else {
        summarizer.summarize(&transcript).await?
    };

    Ok(SummarizeResponse {
        success: true,
        title: Some(video_info.title),
        channel: video_info.channel,
        summary: Some(summary),
        error: None,
    })
}

/// Embedded HTML for the web UI
const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>youSummary - YouTube Video Summarizer</title>
    <style>
        :root {
            --bg-color: #1a1a2e;
            --card-bg: #16213e;
            --text-color: #eaeaea;
            --primary-color: #e94560;
            --secondary-color: #0f3460;
            --border-color: #0f3460;
            --success-color: #4ade80;
            --error-color: #f87171;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }

        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            background-color: var(--bg-color);
            color: var(--text-color);
            min-height: 100vh;
            padding: 2rem;
        }

        .container {
            max-width: 900px;
            margin: 0 auto;
        }

        header {
            text-align: center;
            margin-bottom: 2rem;
        }

        h1 {
            font-size: 2.5rem;
            color: var(--primary-color);
            margin-bottom: 0.5rem;
        }

        .subtitle {
            color: #888;
            font-size: 1.1rem;
        }

        .card {
            background-color: var(--card-bg);
            border-radius: 12px;
            padding: 1.5rem;
            margin-bottom: 1.5rem;
            border: 1px solid var(--border-color);
        }

        .form-group {
            margin-bottom: 1rem;
        }

        label {
            display: block;
            margin-bottom: 0.5rem;
            font-weight: 500;
        }

        input[type="text"],
        select {
            width: 100%;
            padding: 0.75rem 1rem;
            border: 1px solid var(--border-color);
            border-radius: 8px;
            background-color: var(--secondary-color);
            color: var(--text-color);
            font-size: 1rem;
        }

        input[type="text"]:focus,
        select:focus {
            outline: none;
            border-color: var(--primary-color);
        }

        .options-row {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 1rem;
        }

        .checkbox-group {
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }

        input[type="checkbox"] {
            width: 1.2rem;
            height: 1.2rem;
            accent-color: var(--primary-color);
        }

        button {
            background-color: var(--primary-color);
            color: white;
            border: none;
            padding: 0.75rem 2rem;
            font-size: 1rem;
            font-weight: 600;
            border-radius: 8px;
            cursor: pointer;
            transition: background-color 0.2s;
            width: 100%;
        }

        button:hover:not(:disabled) {
            background-color: #d63850;
        }

        button:disabled {
            opacity: 0.6;
            cursor: not-allowed;
        }

        .loading {
            display: none;
            text-align: center;
            padding: 2rem;
        }

        .loading.active {
            display: block;
        }

        .spinner {
            border: 3px solid var(--border-color);
            border-top: 3px solid var(--primary-color);
            border-radius: 50%;
            width: 40px;
            height: 40px;
            animation: spin 1s linear infinite;
            margin: 0 auto 1rem;
        }

        @keyframes spin {
            0% { transform: rotate(0deg); }
            100% { transform: rotate(360deg); }
        }

        .result {
            display: none;
        }

        .result.active {
            display: block;
        }

        .result-header {
            margin-bottom: 1rem;
            padding-bottom: 1rem;
            border-bottom: 1px solid var(--border-color);
        }

        .result-title {
            font-size: 1.25rem;
            color: var(--primary-color);
            margin-bottom: 0.25rem;
        }

        .result-channel {
            color: #888;
        }

        .result-content {
            white-space: pre-wrap;
            line-height: 1.7;
        }

        .result-content h2 {
            color: var(--primary-color);
            margin: 1.5rem 0 1rem;
            font-size: 1.2rem;
        }

        .result-content ul {
            margin-left: 1.5rem;
        }

        .result-content li {
            margin-bottom: 0.5rem;
        }

        .error {
            background-color: rgba(248, 113, 113, 0.1);
            border: 1px solid var(--error-color);
            color: var(--error-color);
            padding: 1rem;
            border-radius: 8px;
            display: none;
        }

        .error.active {
            display: block;
        }

        .copy-btn {
            background-color: var(--secondary-color);
            padding: 0.5rem 1rem;
            font-size: 0.875rem;
            width: auto;
            margin-top: 1rem;
        }

        footer {
            text-align: center;
            margin-top: 2rem;
            color: #666;
            font-size: 0.875rem;
        }

        footer a {
            color: var(--primary-color);
            text-decoration: none;
        }

        .logs-panel {
            margin-top: 1rem;
        }

        .logs-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            cursor: pointer;
            padding: 0.75rem 1rem;
            background-color: var(--secondary-color);
            border-radius: 8px;
            margin-bottom: 0.5rem;
        }

        .logs-header:hover {
            background-color: #1a4a7a;
        }

        .logs-header h3 {
            margin: 0;
            font-size: 0.9rem;
            color: #aaa;
        }

        .logs-toggle {
            color: #888;
            font-size: 0.8rem;
        }

        .logs-content {
            display: none;
            background-color: #0d1520;
            border-radius: 8px;
            padding: 1rem;
            font-family: monospace;
            font-size: 0.8rem;
            max-height: 200px;
            overflow-y: auto;
        }

        .logs-content.active {
            display: block;
        }

        .log-entry {
            margin-bottom: 0.3rem;
            line-height: 1.4;
        }

        .log-time {
            color: #666;
        }

        .log-info {
            color: #4ade80;
        }

        .log-warn {
            color: #fbbf24;
        }

        .log-error {
            color: #f87171;
        }

        .status-indicator {
            display: inline-block;
            width: 8px;
            height: 8px;
            border-radius: 50%;
            margin-right: 0.5rem;
        }

        .status-connected {
            background-color: #4ade80;
        }

        .status-disconnected {
            background-color: #f87171;
        }

        .status-checking {
            background-color: #fbbf24;
        }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>youSummary</h1>
            <p class="subtitle">Make YouTube videos readable</p>
        </header>

        <div class="card">
            <form id="summarize-form">
                <div class="form-group">
                    <label for="url">YouTube Video URL</label>
                    <input type="text" id="url" name="url" placeholder="https://www.youtube.com/watch?v=..." required>
                </div>

                <div class="options-row">
                    <div class="form-group">
                        <label for="model">Model</label>
                        <input type="text" id="model" name="model" placeholder="claude-cli/sonnet">
                    </div>

                    <div class="form-group">
                        <label for="length">Summary Length</label>
                        <select id="length" name="length">
                            <option value="short">Short</option>
                            <option value="medium" selected>Medium</option>
                            <option value="long">Long</option>
                        </select>
                    </div>

                    <div class="form-group">
                        <label for="language">Language</label>
                        <select id="language" name="language">
                            <option value="en" selected>English</option>
                            <option value="es">Spanish</option>
                            <option value="fr">French</option>
                            <option value="de">German</option>
                            <option value="it">Italian</option>
                            <option value="pt">Portuguese</option>
                            <option value="ja">Japanese</option>
                            <option value="ko">Korean</option>
                            <option value="zh">Chinese</option>
                        </select>
                    </div>
                </div>

                <div class="options-row" style="margin-bottom: 1.5rem;">
                    <div class="checkbox-group">
                        <input type="checkbox" id="key_points_only" name="key_points_only">
                        <label for="key_points_only" style="margin-bottom: 0;">Key points only</label>
                    </div>

                    <div class="checkbox-group">
                        <input type="checkbox" id="with_key_points" name="with_key_points">
                        <label for="with_key_points" style="margin-bottom: 0;">Include key points</label>
                    </div>
                </div>

                <button type="submit" id="submit-btn">Summarize</button>
            </form>
        </div>

        <div class="loading" id="loading">
            <div class="spinner"></div>
            <p>Generating summary... This may take a minute.</p>
        </div>

        <div class="error" id="error"></div>

        <div class="card result" id="result">
            <div class="result-header">
                <h2 class="result-title" id="result-title"></h2>
                <p class="result-channel" id="result-channel"></p>
            </div>
            <div class="result-content" id="result-content"></div>
            <button class="copy-btn" onclick="copyResult()">Copy to Clipboard</button>
        </div>

        <div class="logs-panel">
            <div class="logs-header" onclick="toggleLogs()">
                <h3><span class="status-indicator status-checking" id="status-indicator"></span>Logs</h3>
                <span class="logs-toggle" id="logs-toggle">[show]</span>
            </div>
            <div class="logs-content" id="logs-content"></div>
        </div>

        <footer>
            <p>Built with Rust • <a href="https://github.com/joaovl/yousummary">GitHub</a></p>
        </footer>
    </div>

    <script>
        const form = document.getElementById('summarize-form');
        const loading = document.getElementById('loading');
        const error = document.getElementById('error');
        const result = document.getElementById('result');
        const submitBtn = document.getElementById('submit-btn');

        form.addEventListener('submit', async (e) => {
            e.preventDefault();

            // Reset UI
            loading.classList.add('active');
            error.classList.remove('active');
            result.classList.remove('active');
            submitBtn.disabled = true;

            const formData = new FormData(form);
            const data = {
                url: formData.get('url'),
                model: formData.get('model') || undefined,
                length: formData.get('length'),
                language: formData.get('language'),
                key_points_only: formData.get('key_points_only') === 'on',
                with_key_points: formData.get('with_key_points') === 'on',
            };

            log(`Starting summarization for: ${data.url}`);
            log(`Model: ${data.model || 'default'}, Length: ${data.length}, Language: ${data.language}`);

            try {
                log('Fetching video info and transcript...');
                const response = await fetch('/api/summarize', {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify(data),
                });

                const json = await response.json();

                if (json.success) {
                    log(`Success! Title: ${json.title}`, 'info');
                    log(`Channel: ${json.channel || 'Unknown'}`, 'info');
                    document.getElementById('result-title').textContent = json.title || 'Video Summary';
                    document.getElementById('result-channel').textContent = json.channel ? `Channel: ${json.channel}` : '';
                    document.getElementById('result-content').innerHTML = formatMarkdown(json.summary);
                    result.classList.add('active');
                } else {
                    log(`Error: ${json.error}`, 'error');
                    error.textContent = json.error || 'An error occurred';
                    error.classList.add('active');
                }
            } catch (err) {
                log(`Connection failed: ${err.message}`, 'error');
                error.textContent = 'Failed to connect to the server. Is it running?';
                error.classList.add('active');
            } finally {
                loading.classList.remove('active');
                submitBtn.disabled = false;
            }
        });

        function formatMarkdown(text) {
            if (!text) return '';

            // Basic markdown formatting
            return text
                .replace(/^## (.+)$/gm, '<h2>$1</h2>')
                .replace(/^### (.+)$/gm, '<h3>$1</h3>')
                .replace(/^\* (.+)$/gm, '<li>$1</li>')
                .replace(/^- (.+)$/gm, '<li>$1</li>')
                .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
                .replace(/\n\n/g, '</p><p>')
                .replace(/^/, '<p>')
                .replace(/$/, '</p>');
        }

        function copyResult() {
            const content = document.getElementById('result-content').innerText;
            navigator.clipboard.writeText(content).then(() => {
                const btn = document.querySelector('.copy-btn');
                btn.textContent = 'Copied!';
                setTimeout(() => {
                    btn.textContent = 'Copy to Clipboard';
                }, 2000);
            });
        }

        // Logging functions
        const logsContent = document.getElementById('logs-content');
        const statusIndicator = document.getElementById('status-indicator');
        let logsVisible = false;

        function toggleLogs() {
            logsVisible = !logsVisible;
            logsContent.classList.toggle('active', logsVisible);
            document.getElementById('logs-toggle').textContent = logsVisible ? '[hide]' : '[show]';
        }

        function log(message, level = 'info') {
            const time = new Date().toLocaleTimeString();
            const entry = document.createElement('div');
            entry.className = 'log-entry';
            entry.innerHTML = `<span class="log-time">[${time}]</span> <span class="log-${level}">${message}</span>`;
            logsContent.appendChild(entry);
            logsContent.scrollTop = logsContent.scrollHeight;
        }

        function setStatus(connected) {
            statusIndicator.className = 'status-indicator ' + (connected ? 'status-connected' : 'status-disconnected');
        }

        // Check Ollama status on page load
        async function checkOllamaStatus() {
            log('Checking Ollama connection...');
            try {
                const response = await fetch('/api/ollama-status');
                const data = await response.json();

                if (data.connected) {
                    setStatus(true);
                    log(`Connected to Ollama at ${data.host}`, 'info');
                    if (data.models && data.models.length > 0) {
                        log(`Available models: ${data.models.join(', ')}`, 'info');
                    } else {
                        log('No models found. Run: ollama pull llama3.2:3b', 'warn');
                    }
                    log(`Default model: ${data.default_model}`, 'info');
                } else {
                    setStatus(false);
                    log(`Cannot connect to Ollama at ${data.host}`, 'error');
                    log(`Error: ${data.error}`, 'error');
                    log('Make sure Ollama is running: ollama serve', 'warn');
                }
            } catch (err) {
                setStatus(false);
                log('Failed to check Ollama status', 'error');
                log(err.message, 'error');
            }
        }

        // Run on page load
        checkOllamaStatus();
        log('youSummary web UI ready', 'info');
    </script>
</body>
</html>
"##;
