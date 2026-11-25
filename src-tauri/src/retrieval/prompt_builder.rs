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

const HITS_HEADING: &str = "## Retrieved Textbook Evidence (Cite Exact Chapter & Passage ID)";
const USER_PROMPT_PREFIX: &str =
    "Analyze the following English text and return ONLY valid JSON matching the schema:\n\n";

impl PromptBuilder {
    pub fn new(sources: PromptSources, config: PromptConfig) -> Self {
        Self { sources, config }
    }

    pub fn from_dir(prompts_dir: &Path, config: PromptConfig) -> Result<Self, PromptError> {
        let sources = PromptSources::load_from_dir(prompts_dir)?;
        Ok(Self::new(sources, config))
    }

    pub fn build(&self, source_text: &str, hits: &[RetrievalHit]) -> AssembledPrompt {
        let caveats_body = self.sources.book_caveats_md.trim();
        let caveats_section = format!("## Mandatory Book Caveats (Always Apply)\n{caveats_body}");
        let schema_section = OUTPUT_SCHEMA_BLOCK.to_string();

        let skill_section = format!(
            "## Core Instructions (SKILL.md)\n{}",
            strip_frontmatter(&self.sources.skill_md).trim()
        );
        let workflows_section = format!(
            "## Task Workflows (workflows.md)\n{}",
            self.sources.workflows_md.trim()
        );
        let teaching_section = format!(
            "## Teaching Method (teaching-method.md)\n{}",
            self.sources.teaching_method_md.trim()
        );

        let max_total_chars = self.config.max_total_chars;

        // Priority order for budget allocation:
        // 1. Mandatory core: book-caveats.md + OUTPUT_SCHEMA_BLOCK
        // 2. user_prompt (truncating source_text on a char boundary if needed)
        // 3. Retrieved textbook hits (only complete hits; never a dangling heading)
        // 4. Optional guidance: SKILL.md, workflows.md, teaching-method.md
        let mut system_parts: Vec<String> = Vec::with_capacity(6);
        let mut used_system_chars = 0usize;
        let mut truncated = false;

        let mut push_mandatory = |block: &str| {
            let sep = if system_parts.is_empty() { 0 } else { 2 };
            let remaining = max_total_chars
                .saturating_sub(used_system_chars)
                .saturating_sub(sep);
            let block_len = block.chars().count();
            if block_len <= remaining {
                system_parts.push(block.to_string());
                used_system_chars += sep + block_len;
            } else {
                truncated = true;
                if remaining > 0 {
                    let clipped = truncate_chars(block, remaining);
                    used_system_chars += sep + clipped.chars().count();
                    system_parts.push(clipped);
                }
            }
        };

        // 1. Reserve budget for mandatory core first so long source_text can never starve it.
        push_mandatory(&caveats_section);
        push_mandatory(&schema_section);

        // 2. Allocate remaining budget to user_prompt.
        let full_user_prompt = format!("{USER_PROMPT_PREFIX}{}", source_text.trim());
        let remaining_for_user = max_total_chars.saturating_sub(used_system_chars);
        let user_prompt = if full_user_prompt.chars().count() <= remaining_for_user {
            full_user_prompt
        } else {
            truncated = true;
            truncate_chars(&full_user_prompt, remaining_for_user)
        };
        let user_chars = user_prompt.chars().count();
        let budget_for_system = max_total_chars.saturating_sub(user_chars);

        // 3. Allocate remaining budget to complete textbook hits.
        let mut included_hits = 0usize;
        if !hits.is_empty() {
            let sep = if system_parts.is_empty() { 0 } else { 2 };
            let remaining = budget_for_system
                .saturating_sub(used_system_chars)
                .saturating_sub(sep);
            let mut section = String::from(HITS_HEADING);
            let mut section_chars = HITS_HEADING.chars().count();

            for hit in hits {
                let line = format_hit_line(hit, self.config.max_hit_excerpt_chars);
                let line_chars = line.chars().count();
                if section_chars + line_chars <= remaining {
                    section.push_str(&line);
                    section_chars += line_chars;
                    included_hits += 1;
                } else {
                    truncated = true;
                    break;
                }
            }

            if included_hits > 0 {
                system_parts.push(section);
                used_system_chars += sep + section_chars;
            }
        }

        // 4. Allocate remaining budget to optional guidance.
        for optional_block in [skill_section, workflows_section, teaching_section] {
            let sep = if system_parts.is_empty() { 0 } else { 2 };
            let remaining = budget_for_system
                .saturating_sub(used_system_chars)
                .saturating_sub(sep);
            let block_len = optional_block.chars().count();
            if block_len <= remaining {
                system_parts.push(optional_block);
                used_system_chars += sep + block_len;
            } else {
                truncated = true;
                if remaining >= 80 {
                    let clipped = truncate_chars(&optional_block, remaining);
                    used_system_chars += sep + clipped.chars().count();
                    system_parts.push(clipped);
                }
            }
        }

        let system_prompt = system_parts.join("\n\n");
        let total_chars = system_prompt.chars().count() + user_chars;
        let estimated_tokens = estimate_tokens(&system_prompt) + estimate_tokens(&user_prompt);
        let included_caveats = !caveats_body.is_empty() && system_prompt.contains(caveats_body);

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

fn format_hit_line(hit: &RetrievalHit, max_excerpt_chars: usize) -> String {
    let pid = hit.passage_id.as_deref().unwrap_or("n/a");
    let excerpt = truncate_chars(&hit.excerpt, max_excerpt_chars);
    format!(
        "\n- [{chapter} | {section} | {pid} | score={score:.2}] {excerpt}",
        chapter = hit.chapter,
        section = hit.section,
        pid = pid,
        score = hit.score,
        excerpt = excerpt
    )
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

    #[test]
    fn long_source_text_preserves_mandatory_core_and_respects_char_cap() {
        let sources = PromptSources::load_from_dir(&workspace_prompts_dir()).expect("load prompts");
        let cap = 5_000;
        let builder = PromptBuilder::new(
            sources.clone(),
            PromptConfig {
                max_total_chars: cap,
                max_hit_excerpt_chars: 200,
            },
        );
        let huge_source = "The complex English sentence — 包含中文与多字节字符 — keeps going. ".repeat(400);

        let assembled = builder.build(&huge_source, &sample_hits());

        assert!(assembled.truncated);
        assert!(
            assembled.total_chars <= cap,
            "total_chars {} exceeded cap {}",
            assembled.total_chars,
            cap
        );
        assert!(
            assembled.included_caveats,
            "full book-caveats.md must be preserved even when source_text is huge"
        );
        assert!(assembled.system_prompt.contains(sources.book_caveats_md.trim()));
        assert!(assembled.system_prompt.contains(OUTPUT_SCHEMA_BLOCK));
        assert!(!assembled.user_prompt.is_empty());
        assert!(assembled.user_prompt.ends_with("..."));
    }

    #[test]
    fn tiny_cap_truncating_caveats_reports_included_caveats_false() {
        // 500 chars is large enough to include the first heading of book-caveats.md
        // ("教材解释与实际用法的边界"), yet far smaller than the complete caveats file.
        let tiny_cap = 500;
        let builder = PromptBuilder::from_dir(
            &workspace_prompts_dir(),
            PromptConfig {
                max_total_chars: tiny_cap,
                max_hit_excerpt_chars: 200,
            },
        )
        .expect("load prompts");

        let assembled = builder.build("Hello world.", &sample_hits());

        assert!(assembled.truncated);
        assert!(assembled.total_chars <= tiny_cap);
        assert!(
            assembled.system_prompt.contains("教材解释与实际用法的边界"),
            "test precondition: heading fits inside tiny_cap"
        );
        assert!(
            !assembled.included_caveats,
            "truncated caveats must report included_caveats = false"
        );
        assert_eq!(assembled.included_hits, 0);
        assert!(!assembled.system_prompt.contains(HITS_HEADING));
    }

    #[test]
    fn included_caveats_checks_actual_caveats_source_not_hardcoded_phrase() {
        let sources = PromptSources {
            skill_md: "Skill guidance.".into(),
            workflows_md: "Workflow guidance.".into(),
            teaching_method_md: "Teaching guidance.".into(),
            book_caveats_md: "Custom caveat rule: never invent grammar terms.".into(),
        };

        let ample = PromptBuilder::new(
            sources.clone(),
            PromptConfig {
                max_total_chars: 2_000,
                max_hit_excerpt_chars: 200,
            },
        )
        .build("Cats sleep.", &[]);
        assert!(ample.included_caveats);

        let tiny = PromptBuilder::new(
            sources,
            PromptConfig {
                max_total_chars: 60,
                max_hit_excerpt_chars: 200,
            },
        )
        .build("Cats sleep.", &[]);
        assert!(!tiny.included_caveats);
        assert!(tiny.total_chars <= 60);
    }

    #[test]
    fn no_dangling_hits_heading_and_exact_included_hits_count() {
        let sources = PromptSources {
            skill_md: String::new(),
            workflows_md: String::new(),
            teaching_method_md: String::new(),
            book_caveats_md: "Caveat body.".into(),
        };
        let hits = vec![
            RetrievalHit {
                chapter: "ch25".into(),
                section: "Dangling Modifier".into(),
                passage_id: Some("ch25-b0118".into()),
                excerpt: "First hit excerpt about dangling modifiers.".into(),
                score: 14.2,
            },
            RetrievalHit {
                chapter: "ch09".into(),
                section: "Relative Clauses".into(),
                passage_id: Some("ch09-b0042".into()),
                excerpt: "Second hit excerpt about relative clauses.".into(),
                score: 9.8,
            },
        ];
        let source_text = "Short input.";

        // Compute exact base size of mandatory core + user_prompt.
        let base = PromptBuilder::new(
            sources.clone(),
            PromptConfig {
                max_total_chars: 10_000,
                max_hit_excerpt_chars: 200,
            },
        )
        .build(source_text, &[]);
        let base_chars = base.system_prompt.chars().count()
            - "## Core Instructions (SKILL.md)\n".chars().count()
            - "## Task Workflows (workflows.md)\n".chars().count()
            - "## Teaching Method (teaching-method.md)\n".chars().count()
            - 6 // three "\n\n" separators for the three empty optional sections
            + base.user_prompt.chars().count();

        // Case A: enough room for the hits heading alone, but NOT for heading + 1st complete hit.
        let cap_zero_hits = base_chars + 2 + HITS_HEADING.chars().count() + 10;
        let zero_fit = PromptBuilder::new(
            sources.clone(),
            PromptConfig {
                max_total_chars: cap_zero_hits,
                max_hit_excerpt_chars: 200,
            },
        )
        .build(source_text, &hits);

        assert_eq!(zero_fit.included_hits, 0);
        assert!(
            !zero_fit.system_prompt.contains(HITS_HEADING),
            "must not emit dangling hits heading when 0 hits fit"
        );
        assert!(zero_fit.truncated);
        assert!(zero_fit.total_chars <= cap_zero_hits);

        // Case B: enough room for heading + 1st complete hit, but NOT 2nd hit.
        let hit1_line = format_hit_line(&hits[0], 200);
        let cap_one_hit = base_chars + 2 + HITS_HEADING.chars().count() + hit1_line.chars().count();
        let one_fit = PromptBuilder::new(
            sources,
            PromptConfig {
                max_total_chars: cap_one_hit,
                max_hit_excerpt_chars: 200,
            },
        )
        .build(source_text, &hits);

        assert_eq!(one_fit.included_hits, 1);
        assert!(one_fit.system_prompt.contains(HITS_HEADING));
        assert!(one_fit.system_prompt.contains("ch25-b0118"));
        assert!(!one_fit.system_prompt.contains("ch09-b0042"));
        assert!(one_fit.truncated);
        assert!(one_fit.total_chars <= cap_one_hit);
    }
}
