use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::types::RetrievalHit;

const BM25_K1: f32 = 1.5;
const BM25_B: f32 = 0.75;
const HEADING_TERM_WEIGHT: u32 = 3;
const ROUTE_TERM_WEIGHT: u32 = 2;
const EXCERPT_MAX_CHARS: usize = 320;

pub trait BookRetriever: Send + Sync {
    fn retrieve(&self, source_text: &str, limit: usize) -> Vec<RetrievalHit>;
}

#[derive(Debug)]
pub enum RetrievalError {
    Io { path: PathBuf, source: io::Error },
    EmptyCorpus { book_dir: PathBuf },
}

impl fmt::Display for RetrievalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read {}: {}", path.display(), source)
            }
            Self::EmptyCorpus { book_dir } => {
                write!(f, "no textbook chunks found under {}", book_dir.display())
            }
        }
    }
}

impl std::error::Error for RetrievalError {}

#[derive(Debug, Clone)]
struct SectionDoc {
    chapter: String,
    section: String,
    passage_id: Option<String>,
    excerpt: String,
    doc_len: u32,
}

#[derive(Debug, Clone)]
struct Posting {
    doc_id: usize,
    tf: u32,
}

#[derive(Debug)]
pub struct Bm25BookRetriever {
    docs: Vec<SectionDoc>,
    postings: HashMap<String, Vec<Posting>>,
    idf: HashMap<String, f32>,
    avg_doc_len: f32,
    chapter_routes: HashMap<String, Vec<String>>,
}

