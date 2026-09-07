use crate::error::YouSummaryError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::debug;

/// Supported LLM providers
#[derive(Debug, Clone)]
pub enum LlmProvider {
    Ollama {
        host: String,
        model: String,
    },
    OpenAI {
        api_key: String,
        model: String,
        base_url: String,
    },
    Anthropic {
        api_key: String,
        model: String,
    },
    OpenRouter {
        api_key: String,
        model: String,
    },
    /// Generic OpenAI-compatible endpoint
    OpenAICompatible {
        api_key: String,
        model: String,
        base_url: String,
    },
}

impl LlmProvider {
    /// Parse a model string like "ollama/llama3.2:3b" or "openai/gpt-4o-mini"
    pub fn from_model_string(
        model_str: &str,
        ollama_host: &str,
        api_key: Option<&str>,
    ) -> Result<Self, YouSummaryError> {
        let parts: Vec<&str> = model_str.splitn(2, '/').collect();

        if parts.len() != 2 {
            return Err(YouSummaryError::InvalidModelFormat(model_str.to_string()));
        }

        let provider = parts[0].to_lowercase();
        let model = parts[1].to_string();

        match provider.as_str() {
            "ollama" => Ok(LlmProvider::Ollama {
                host: ollama_host.to_string(),
                model,
            }),
            "openai" => {
                let key = api_key
                    .map(|s| s.to_string())
                    .or_else(|| env::var("OPENAI_API_KEY").ok())
                    .ok_or_else(|| {
                        YouSummaryError::LlmError("OpenAI API key not provided".to_string())
                    })?;
                Ok(LlmProvider::OpenAI {
                    api_key: key,
                    model,
                    base_url: "https://api.openai.com/v1".to_string(),
                })
            }
            "anthropic" | "claude" => {
                let key = api_key
                    .map(|s| s.to_string())
                    .or_else(|| env::var("ANTHROPIC_API_KEY").ok())
                    .ok_or_else(|| {
                        YouSummaryError::LlmError("Anthropic API key not provided".to_string())
                    })?;
                Ok(LlmProvider::Anthropic {
                    api_key: key,
                    model,
                })
            }
            "openrouter" => {
                let key = api_key
                    .map(|s| s.to_string())
                    .or_else(|| env::var("OPENROUTER_API_KEY").ok())
                    .ok_or_else(|| {
                        YouSummaryError::LlmError("OpenRouter API key not provided".to_string())
                    })?;
                Ok(LlmProvider::OpenRouter {
                    api_key: key,
                    model,
                })
            }
            "groq" => {
                let key = api_key
                    .map(|s| s.to_string())
                    .or_else(|| env::var("GROQ_API_KEY").ok())
                    .ok_or_else(|| {
                        YouSummaryError::LlmError("Groq API key not provided".to_string())
                    })?;
                Ok(LlmProvider::OpenAICompatible {
                    api_key: key,
                    model,
                    base_url: "https://api.groq.com/openai/v1".to_string(),
                })
            }
            "together" => {
                let key = api_key
                    .map(|s| s.to_string())
                    .or_else(|| env::var("TOGETHER_API_KEY").ok())
                    .ok_or_else(|| {
                        YouSummaryError::LlmError("Together API key not provided".to_string())
                    })?;
                Ok(LlmProvider::OpenAICompatible {
                    api_key: key,
                    model,
                    base_url: "https://api.together.xyz/v1".to_string(),
                })
            }
            _ => Err(YouSummaryError::UnsupportedProvider(provider)),
        }
    }
}

/// LLM client for generating completions
pub struct LlmClient {
    provider: LlmProvider,
    client: Client,
}

