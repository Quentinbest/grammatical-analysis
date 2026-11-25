use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::types::RetrievalHit;

const OUTPUT_SCHEMA_BLOCK: &str = r#"## Required JSON Output Contract
Return ONLY a single JSON object matching `StructuredGrammarCard` (no markdown fences):
{
  "kind": "structured",
  "sourceText": "<exact original input>",
  "meaning": "<natural Simplified Chinese whole-sentence meaning>",
  "parts": [
    { "kind": "token", "id": "t1", "text": "...", "role": "subj|pred|obj|mod", "subLabel": "...", "star": false, "antecedentIndex": 1, "note": "..." },
    { "kind": "clause", "id": "c1", "badge": "...", "connector": "...", "omittedHint": "...", "star": false, "items": [
        { "kind": "sub-token", "id": "st1", "text": "...", "role": "subj|pred|obj|mod", "note": "..." },
        { "kind": "gap", "id": "g1", "index": 1, "note": "..." }
    ]},
    { "kind": "punct", "text": "." }
  ],
  "recombination": "<how the blocks recombine into the full meaning>",
  "caveatNote": "<relevant caveat from book-caveats.md or null>",
  "bookRef": { "chapter": "chXX", "section": "...", "passageId": "chXX-bYYYY" }
}"#;

#[derive(Debug)]
pub enum PromptError {
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for PromptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read prompt file {}: {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for PromptError {}

#[derive(Debug, Clone)]
pub struct PromptSources {
    pub skill_md: String,
    pub workflows_md: String,
    pub teaching_method_md: String,
    pub book_caveats_md: String,
}

impl PromptSources {
    pub fn load_from_dir(prompts_dir: &Path) -> Result<Self, PromptError> {
        let read = |name: &str| -> Result<String, PromptError> {
            let path = prompts_dir.join(name);
            fs::read_to_string(&path).map_err(|source| PromptError::Io { path, source })
        };

        Ok(Self {
            skill_md: read("SKILL.md")?,
            workflows_md: read("workflows.md")?,
            teaching_method_md: read("teaching-method.md")?,
            book_caveats_md: read("book-caveats.md")?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PromptConfig {
    pub max_total_chars: usize,
    pub max_hit_excerpt_chars: usize,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            max_total_chars: 14_000,
            max_hit_excerpt_chars: 480,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    pub system_prompt: String,
    pub user_prompt: String,
    pub included_caveats: bool,
    pub included_hits: usize,
    pub total_chars: usize,
    pub estimated_tokens: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct PromptBuilder {
    sources: PromptSources,
    config: PromptConfig,
}

impl PromptBuilder {
    pub fn new(sources: PromptSources, config: PromptConfig) -> Self {
        Self { sources, config }
    }

    pub fn from_dir(prompts_dir: &Path, config: PromptConfig) -> Result<Self, PromptError> {
        let sources = PromptSources::load_from_dir(prompts_dir)?;
        Ok(Self::new(sources, config))
    }

    pub fn build(&self, source_text: &str, hits: &[RetrievalHit]) -> AssembledPrompt {
        let user_prompt = format!(
            "Analyze the following English text and return ONLY valid JSON matching the schema:\n\n{}",
            source_text.trim()
        );
        let user_chars = user_prompt.chars().count();

        // Mandatory blocks that must always be included: caveats + output schema.
        let caveats_section = format!(
            "## Mandatory Book Caveats (Always Apply)\n{}",
            self.sources.book_caveats_md.trim()
        );
        let schema_section = OUTPUT_SCHEMA_BLOCK.to_string();

        let skill_section = format!(
            "## Core Instructions (SKILL.md)\n{}",
            strip_frontmatter(&self.sources.skill_md).trim()
        );
        let teaching_section = format!(
            "## Teaching Method (teaching-method.md)\n{}",
            self.sources.teaching_method_md.trim()
        );
        let workflows_section = format!(
            "## Task Workflows (workflows.md)\n{}",
            self.sources.workflows_md.trim()
        );

        let hits_section = format_hits_section(hits, self.config.max_hit_excerpt_chars);

        let budget_for_system = self.config.max_total_chars.saturating_sub(user_chars);

        // Priority order for inclusion:
        // 1. Mandatory: caveats + schema (always included; only trimmed if cap itself is tiny)
        // 2. Retrieved textbook hits
        // 3. SKILL.md
        // 4. teaching-method.md
        // 5. workflows.md
        let mut system_parts: Vec<String> = Vec::with_capacity(6);
        let mut used_chars = 0usize;
        let mut truncated = false;

        let mut push_mandatory = |block: &str| {
            let sep = if system_parts.is_empty() { 0 } else { 2 };
            let remaining = budget_for_system.saturating_sub(used_chars + sep);
            let block_len = block.chars().count();
            if block_len <= remaining {
                system_parts.push(block.to_string());
                used_chars += sep + block_len;
            } else {
                truncated = true;
                if remaining > 0 {
                    let clipped = truncate_chars(block, remaining);
                    used_chars += sep + clipped.chars().count();
                    system_parts.push(clipped);
                }
            }
        };

        // Always place caveats first so guardrails are never dropped under tight budgets.
        push_mandatory(&caveats_section);
        push_mandatory(&schema_section);

        let mut included_hits = 0usize;
        if !hits_section.is_empty() {
            let sep = if system_parts.is_empty() { 0 } else { 2 };
            let remaining = budget_for_system.saturating_sub(used_chars + sep);
            if remaining > 40 {
                let hits_len = hits_section.chars().count();
                if hits_len <= remaining {
                    system_parts.push(hits_section);
                    used_chars += sep + hits_len;
                    included_hits = hits.len();
                } else {
                    truncated = true;
                    let clipped = truncate_chars(&hits_section, remaining);
                    included_hits = clipped.matches("\n- [").count().max(usize::from(!hits.is_empty()));
                    used_chars += sep + clipped.chars().count();
                    system_parts.push(clipped);
                }
            } else if !hits.is_empty() {
                truncated = true;
            }
        }

        for optional_block in [skill_section, teaching_section, workflows_section] {
            let sep = if system_parts.is_empty() { 0 } else { 2 };
            let remaining = budget_for_system.saturating_sub(used_chars + sep);
            let block_len = optional_block.chars().count();
            if block_len <= remaining {
                system_parts.push(optional_block);
                used_chars += sep + block_len;
            } else {
                truncated = true;
                if remaining >= 80 {
                    let clipped = truncate_chars(&optional_block, remaining);
                    used_chars += sep + clipped.chars().count();
                    system_parts.push(clipped);
                }
            }
        }

        let system_prompt = system_parts.join("\n\n");
        let total_chars = system_prompt.chars().count() + user_chars;
        let estimated_tokens = estimate_tokens(&system_prompt) + estimate_tokens(&user_prompt);
        let included_caveats = system_prompt.contains("教材解释与实际用法的边界");

        AssembledPrompt {
            system_prompt,
            user_prompt,
            included_caveats,
            included_hits,
            total_chars,
            estimated_tokens,
            truncated,
        }
    }
}

pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk_count = 0usize;
    let mut non_cjk_count = 0usize;
    for ch in text.chars() {
        if matches!(
            ch as u32,
            0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF | 0x3000..=0x303F | 0xFF00..=0xFFEF
        ) {
            cjk_count += 1;
        } else {
            non_cjk_count += 1;
        }
    }
    cjk_count + non_cjk_count.div_ceil(4)
}

fn format_hits_section(hits: &[RetrievalHit], max_excerpt_chars: usize) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Retrieved Textbook Evidence (Cite Exact Chapter & Passage ID)");
    for hit in hits {
        let pid = hit.passage_id.as_deref().unwrap_or("n/a");
        let excerpt = truncate_chars(&hit.excerpt, max_excerpt_chars);
        out.push_str(&format!(
            "\n- [{chapter} | {section} | {pid} | score={score:.2}] {excerpt}",
            chapter = hit.chapter,
            section = hit.section,
            pid = pid,
            score = hit.score,
            excerpt = excerpt
        ));
    }
    out
}

fn strip_frontmatter(md: &str) -> &str {
    if let Some(rest) = md.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            return &rest[end + "\n---\n".len()..];
        }
    }
    md
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars <= 3 {
        return text.chars().take(max_chars).collect();
    }
    let mut out: String = text.chars().take(max_chars - 3).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_prompts_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("prompts")
    }

    fn sample_hits() -> Vec<RetrievalHit> {
        vec![RetrievalHit {
            chapter: "ch25".into(),
            section: "Dangling Modifier 的错误".into(),
            passage_id: Some("ch25-b0118".into()),
            excerpt: "副词子句减化时，副词子句的主词必须与主要子句的主词相同。".into(),
            score: 14.2,
        }]
    }

    #[test]
    fn always_includes_book_caveats_and_hits() {
        let builder = PromptBuilder::from_dir(&workspace_prompts_dir(), PromptConfig::default())
            .expect("load prompts");
        let assembled = builder.build("While walking home, the rain started.", &sample_hits());

        assert!(assembled.included_caveats, "book-caveats.md must always be included");
        assert!(assembled.system_prompt.contains("教材解释与实际用法的边界"));
        assert!(assembled.system_prompt.contains("减化不能一律机械执行"));
        assert!(assembled.system_prompt.contains("ch25-b0118"));
        assert!(assembled.system_prompt.contains("StructuredGrammarCard"));
        assert!(assembled.user_prompt.contains("While walking home, the rain started."));
        assert_eq!(assembled.included_hits, 1);
        assert!(assembled.total_chars <= PromptConfig::default().max_total_chars);
    }

    #[test]
    fn enforces_strict_char_cap_while_preserving_caveats() {
        let tight_cap = 3_000;
        let builder = PromptBuilder::from_dir(
            &workspace_prompts_dir(),
            PromptConfig {
                max_total_chars: tight_cap,
                max_hit_excerpt_chars: 200,
            },
        )
        .expect("load prompts");

        let assembled = builder.build("The book Maya recommended won a prize.", &sample_hits());

        assert!(assembled.truncated, "expected tight cap to trigger truncation");
        assert!(
            assembled.total_chars <= tight_cap,
            "total_chars {} exceeded cap {}",
            assembled.total_chars,
            tight_cap
        );
        assert!(
            assembled.included_caveats,
            "book-caveats.md must remain present even under tight cap"
        );
        assert!(assembled.estimated_tokens > 0);
    }

    #[test]
    fn builds_cleanly_with_empty_hits_and_still_includes_caveats() {
        let builder = PromptBuilder::from_dir(&workspace_prompts_dir(), PromptConfig::default())
            .expect("load prompts");
        let assembled = builder.build("Hello world.", &[]);

        assert!(assembled.included_caveats);
        assert_eq!(assembled.included_hits, 0);
        assert!(assembled.total_chars <= PromptConfig::default().max_total_chars);
    }
}
