use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SummaryLength {
    /// Brief, concise summary (1-2 paragraphs)
    Short,
    /// Balanced summary (3-5 paragraphs)
    #[default]
    Medium,
    /// Detailed summary (comprehensive coverage)
    Long,
}

impl fmt::Display for SummaryLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummaryLength::Short => write!(f, "short"),
            SummaryLength::Medium => write!(f, "medium"),
            SummaryLength::Long => write!(f, "long"),
        }
    }
}

impl SummaryLength {
    /// Get the prompt instruction for this length
    pub fn prompt_instruction(&self) -> &'static str {
        match self {
            SummaryLength::Short => "Provide a brief, concise summary in 1-2 short paragraphs. Focus only on the most important points.",
            SummaryLength::Medium => "Provide a balanced summary in 3-5 paragraphs. Cover the main points and key details.",
            SummaryLength::Long => "Provide a comprehensive, detailed summary. Cover all important topics, examples, and nuances discussed.",
        }
    }
}