impl Bm25BookRetriever {
    pub fn from_book_dir(book_dir: &Path) -> Result<Self, RetrievalError> {
        let index_path = book_dir.join("index.md");
        let index_content = fs::read_to_string(&index_path).map_err(|source| RetrievalError::Io {
            path: index_path.clone(),
            source,
        })?;
        let chapter_routes = parse_index_topic_routes(&index_content);

        let chapters_dir = book_dir.join("chapters");
        let mut chapter_paths: Vec<PathBuf> = fs::read_dir(&chapters_dir)
            .map_err(|source| RetrievalError::Io {
                path: chapters_dir.clone(),
                source,
            })?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        chapter_paths.sort();

        let mut docs = Vec::with_capacity(600);
        let mut postings: HashMap<String, Vec<Posting>> = HashMap::with_capacity(8192);
        let mut total_doc_len: u64 = 0;

        for chapter_path in chapter_paths {
            let chapter_id = chapter_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let content =
                fs::read_to_string(&chapter_path).map_err(|source| RetrievalError::Io {
                    path: chapter_path.clone(),
                    source,
                })?;

            let route_terms = chapter_routes
                .get(&chapter_id)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            let sections = parse_chapter_sections(&chapter_id, &content);
            for raw in sections {
                let mut tf_map: HashMap<String, u32> = HashMap::new();

                for token in tokenize(&raw.body_plain) {
                    *tf_map.entry(token).or_insert(0) += 1;
                }
                for token in tokenize(&raw.section_title) {
                    *tf_map.entry(token).or_insert(0) += HEADING_TERM_WEIGHT;
                }
                for token in tokenize(&raw.chapter_title) {
                    *tf_map.entry(token).or_insert(0) += 1;
                }
                // Include chapter identifier itself so explicit queries like "ch19" match.
                *tf_map.entry(chapter_id.to_ascii_lowercase()).or_insert(0) += HEADING_TERM_WEIGHT;

                // Attach route terms with light weight so chapter-level topic keywords match sections.
                for route_token in route_terms {
                    *tf_map.entry(route_token.clone()).or_insert(0) += 1;
                }

                let doc_len: u32 = tf_map.values().sum();
                if doc_len == 0 {
                    continue;
                }

                let doc_id = docs.len();
                total_doc_len += u64::from(doc_len);

                for (term, tf) in tf_map {
                    postings.entry(term).or_default().push(Posting { doc_id, tf });
                }

                docs.push(SectionDoc {
                    chapter: chapter_id.clone(),
                    section: raw.section_title,
                    passage_id: raw.passage_id,
                    excerpt: build_excerpt(&raw.body_plain, EXCERPT_MAX_CHARS),
                    doc_len,
                });
            }
        }

        if docs.is_empty() {
            return Err(RetrievalError::EmptyCorpus {
                book_dir: book_dir.to_path_buf(),
            });
        }

        let num_docs = docs.len() as f32;
        let avg_doc_len = (total_doc_len as f32) / num_docs;

        let mut idf = HashMap::with_capacity(postings.len());
        for (term, list) in &postings {
            let df = list.len() as f32;
            let score = ((num_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
            idf.insert(term.clone(), score);
        }

        Ok(Self {
            docs,
            postings,
            idf,
            avg_doc_len,
            chapter_routes,
        })
    }

    pub fn document_count(&self) -> usize {
        self.docs.len()
    }

    pub fn route_count(&self) -> usize {
        self.chapter_routes.len()
    }
}

impl BookRetriever for Bm25BookRetriever {
    fn retrieve(&self, source_text: &str, limit: usize) -> Vec<RetrievalHit> {
        if limit == 0 || source_text.trim().is_empty() || self.docs.is_empty() {
            return Vec::new();
        }

        let mut weighted_query_terms: HashMap<String, f32> = HashMap::new();

        // 1. Direct tokens from the user's input text.
        for token in tokenize(source_text) {
            if !is_english_stopword(&token) {
                *weighted_query_terms.entry(token).or_insert(0.0) += 1.0;
            }
        }

        // 2. Bilingual grammar pattern triggers derived from English surface syntax.
        let triggers = extract_grammar_triggers(source_text);
        let mut chapter_boosts: HashMap<&'static str, f32> = HashMap::new();
        for trigger in triggers {
            for token in tokenize(trigger.term) {
                *weighted_query_terms.entry(token).or_insert(0.0) += trigger.weight;
            }
            if let Some(ch) = trigger.chapter_hint {
                *chapter_boosts.entry(ch).or_insert(0.0) += trigger.chapter_boost;
            }
        }

        // 3. Topic-route chapter boost when direct query tokens overlap a chapter's index.md keywords.
        let mut route_chapter_boosts: HashMap<&str, f32> = HashMap::new();
        for (chapter, route_tokens) in &self.chapter_routes {
            let mut matched = 0u32;
            for rt in route_tokens {
                if let Some(&w) = weighted_query_terms.get(rt) {
                    if w > 0.0 {
                        matched += 1;
                    }
                }
            }
            if matched > 0 {
                route_chapter_boosts.insert(
                    chapter.as_str(),
                    (matched as f32).min(6.0) * (ROUTE_TERM_WEIGHT as f32) * 0.35,
                );
            }
        }

        // 4. Accumulate BM25 scores across postings.
        let mut scores: Vec<f32> = vec![0.0; self.docs.len()];
        for (term, query_weight) in &weighted_query_terms {
            let Some(list) = self.postings.get(term) else {
                continue;
            };
            let idf = *self.idf.get(term).unwrap_or(&0.0);
            if idf <= 0.0 {
                continue;
            }
            for posting in list {
                let doc_len = self.docs[posting.doc_id].doc_len as f32;
                let tf = posting.tf as f32;
                let denom = tf + BM25_K1 * (1.0 - BM25_B + BM25_B * (doc_len / self.avg_doc_len));
                let bm25 = idf * ((tf * (BM25_K1 + 1.0)) / denom);
                scores[posting.doc_id] += bm25 * query_weight;
            }
        }

        // 5. Apply chapter-level grammar trigger & topic-route boosts, and downweight non-chapter frontmatter.
        let mut ranked: Vec<(usize, f32)> = Vec::with_capacity(self.docs.len());
        for (doc_id, base_score) in scores.into_iter().enumerate() {
            let doc = &self.docs[doc_id];
            let mut score = base_score;

            if let Some(&boost) = chapter_boosts.get(doc.chapter.as_str()) {
                if score > 0.0 {
                    score += boost;
                } else {
                    score += boost * 0.25;
                }
            }
            if let Some(&rboost) = route_chapter_boosts.get(doc.chapter.as_str()) {
                if score > 0.0 {
                    score += rboost;
                }
            }

            // Slightly downweight frontmatter/appendix unless explicitly targeted.
            if !doc.chapter.starts_with("ch") {
                score *= 0.35;
            }

            if score > 0.0 {
                ranked.push((doc_id, score));
            }
        }

        // Deterministic sort: score descending, then chapter, then passage_id, then doc_id.
        ranked.sort_by(|(id_a, score_a), (id_b, score_b)| {
            score_b
                .total_cmp(score_a)
                .then_with(|| self.docs[*id_a].chapter.cmp(&self.docs[*id_b].chapter))
                .then_with(|| self.docs[*id_a].passage_id.cmp(&self.docs[*id_b].passage_id))
                .then_with(|| id_a.cmp(id_b))
        });

        // Deduplicate by (chapter, section) and keep chapter diversity (max 2 per chapter on pass 1)
        // so multi-phenomenon sentences surface both relevant chapters.
        let mut hits = Vec::with_capacity(limit);
        let mut seen: HashSet<(&str, &str)> = HashSet::new();
        let mut chapter_counts: HashMap<&str, usize> = HashMap::new();
        let max_per_chapter = if limit >= 3 { 2 } else { limit };
        let mut overflow: Vec<(usize, f32)> = Vec::new();

        for (doc_id, score) in ranked {
            let doc = &self.docs[doc_id];
            if !seen.insert((doc.chapter.as_str(), doc.section.as_str())) {
                continue;
            }
            let count = chapter_counts.entry(doc.chapter.as_str()).or_insert(0);
            if *count >= max_per_chapter {
                overflow.push((doc_id, score));
                continue;
            }
            *count += 1;
            hits.push(RetrievalHit {
                chapter: doc.chapter.clone(),
                section: doc.section.clone(),
                passage_id: doc.passage_id.clone(),
                excerpt: doc.excerpt.clone(),
                score: (score * 1000.0).round() / 1000.0,
            });
            if hits.len() >= limit {
                break;
            }
        }

        if hits.len() < limit {
            for (doc_id, score) in overflow {
                let doc = &self.docs[doc_id];
                hits.push(RetrievalHit {
                    chapter: doc.chapter.clone(),
                    section: doc.section.clone(),
                    passage_id: doc.passage_id.clone(),
                    excerpt: doc.excerpt.clone(),
                    score: (score * 1000.0).round() / 1000.0,
                });
                if hits.len() >= limit {
                    break;
                }
            }
        }

        hits
    }
}

struct RawSection {
    chapter_title: String,
    section_title: String,
    passage_id: Option<String>,
    body_plain: String,
}

fn parse_index_topic_routes(index_md: &str) -> HashMap<String, Vec<String>> {
    let mut routes: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_table = false;

    for line in index_md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Topic routes") {
            in_table = true;
            continue;
        }
        if in_table && trimmed.starts_with("## ") {
            break;
        }
        if !in_table || !trimmed.starts_with('|') {
            continue;
        }
        let cols: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cols.len() < 2 {
            continue;
        }
        if cols[0].starts_with("---") || cols[0].contains("主题、关键词") {
            continue;
        }
        if let Some(chapter_id) = extract_chapter_id_from_link(cols[1]) {
            let mut tokens = tokenize(cols[0]);
            if let Some(notes) = cols.get(2) {
                tokens.extend(tokenize(notes));
            }
            tokens.sort();
            tokens.dedup();
            routes.entry(chapter_id).or_default().extend(tokens);
        }
    }

