pub mod bm25;
pub mod prompt_builder;

use std::path::Path;

use crate::domain::types::RetrievalHit;

pub use bm25::{Bm25BookRetriever, BookRetriever, RetrievalError};
pub use prompt_builder::{
    estimate_tokens, AssembledPrompt, PromptBuilder, PromptConfig, PromptError, PromptSources,
};

pub fn retrieve_book_context(
    retriever: &dyn BookRetriever,
    source_text: &str,
    limit: usize,
) -> Vec<RetrievalHit> {
    retriever.retrieve(source_text, limit)
}

pub fn load_default_retriever(book_dir: &Path) -> Result<Bm25BookRetriever, RetrievalError> {
    Bm25BookRetriever::from_book_dir(book_dir)
}