impl LlmClient {
    pub fn new(provider: LlmProvider) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        Self { provider, client }
    }

    /// Generate a completion for the given prompt
    pub async fn complete(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String, YouSummaryError> {
        match &self.provider {
            LlmProvider::Ollama { host, model } => {
                self.complete_ollama(host, model, prompt, system_prompt).await
            }
            LlmProvider::OpenAI { api_key, model, base_url } => {
                self.complete_openai_compatible(base_url, api_key, model, prompt, system_prompt)
                    .await
            }
            LlmProvider::Anthropic { api_key, model } => {
                self.complete_anthropic(api_key, model, prompt, system_prompt).await
            }
            LlmProvider::OpenRouter { api_key, model } => {
                self.complete_openai_compatible(
                    "https://openrouter.ai/api/v1",
                    api_key,
                    model,
                    prompt,
                    system_prompt,
                )
                .await
            }
            LlmProvider::OpenAICompatible { api_key, model, base_url } => {
                self.complete_openai_compatible(base_url, api_key, model, prompt, system_prompt)
                    .await
            }
        }
    }

    /// Complete using Ollama API
    async fn complete_ollama(
        &self,
        host: &str,
        model: &str,
        prompt: &str,
        system_prompt: Option<&str>,
    ) -> Result<String, YouSummaryError> {
        let url = format!("{}/api/generate", host.trim_end_matches('/'));

        let request = OllamaGenerateRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            system: system_prompt.map(|s| s.to_string()),
            stream: false,
        };

        debug!("Sending request to Ollama: {}", url);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| YouSummaryError::LlmError(format!("Failed to connect to Ollama: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(YouSummaryError::LlmError(format!(
                "Ollama error ({}): {}",
                status, body
            )));
        }

        let result: OllamaGenerateResponse = response
            .json()
            .await
            .map_err(|e| YouSummaryError::LlmError(format!("Failed to parse Ollama response: {}", e)))?;

        Ok(result.response)
    }

    /// Complete using OpenAI-compatible API
    async fn complete_openai_compatible(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        prompt: &str,
        system_prompt: Option<&str>,
    ) -> Result<String, YouSummaryError> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        let mut messages = Vec::new();

        if let Some(system) = system_prompt {
            messages.push(OpenAIChatMessage {
                role: "system".to_string(),
                content: system.to_string(),
            });
        }

        messages.push(OpenAIChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        let request = OpenAIChatRequest {
            model: model.to_string(),
            messages,
            temperature: Some(0.7),
            max_tokens: None,
        };

        debug!("Sending request to OpenAI-compatible API: {}", url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| YouSummaryError::LlmError(format!("API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(YouSummaryError::LlmError(format!(
                "API error ({}): {}",
                status, body
            )));
        }

        let result: OpenAIChatResponse = response
            .json()
            .await
            .map_err(|e| YouSummaryError::LlmError(format!("Failed to parse API response: {}", e)))?;

        result
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| YouSummaryError::LlmError("No response from API".to_string()))
    }

    /// Complete using Anthropic API
    async fn complete_anthropic(
        &self,
        api_key: &str,
        model: &str,
        prompt: &str,
        system_prompt: Option<&str>,
    ) -> Result<String, YouSummaryError> {
        let url = "https://api.anthropic.com/v1/messages";

        let request = AnthropicRequest {
            model: model.to_string(),
            max_tokens: 4096,
            system: system_prompt.map(|s| s.to_string()),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        debug!("Sending request to Anthropic API");

        let response = self
            .client
            .post(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| YouSummaryError::LlmError(format!("Anthropic API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(YouSummaryError::LlmError(format!(
                "Anthropic API error ({}): {}",
                status, body
            )));
        }

        let result: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| YouSummaryError::LlmError(format!("Failed to parse Anthropic response: {}", e)))?;

        result
            .content
            .iter()
            .find(|c| c.block_type == "text")
            .map(|c| c.text.clone())
            .ok_or_else(|| YouSummaryError::LlmError("No response from Anthropic".to_string()))
    }
}

// Ollama API types
#[derive(Debug, Serialize)]
struct OllamaGenerateRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

// OpenAI-compatible API types
#[derive(Debug, Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OpenAIChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChatChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChatChoice {
    message: OpenAIChatMessage,
}

// Anthropic API types
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_string() {
        let provider = LlmProvider::from_model_string("ollama/llama3.2:3b", "http://localhost:11434", None).unwrap();
        match provider {
            LlmProvider::Ollama { model, .. } => {
                assert_eq!(model, "llama3.2:3b");
            }
            _ => panic!("Expected Ollama provider"),
        }
    }

    #[test]
    fn test_invalid_model_string() {
        let result = LlmProvider::from_model_string("invalid", "http://localhost:11434", None);
        assert!(result.is_err());
    }
}