    for tokens in routes.values_mut() {
        tokens.sort();
        tokens.dedup();
    }

    routes
}

fn extract_chapter_id_from_link(cell: &str) -> Option<String> {
    // Matches `[ch19](chapters/ch19.md)`
    let start = cell.find("chapters/")? + "chapters/".len();
    let rest = &cell[start..];
    let end = rest.find(".md")?;
    let id = rest[..end].trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn parse_chapter_sections(chapter_id: &str, content: &str) -> Vec<RawSection> {
    let mut sections = Vec::new();
    let mut chapter_title = chapter_id.to_string();
    let mut h2_title: Option<String> = None;
    let mut h3_title: Option<String> = None;

    let mut current_title = chapter_id.to_string();
    let mut current_passage_id: Option<String> = None;
    let mut current_body = String::new();
    let mut pending_passage_id: Option<String> = None;

    let flush = |sections: &mut Vec<RawSection>,
                 chapter_title: &str,
                 section_title: &str,
                 passage_id: &Option<String>,
                 body: &mut String| {
        let trimmed = body.trim();
        if !trimmed.is_empty() {
            sections.push(RawSection {
                chapter_title: chapter_title.to_string(),
                section_title: section_title.to_string(),
                passage_id: passage_id.clone(),
                body_plain: trimmed.to_string(),
            });
        }
        body.clear();
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("> 教材原文") {
            continue;
        }
        if let Some(pid) = parse_passage_anchor(trimmed) {
            pending_passage_id = Some(pid);
            continue;
        }
        if trimmed.starts_with("<!-- ") && trimmed.ends_with(" -->") {
            continue;
        }

        if let Some((level, heading_text)) = parse_markdown_heading(trimmed) {
            flush(
                &mut sections,
                &chapter_title,
                &current_title,
                &current_passage_id,
                &mut current_body,
            );

            let clean_heading = strip_html_tags(heading_text);
            match level {
                1 => {
                    chapter_title = clean_heading.clone();
                    h2_title = None;
                    h3_title = None;
                    current_title = clean_heading;
                }
                2 => {
                    h2_title = Some(clean_heading.clone());
                    h3_title = None;
                    current_title = clean_heading;
                }
                3 => {
                    h3_title = Some(clean_heading.clone());
                    current_title = match &h2_title {
                        Some(p) => format!("{p} > {clean_heading}"),
                        None => clean_heading,
                    };
                }
                _ => {
                    current_title = match (&h2_title, &h3_title) {
                        (_, Some(p3)) => format!("{p3} > {clean_heading}"),
                        (Some(p2), None) => format!("{p2} > {clean_heading}"),
                        (None, None) => clean_heading,
                    };
                }
            }
            current_passage_id = pending_passage_id.take();
            continue;
        }

        if current_passage_id.is_none() {
            current_passage_id = pending_passage_id.take();
        } else {
            pending_passage_id = None;
        }

        let plain = strip_html_tags(trimmed);
        if !plain.is_empty() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(&plain);
        }
    }

    flush(
        &mut sections,
        &chapter_title,
        &current_title,
        &current_passage_id,
        &mut current_body,
    );

    sections
}

fn parse_passage_anchor(line: &str) -> Option<String> {
    let prefix = "<a id=\"";
    if !line.starts_with(prefix) {
        return None;
    }
    let rest = &line[prefix.len()..];
    let end = rest.find("\"></a>")?;
    let id = &rest[..end];
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn parse_markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = line.get(hashes..)?;
    if !rest.starts_with(' ') {
        return None;
    }
    Some((hashes, rest.trim()))
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_excerpt(body: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in body.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii_buf = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    let flush_ascii = |buf: &mut String, tokens: &mut Vec<String>| {
        if buf.len() >= 2 {
            tokens.push(std::mem::take(buf));
        } else {
            buf.clear();
        }
    };

    let flush_cjk = |run: &mut Vec<char>, tokens: &mut Vec<String>| {
        if run.is_empty() {
            return;
        }
        for &ch in run.iter() {
            tokens.push(ch.to_string());
        }
        for win in run.windows(2) {
            let mut bigram = String::with_capacity(6);
            bigram.push(win[0]);
            bigram.push(win[1]);
            tokens.push(bigram);
        }
        run.clear();
    };

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk_run, &mut tokens);
            ascii_buf.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_ascii(&mut ascii_buf, &mut tokens);
            cjk_run.push(normalize_cjk_char(ch));
        } else if ch == '\'' && !ascii_buf.is_empty() {
            // Keep contractions contiguous (e.g. shouldn't -> shouldnt).
        } else {
            flush_ascii(&mut ascii_buf, &mut tokens);
            flush_cjk(&mut cjk_run, &mut tokens);
        }
    }

    flush_ascii(&mut ascii_buf, &mut tokens);
    flush_cjk(&mut cjk_run, &mut tokens);
    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF
    )
}

