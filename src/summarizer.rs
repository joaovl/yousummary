use crate::config::SummaryLength;
use crate::error::YouSummaryError;
use crate::llm::LlmClient;
use tracing::info;

/// Summarizer for processing transcripts using LLMs
pub struct Summarizer {
    llm_client: LlmClient,
    chunk_size: usize,
    length: SummaryLength,
    language: String,
}

impl Summarizer {
    pub fn new(
        llm_client: LlmClient,
        chunk_size: usize,
        length: SummaryLength,
        language: String,
    ) -> Self {
        Self {
            llm_client,
            chunk_size,
            length,
            language,
        }
    }

    /// Summarize a transcript
    pub async fn summarize(&self, transcript: &str) -> Result<String, YouSummaryError> {
        let words: Vec<&str> = transcript.split_whitespace().collect();
        let word_count = words.len();

        info!(
            "Processing transcript with {} words (chunk size: {})",
            word_count, self.chunk_size
        );

        // If transcript is small enough, summarize directly
        if word_count <= self.chunk_size {
            return self.summarize_single_chunk(transcript).await;
        }

        // Split into chunks and summarize each
        let chunks = self.split_into_chunks(&words);
        info!("Split transcript into {} chunks", chunks.len());

        let mut chunk_summaries = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            info!("Processing chunk {}/{}", i + 1, chunks.len());
            let summary = self.summarize_single_chunk(chunk).await?;
            chunk_summaries.push(summary);
        }

        // Merge chunk summaries into final summary
        if chunk_summaries.len() == 1 {
            return Ok(chunk_summaries.remove(0));
        }

        info!("Merging {} chunk summaries", chunk_summaries.len());
        self.merge_summaries(&chunk_summaries).await
    }

    /// Generate key points from text
    pub async fn generate_key_points(&self, text: &str) -> Result<String, YouSummaryError> {
        let system_prompt = self.get_key_points_system_prompt();
        let prompt = self.format_key_points_prompt(text);

        self.llm_client
            .complete(&prompt, Some(&system_prompt))
            .await
    }

    /// Split words into chunks with some overlap for context
    fn split_into_chunks(&self, words: &[&str]) -> Vec<String> {
        let overlap = self.chunk_size / 10; // 10% overlap
        let step = self.chunk_size - overlap;
        let mut chunks = Vec::new();

        let mut i = 0;
        while i < words.len() {
            let end = (i + self.chunk_size).min(words.len());
            let chunk: String = words[i..end].join(" ");
            chunks.push(chunk);

            if end >= words.len() {
                break;
            }

            i += step;
        }

        chunks
    }

    /// Summarize a single chunk of text
    async fn summarize_single_chunk(&self, text: &str) -> Result<String, YouSummaryError> {
        let system_prompt = self.get_summary_system_prompt();
        let prompt = self.format_summary_prompt(text);

        self.llm_client
            .complete(&prompt, Some(&system_prompt))
            .await
    }

    /// Merge multiple chunk summaries into a coherent final summary
    async fn merge_summaries(&self, summaries: &[String]) -> Result<String, YouSummaryError> {
        let combined = summaries.join("\n\n---\n\n");
        let system_prompt = self.get_merge_system_prompt();
        let prompt = self.format_merge_prompt(&combined);

        self.llm_client
            .complete(&prompt, Some(&system_prompt))
            .await
    }

    /// Get the system prompt for summarization
    fn get_summary_system_prompt(&self) -> String {
        format!(
            r#"You are an expert summarizer. Your task is to create clear, accurate summaries of video transcripts.

Guidelines:
- {}
- Write in {}.
- Maintain the key ideas and important details from the original content.
- Use clear, accessible language.
- Structure the summary logically with smooth transitions.
- Do not add information not present in the transcript.
- Do not include meta-commentary about the summarization process."#,
            self.length.prompt_instruction(),
            self.get_language_name()
        )
    }

    /// Get the system prompt for key points extraction
    fn get_key_points_system_prompt(&self) -> String {
        format!(
            r#"You are an expert at extracting key points from content.

Guidelines:
- Extract the 5-10 most important points from the content.
- Write in {}.
- Use bullet points for clarity.
- Each point should be concise but informative.
- Focus on actionable insights and main takeaways.
- Do not add information not present in the original content."#,
            self.get_language_name()
        )
    }

    /// Get the system prompt for merging summaries
    fn get_merge_system_prompt(&self) -> String {
        format!(
            r#"You are an expert editor. Your task is to combine multiple partial summaries into a single coherent summary.

Guidelines:
- {}
- Write in {}.
- Merge the content smoothly, avoiding repetition.
- Maintain logical flow and structure.
- Keep the most important information from each section.
- Create a unified narrative that reads as a single summary."#,
            self.length.prompt_instruction(),
            self.get_language_name()
        )
    }

    /// Format the prompt for summarization
    fn format_summary_prompt(&self, text: &str) -> String {
        format!(
            r#"Please summarize the following video transcript:

---
{}
---

Provide a {} summary that captures the main points and key information."#,
            text,
            self.length
        )
    }

    /// Format the prompt for key points extraction
    fn format_key_points_prompt(&self, text: &str) -> String {
        format!(
            r#"Extract the key points from the following content:

---
{}
---

List the 5-10 most important takeaways as bullet points."#,
            text
        )
    }

    /// Format the prompt for merging summaries
    fn format_merge_prompt(&self, combined_summaries: &str) -> String {
        format!(
            r#"The following are summaries of different parts of a video. Please combine them into a single, coherent {} summary:

---
{}
---

Create a unified summary that flows naturally and avoids repetition."#,
            self.length,
            combined_summaries
        )
    }

    /// Get the full language name from code
    fn get_language_name(&self) -> &str {
        match self.language.as_str() {
            "en" => "English",
            "es" => "Spanish",
            "fr" => "French",
            "de" => "German",
            "it" => "Italian",
            "pt" => "Portuguese",
            "nl" => "Dutch",
            "ru" => "Russian",
            "ja" => "Japanese",
            "ko" => "Korean",
            "zh" => "Chinese",
            "ar" => "Arabic",
            "hi" => "Hindi",
            "pl" => "Polish",
            "sv" => "Swedish",
            "no" => "Norwegian",
            "da" => "Danish",
            "fi" => "Finnish",
            "tr" => "Turkish",
            _ => "English",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_summarizer() -> Summarizer {
        use crate::llm::{LlmClient, LlmProvider};

        let provider = LlmProvider::Ollama {
            host: "http://localhost:11434".to_string(),
            model: "test".to_string(),
        };
        let client = LlmClient::new(provider);

        Summarizer::new(client, 100, SummaryLength::Medium, "en".to_string())
    }

    #[test]
    fn test_split_into_chunks() {
        let summarizer = mock_summarizer();
        let words: Vec<&str> = (0..250).map(|_| "word").collect();

        let chunks = summarizer.split_into_chunks(&words);

        // With chunk_size=100 and overlap=10, step=90
        // 250 words should give us 3 chunks
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_split_small_text() {
        let summarizer = mock_summarizer();
        let words: Vec<&str> = (0..50).map(|_| "word").collect();

        let chunks = summarizer.split_into_chunks(&words);
        assert_eq!(chunks.len(), 1);
    }
}
