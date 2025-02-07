use tantivy::tokenizer::TextAnalyzer;

use super::tokenizer_types::TokenizerType;

pub struct TokenizerConfig {
    pub tokenizer_type: TokenizerType,
    pub text_analyzer: TextAnalyzer,
    pub doc_store: bool,
    pub doc_index: bool,
    pub is_text_field: bool,
    pub doc_fast: bool,
    pub doc_coerce: bool,
}

impl TokenizerConfig {
    pub fn new(tokenizer_type: TokenizerType, analyzer: TextAnalyzer, stored: bool) -> Self {
        Self {
            tokenizer_type: tokenizer_type.clone(),
            text_analyzer: analyzer.clone(),
            doc_store: stored,
            doc_index: true,
            is_text_field: true,
            doc_fast: false,
            doc_coerce: false,
        }
    }

    pub fn new_non_text(
        tokenizer_type: TokenizerType,
        stored: bool,
        indexed: bool,
        fast: bool,
        coerce: bool,
    ) -> Self {
        Self {
            tokenizer_type: tokenizer_type.clone(),
            text_analyzer: TextAnalyzer::default(),
            doc_store: stored,
            doc_index: indexed,
            is_text_field: false,
            doc_fast: fast,
            doc_coerce: coerce,
        }
    }
}