fn normalize_cjk_char(ch: char) -> char {
    match ch {
        '詞' => '词',
        '語' => '语',
        '動' => '动',
        '從' => '从',
        '關' => '关',
        '係' => '系',
        '簡' => '简',
        '減' => '减',
        '時' => '时',
        '態' => '态',
        '補' => '补',
        '飾' => '饰',
        '複' => '复',
        '數' => '数',
        '單' => '单',
        '問' => '问',
        '裝' => '装',
        '條' => '条',
        '設' => '设',
        '氣' => '气',
        '發' => '发',
        '現' => '现',
        '過' => '过',
        '來' => '来',
        '與' => '与',
        '為' => '为',
        '對' => '对',
        '經' => '经',
        '應' => '应',
        '當' => '当',
        '會' => '会',
        '這' => '这',
        '個' => '个',
        '們' => '们',
        '點' => '点',
        '節' => '节',
        '類' => '类',
        '種' => '种',
        '體' => '体',
        '義' => '义',
        '實' => '实',
        '際' => '际',
        '構' => '构',
        '組' => '组',
        '連' => '连',
        '轉' => '转',
        '變' => '变',
        '換' => '换',
        '選' => '选',
        '擇' => '择',
        '區' => '区',
        '別' => '别',
        '較' => '较',
        '強' => '强',
        '調' => '调',
        '處' => '处',
        '開' => '开',
        '無' => '无',
        '須' => '须',
        '雖' => '虽',
        '讓' => '让',
        '說' => '说',
        '話' => '话',
        '讀' => '读',
        '寫' => '写',
        '學' => '学',
        '習' => '习',
        '練' => '练',
        '題' => '题',
        '錯' => '错',
        '誤' => '误',
        _ => ch,
    }
}

fn is_english_stopword(token: &str) -> bool {
    matches!(
        token,
        "the" | "an" | "is" | "are" | "was" | "were" | "be" | "been" | "being"
            | "it" | "its" | "this" | "these" | "those" | "he" | "she" | "they"
            | "we" | "you" | "his" | "her" | "their" | "our" | "my" | "your"
            | "of" | "to" | "in" | "on" | "at" | "for" | "with" | "by" | "from"
            | "and" | "or" | "but" | "as" | "not" | "no"
    )
}

#[derive(Debug, Clone, Copy)]
struct GrammarTrigger {
    term: &'static str,
    weight: f32,
    chapter_hint: Option<&'static str>,
    chapter_boost: f32,
}

impl GrammarTrigger {
    const fn new(
        term: &'static str,
        weight: f32,
        chapter_hint: Option<&'static str>,
        chapter_boost: f32,
    ) -> Self {
        Self {
            term,
            weight,
            chapter_hint,
            chapter_boost,
        }
    }
}

fn extract_grammar_triggers(source_text: &str) -> Vec<GrammarTrigger> {
    let mut triggers = Vec::new();
    let lower = source_text.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .collect();
    let raw_words: Vec<&str> = source_text
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .collect();

    if words.is_empty() {
        return triggers;
    }

    let has_comma = source_text.contains(',');

    // 1. Reduced adverbial clause / dangling modifier (ch25)
    let mut matched_reduced_adv = false;
    for i in 0..words.len() {
        let w = words[i];
        if matches!(
            w,
            "while" | "when" | "although" | "though" | "after" | "before" | "if" | "unless" | "once"
        ) {
            if let Some(&next) = words.get(i + 1) {
                let next2 = words.get(i + 2).copied();
                let is_participle_after = is_ving(next)
                    || is_past_participle(next)
                    || (next.ends_with("ly")
                        && next2.is_some_and(|n2| is_ving(n2) || is_past_participle(n2)))
                    || (next == "not"
                        && next2.is_some_and(|n2| is_ving(n2) || is_past_participle(n2)));

                if is_participle_after {
                    matched_reduced_adv = true;
                    triggers.push(GrammarTrigger::new(
                        "副词子句减化 分词构句 dangling modifier 悬垂 主词相同",
                        2.8,
                        Some("ch25"),
                        8.0,
                    ));
                    if is_ving(next) || next2.is_some_and(is_ving) {
                        triggers.push(GrammarTrigger::new(
                            "现在分词构句 ving while walking",
                            2.2,
                            Some("ch25"),
                            4.0,
                        ));
                    } else {
                        triggers.push(GrammarTrigger::new(
                            "过去分词构句 ven",
                            2.2,
                            Some("ch25"),
                            4.0,
                        ));
                    }
                }
            }
        }
    }

    // Sentence-initial participle clause with comma: "Walking home, ..." / "Having finished ..., ..."
    if !matched_reduced_adv
        && has_comma
        && (is_ving(words[0])
            || words[0] == "having"
            || words[0] == "being"
            || (words[0] == "not" && words.get(1).copied().is_some_and(is_ving)))
    {
        triggers.push(GrammarTrigger::new(
            "副词子句减化 分词构句 现在分词构句 句首 dangling modifier",
            2.4,
            Some("ch25"),
            6.5,
        ));
    }

    // Reduced adverbial infinitive (ch25)
    if lower.contains("in order to")
        || lower.contains("so as to")
        || lower.contains("only to ")
        || (words.contains(&"too") && words.contains(&"to"))
        || (words.contains(&"enough") && words.contains(&"to"))
    {
        triggers.push(GrammarTrigger::new(
            "副词子句减化 简化为 to in order to so as to",
            2.0,
            Some("ch25"),
            4.5,
        ));
    }

    // 2. Relative clause: explicit wh-/that or zero object relative (ch19)
    let has_explicit_rel = words.iter().enumerate().any(|(i, &w)| {
        i > 0 && matches!(w, "who" | "whom" | "whose" | "which")
    });
    if has_explicit_rel {
        triggers.push(GrammarTrigger::new(
            "关系子句 关系代名词 先行词 who which that",
            2.4,
            Some("ch19"),
            6.5,
        ));
        if lower.contains(", which") {
            triggers.push(GrammarTrigger::new(
                "主要子句为先行词 关系子句 which",
                2.5,
                Some("ch19"),
                5.0,
            ));
        }
    }

    if detect_zero_relative_clause(&words, &raw_words) {
        triggers.push(GrammarTrigger::new(
            "关系子句 关系代名词 受词位置 省略 先行词 which that",
            3.0,
            Some("ch19"),
            9.0,
        ));
    }

    // 3. Reduced relative clause: Det + Noun + Ving / Ven + prep (ch23)
    if detect_reduced_relative_clause(&words) {
        triggers.push(GrammarTrigger::new(
            "关系从句减化 简化为分词 现在分词 过去分词 后置分词",
            2.6,
            Some("ch23"),
            7.5,
        ));
    }

    // 4. Gerund vs Infinitive (ch13, ch14)
    if lower.contains("used to") {
        let is_be_used_to = lower.contains("am used to")
            || lower.contains("is used to")
            || lower.contains("are used to")
            || lower.contains("was used to")
            || lower.contains("were used to")
            || lower.contains("be used to")
            || lower.contains("get used to")
            || lower.contains("got used to");
        if is_be_used_to {
            triggers.push(GrammarTrigger::new(
                "表示不定词的 to 与介系词 to 的区别 动名词 介系词的受词",
                2.8,
                Some("ch14"),
                7.5,
            ));
            triggers.push(GrammarTrigger::new("动名词 介系词的受词", 1.8, Some("ch13"), 4.0));
        } else {
            triggers.push(GrammarTrigger::new(
                "不定词片语 语气助动词 used to",
                2.2,
                Some("ch14"),
                5.0,
            ));
        }
    }

    for i in 0..words.len().saturating_sub(1) {
        let w = words[i];
        let next = words[i + 1];
        if is_gerund_or_inf_verb(w) {
            if is_ving(next) {
                triggers.push(GrammarTrigger::new(
                    "动名词 动名词与不定词片语的选择 stop doing remember doing",
                    2.6,
                    Some("ch13"),
                    7.0,
                ));
                triggers.push(GrammarTrigger::new(
                    "不定词与动名词皆可 但意思不同",
                    2.0,
                    Some("ch14"),
                    4.5,
                ));
            } else if next == "to" && words.get(i + 2).is_some() {
                triggers.push(GrammarTrigger::new(
                    "不定词片语 不定词与动名词的比较 stop to do 只能用不定词",
                    2.6,
                    Some("ch14"),
                    7.0,
                ));
            }
        }
    }

    // 5. Passive voice (ch10)
    if detect_passive_voice(&words) {
        triggers.push(GrammarTrigger::new(
            "语态 被动态 被动 passive 基本句型的被动态",
            2.6,
            Some("ch10"),
            7.5,
        ));
    }

    // 6. Tense & Aspect (ch09)
    if detect_progressive(&words) {
        triggers.push(GrammarTrigger::new(
            "动词时态 进行 简单式 现在时间 tense aspect",
            2.0,
            Some("ch09"),
            5.0,
        ));
    }
    if detect_perfect(&words) {
        triggers.push(GrammarTrigger::new(
            "动词时态 完成式 过去时间 现在时间 tense aspect",
            2.2,
            Some("ch09"),
            5.5,
        ));
    }

    // 7. Modal auxiliaries (ch11)
    let has_modal = words.iter().any(|&w| {
        matches!(
            w,
            "must" | "may" | "might" | "should" | "ought" | "could" | "would" | "can" | "shall"
        )
    });
    if has_modal {
        let modal_perfect = words.windows(2).any(|win| {
            matches!(
                win[0],
                "must" | "may" | "might" | "should" | "could" | "would" | "can"
            ) && win[1] == "have"
        });
        if modal_perfect {
            triggers.push(GrammarTrigger::new(
                "语气助动词 猜测用法 一般用法的过去拼法配合完成式 must could may might",
                2.8,
                Some("ch11"),
                8.0,
            ));
        } else {
            triggers.push(GrammarTrigger::new(
                "语气助动词 情态 一般用法 猜测用法",
                1.6,
                Some("ch11"),
                3.5,
            ));
        }
    }

    // 8. Subjunctive / Conditional mood (ch12)
    let has_subjunctive = (words.contains(&"if")
        && (words.contains(&"were")
            || words.contains(&"would")
            || words.contains(&"could")
            || words.contains(&"might")
            || words.contains(&"had")))
        || (words.contains(&"wish")
            && (words.contains(&"were") || words.contains(&"had") || words.contains(&"would")));
    if has_subjunctive {
        triggers.push(GrammarTrigger::new(
            "语气 假设语气 条件语气 subjunctive 现在时间 过去时间",
            2.8,
            Some("ch12"),
            8.0,
        ));
    }

    // 9. Noun clause in subject position (ch17)
    if matches!(
        words[0],
        "what" | "whether" | "that" | "whoever" | "whatever" | "how" | "why"
    ) && words.len() >= 4
        && !source_text.trim_end().ends_with('?')
    {
        triggers.push(GrammarTrigger::new(
            "名词子句 主词位置 wh 子句 that 子句",
            2.8,
            Some("ch17"),
            8.0,
        ));
    }

    // 10. Full adverbial clause (ch18)
    if !matched_reduced_adv
        && words.iter().any(|&w| {
            matches!(
                w,
                "because" | "although" | "though" | "unless" | "whereas" | "until" | "since"
            )
        })
    {
        triggers.push(GrammarTrigger::new(
            "副词子句 常用的副词子句连接词 because although if while",
            2.2,
            Some("ch18"),
            5.5,
        ));
    }

    // 11. Subject-verb agreement (ch20)
    if lower.contains("neither ") && lower.contains(" nor ")
        || lower.contains("either ") && lower.contains(" or ")
        || lower.contains("the number of")
        || lower.contains("a number of")
        || lower.contains("as well as")
        || lower.contains("none of")
        || lower.contains("not only") && lower.contains("but also")
    {
        triggers.push(GrammarTrigger::new(
            "主词动词一致性 主谓一致 单复数 neither nor the number of a number of",
            2.8,
            Some("ch20"),
            8.0,
        ));
    }

    // 12. Inversion (ch21)
    let starts_negative_inversion = matches!(
        words[0],
        "never" | "seldom" | "rarely" | "hardly" | "scarcely" | "barely" | "little" | "nowhere"
    ) && words.get(1).copied().is_some_and(is_auxiliary);
    let starts_so_neither_inv = matches!(words[0], "so" | "neither" | "nor")
        && words.get(1).copied().is_some_and(is_auxiliary);
    let starts_only_not_only_inv = (lower.starts_with("not only ")
        || lower.starts_with("only when ")
        || lower.starts_with("only after ")
        || lower.starts_with("only by ")
        || lower.starts_with("no sooner "))
        && words.iter().skip(2).take(5).any(|&w| is_auxiliary(w));

    if starts_negative_inversion || starts_so_neither_inv || starts_only_not_only_inv {
        triggers.push(GrammarTrigger::new(
            "倒装句 倒装 inversion 助动词或 be 前移 so do i",
            3.0,
            Some("ch21"),
            9.0,
        ));
    }

    // 13. Comparison (ch06)
    if words.contains(&"than")
        || (words.iter().filter(|&&w| w == "as").count() >= 2)
    {
        triggers.push(GrammarTrigger::new(
            "比较句法 比较级 大于 小于 等于 than as 平行对称",
            2.5,
            Some("ch06"),
            7.0,
        ));
    }

    // 14. Phrasal verbs / prepositions (ch07)
    if detect_phrasal_verb(&words) {
        triggers.push(GrammarTrigger::new(
            "介系词 片语动词 介副词 片语动词的种类 及物动词 可以拆开",
            2.6,
            Some("ch07"),
            7.0,
        ));
    }

    // 15. Manner / frequency / focusing adverbs (ch05)
    if words.iter().any(|&w| {
        matches!(
            w,
            "diligently" | "carefully" | "quickly" | "slowly" | "frequently" | "only" | "seldom"
        )
    }) {
        triggers.push(GrammarTrigger::new(
            "副词 方法状态的副词 频率副词 强调范围的副词",
            1.8,
            Some("ch05"),
            4.0,
        ));
    }

    triggers
}

fn detect_zero_relative_clause(words: &[&str], raw_words: &[&str]) -> bool {
    if words.len() < 5 {
        return false;
    }
    for i in 0..words.len().saturating_sub(3) {
        if !is_determiner(words[i]) {
            continue;
        }
        // Noun at i+1 (skip optional adjective if noun is at i+2)
        for noun_offset in [1usize, 2usize] {
            let subj_idx = i + noun_offset + 1;
            let verb_idx = subj_idx + 1;
            if verb_idx >= words.len() {
                continue;
            }
            let noun = words[i + noun_offset];
            if is_determiner(noun) || is_auxiliary(noun) || is_subject_pronoun(noun) {
                continue;
            }
            let subj_lower = words[subj_idx];
            let subj_raw = raw_words[subj_idx];
            let is_embedded_subj = is_subject_pronoun(subj_lower)
                || (subj_idx > 0
                    && subj_raw
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase()));
            if !is_embedded_subj {
                continue;
            }
            let embedded_verb = words[verb_idx];
            if is_likely_finite_verb(embedded_verb) {
                // Confirm there is a later main verb or the embedded clause is an NP fragment.
                let has_later_verb = words[verb_idx + 1..]
                    .iter()
                    .any(|&w| is_likely_finite_verb(w) || is_auxiliary(w));
                if has_later_verb || words.len() <= verb_idx + 2 {
                    return true;
                }
            }
        }
    }
    false
}

fn detect_reduced_relative_clause(words: &[&str]) -> bool {
    if words.len() < 5 {
        return false;
    }
    for i in 0..words.len().saturating_sub(3) {
        if !is_determiner(words[i]) {
            continue;
        }
        let noun = words[i + 1];
        let part = words[i + 2];
        if is_auxiliary(noun) || is_subject_pronoun(noun) {
            continue;
        }
        let has_later_verb = words[i + 3..]
            .iter()
            .any(|&w| is_auxiliary(w) || is_likely_finite_verb(w));
        if is_ving(part) && has_later_verb {
            return true;
        }
        if is_past_participle(part)
            && words
                .get(i + 3)
                .copied()
                .is_some_and(|p| matches!(p, "by" | "in" | "on" | "at" | "from" | "with"))
            && has_later_verb
        {
            return true;
        }
    }
    false
}

fn detect_passive_voice(words: &[&str]) -> bool {
    for i in 0..words.len().saturating_sub(1) {
        if matches!(
            words[i],
            "am" | "is" | "are" | "was" | "were" | "be" | "been" | "being"
        ) {
            let next = words[i + 1];
            let candidate = if next.ends_with("ly") || next == "not" {
                words.get(i + 2).copied()
            } else {
                Some(next)
            };
            if let Some(part) = candidate {
                if is_past_participle(part) && part != "used" {
                    return true;
                }
            }
        }
    }
    false
}

fn detect_progressive(words: &[&str]) -> bool {
    words.windows(2).any(|win| {
        matches!(win[0], "am" | "is" | "are" | "was" | "were" | "been") && is_ving(win[1])
    })
}

fn detect_perfect(words: &[&str]) -> bool {
    words.windows(2).any(|win| {
        matches!(win[0], "have" | "has" | "had") && is_past_participle(win[1])
    })
}

fn detect_phrasal_verb(words: &[&str]) -> bool {
    let verb_roots = [
        "turn", "turned", "turns", "turning", "put", "puts", "putting", "look", "looked", "looks",
        "looking", "call", "called", "calls", "bring", "brought", "brings", "get", "got", "gets",
        "make", "made", "makes", "take", "took", "takes", "give", "gave", "gives", "pick",
        "picked", "picks", "catch", "caught", "drop", "dropped",
    ];
    let particles = ["off", "down", "up", "after", "into", "over", "out", "away"];
    for i in 0..words.len() {
        if verb_roots.contains(&words[i]) {
            for &ahead in words.iter().skip(i + 1).take(3) {
                if particles.contains(&ahead) {
                    return true;
                }
            }
        }
    }
    false
}

fn is_determiner(w: &str) -> bool {
    matches!(
        w,
        "the" | "a" | "an" | "this" | "that" | "these" | "those" | "my" | "your" | "his" | "her"
            | "our" | "their" | "every" | "any" | "some"
    )
}

fn is_subject_pronoun(w: &str) -> bool {
    matches!(w, "i" | "you" | "he" | "she" | "it" | "we" | "they")
}

fn is_auxiliary(w: &str) -> bool {
    matches!(
        w,
        "am" | "is" | "are" | "was" | "were" | "do" | "does" | "did" | "have" | "has" | "had"
            | "can" | "could" | "will" | "would" | "shall" | "should" | "may" | "might" | "must"
    )
}

fn is_ving(w: &str) -> bool {
    w.len() >= 5
        && w.ends_with("ing")
        && !matches!(
            w,
            "thing" | "nothing" | "something" | "everything" | "morning" | "evening" | "during"
                | "bring" | "spring" | "string" | "king" | "ring" | "wing"
        )
}

fn is_past_participle(w: &str) -> bool {
    (w.len() >= 4 && w.ends_with("ed") && !matches!(w, "need" | "seed" | "feed" | "speed" | "bed" | "red"))
        || matches!(
            w,
            "written" | "built" | "made" | "done" | "taken" | "given" | "seen" | "known" | "shown"
                | "told" | "sent" | "found" | "bought" | "caught" | "taught" | "kept" | "left"
                | "held" | "brought" | "spoken" | "broken" | "chosen" | "driven" | "eaten"
                | "fallen" | "forgotten" | "grown" | "hidden" | "paid" | "put" | "read" | "run"
                | "said" | "sold" | "set" | "spent" | "understood" | "won" | "been" | "gone"
                | "heard" | "met" | "lost" | "begun" | "drawn" | "felt" | "meant" | "stood"
        )
}

fn is_likely_finite_verb(w: &str) -> bool {
    is_past_participle(w)
        || matches!(
            w,
            "won" | "saw" | "wrote" | "gave" | "took" | "came" | "went" | "knew" | "began"
                | "spoke" | "broke" | "chose" | "drove" | "ate" | "fell" | "grew" | "ran" | "sang"
                | "looks" | "seems" | "becomes" | "became" | "remains" | "works" | "makes"
                | "takes" | "gives" | "likes" | "wants" | "needs" | "knows" | "thinks" | "says"
                | "love" | "like" | "want" | "need" | "know" | "think" | "recommend" | "buy"
                | "send" | "write" | "read" | "meet"
        )
}

fn is_gerund_or_inf_verb(w: &str) -> bool {
    matches!(
        w,
        "stop" | "stopped" | "stops" | "remember" | "remembered" | "remembers" | "forget"
            | "forgot" | "forgets" | "try" | "tried" | "tries" | "regret" | "regretted" | "avoid"
            | "avoided" | "avoids" | "enjoy" | "enjoyed" | "enjoys" | "finish" | "finished"
            | "mind" | "keep" | "kept" | "consider" | "considered" | "suggest" | "suggested"
            | "admit" | "admitted" | "deny" | "denied" | "want" | "wanted" | "decide" | "decided"
            | "plan" | "planned" | "hope" | "hoped" | "refuse" | "refused" | "promise"
            | "promised" | "manage" | "managed" | "fail" | "failed"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_book_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("knowledge")
            .join("book")
    }

    #[test]
    fn loads_full_textbook_index_and_chapters() {
        let retriever =
            Bm25BookRetriever::from_book_dir(&workspace_book_dir()).expect("load book corpus");
        assert!(
            retriever.document_count() >= 300,
            "expected >= 300 sections, got {}",
            retriever.document_count()
        );
        assert!(
            retriever.route_count() >= 20,
            "expected >= 20 index topic routes, got {}",
            retriever.route_count()
        );
    }

    #[test]
    fn top_3_recall_on_ten_curated_grammar_queries() {
        let retriever =
            Bm25BookRetriever::from_book_dir(&workspace_book_dir()).expect("load book corpus");

        let cases: &[(&str, &[&str])] = &[
            // 1. Reduced adverbial clause / dangling modifier (required by plan)
            ("While walking home, the rain started.", &["ch25"]),
            // 2. Zero object relative clause (required by plan)
            ("The book Maya recommended won a prize.", &["ch19"]),
            // 3. Explicit non-restrictive relative clause
            ("He resigned, which surprised everyone on the team.", &["ch19"]),
            // 4. Passive voice
            ("The quarterly report was written by the committee.", &["ch10"]),
            // 5. Gerund vs infinitive contrast
            ("He stopped smoking after the doctor warned him.", &["ch13", "ch14"]),
            // 6. be used to + Ving vs used to + V
            ("She is used to working late on Fridays.", &["ch14", "ch13"]),
            // 7. Subjunctive / counterfactual conditional
            ("If I were you, I would accept the fellowship.", &["ch12"]),
            // 8. Negative adverb inversion
            ("Never have I seen such a clear explanation.", &["ch21"]),
            // 9. Subject-verb agreement with correlatives
            ("Neither the manager nor the engineers were informed.", &["ch20"]),
            // 10. Clausal subject (noun clause)
            ("What he said during the meeting surprised everyone.", &["ch17"]),
        ];

        for &(query, expected_chapters) in cases {
            let hits = retriever.retrieve(query, 3);
            assert!(
                !hits.is_empty(),
                "query `{query}` returned no retrieval hits"
            );
            let found_chapters: Vec<&str> = hits.iter().map(|h| h.chapter.as_str()).collect();
            let matched = hits
                .iter()
                .any(|h| expected_chapters.contains(&h.chapter.as_str()));
            assert!(
                matched,
                "query `{query}` expected one of {:?} in top-3, got {:?}",
                expected_chapters, found_chapters
            );
            assert!(
                hits[0].passage_id.is_some(),
                "query `{query}` top hit missing passage_id: {:?}",
                hits[0]
            );
        }
    }

    #[test]
    fn chinese_grammar_queries_match_expected_chapters() {
        let retriever =
            Bm25BookRetriever::from_book_dir(&workspace_book_dir()).expect("load book corpus");

        let hits = retriever.retrieve("关系从句 省略 受词位置", 3);
        assert!(hits.iter().any(|h| h.chapter == "ch19"), "hits: {:?}", hits);

        let hits = retriever.retrieve("副词子句减化 悬垂修饰语", 3);
        assert!(hits.iter().any(|h| h.chapter == "ch25"), "hits: {:?}", hits);
    }

    #[test]
    fn empty_query_or_zero_limit_returns_empty() {
        let retriever =
            Bm25BookRetriever::from_book_dir(&workspace_book_dir()).expect("load book corpus");
        assert!(retriever.retrieve("   ", 3).is_empty());
        assert!(retriever.retrieve("While walking home, the rain started.", 0).is_empty());
    }
}
