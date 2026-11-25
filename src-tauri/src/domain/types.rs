use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProviderConfig {
    #[serde(rename_all = "camelCase")]
    Ollama {
        id: String,
        name: String,
        base_url: String,
        model: String,
    },
    #[serde(rename_all = "camelCase")]
    OpenaiCompatible {
        id: String,
        name: String,
        base_url: String,
        api_key_ref: String,
        model: String,
    },
    #[serde(rename_all = "camelCase")]
    Anthropic {
        id: String,
        name: String,
        base_url: String,
        api_key_ref: String,
        model: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderMode {
    #[serde(rename = "unified-3layer")]
    Unified3Layer,
    #[serde(rename = "custom-html")]
    CustomHtml,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAction {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub is_built_in: bool,
    pub render_mode: RenderMode,
    pub html_template: Option<String>,
    pub css_template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookRef {
    pub chapter: String,
    pub section: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passage_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalHit {
    pub chapter: String,
    pub section: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passage_id: Option<String>,
    pub excerpt: String,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AnalysisStreamEvent {
    #[serde(rename_all = "camelCase")]
    RetrievalReady { hits: Vec<RetrievalHit> },
    #[serde(rename_all = "camelCase")]
    TokenDelta { chunk: String },
    #[serde(rename_all = "camelCase")]
    Completed { raw_json: String, latency_ms: u64 },
    #[serde(rename_all = "camelCase")]
    Failed { code: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_config_round_trip_matches_ts_wire_shape() {
        let providers = vec![
            ProviderConfig::Ollama {
                id: "prov-ollama".into(),
                name: "Local Ollama".into(),
                base_url: "http://localhost:11434".into(),
                model: "qwen2.5:14b".into(),
            },
            ProviderConfig::OpenaiCompatible {
                id: "prov-openai".into(),
                name: "OpenAI".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key_ref: "keychain:openai".into(),
                model: "gpt-4.1-mini".into(),
            },
            ProviderConfig::Anthropic {
                id: "prov-anthropic".into(),
                name: "Anthropic".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key_ref: "keychain:anthropic".into(),
                model: "claude-sonnet-4-5".into(),
            },
        ];

        for provider in providers {
            let value = serde_json::to_value(&provider).expect("serialize provider");
            assert!(value.get("kind").is_some());
            assert!(value.get("baseUrl").is_some());
            let decoded: ProviderConfig =
                serde_json::from_value(value).expect("deserialize provider");
            assert_eq!(decoded, provider);
        }

        let openai_json = serde_json::to_string(&ProviderConfig::OpenaiCompatible {
            id: "p1".into(),
            name: "LM Studio".into(),
            base_url: "http://localhost:1234/v1".into(),
            api_key_ref: "none".into(),
            model: "local".into(),
        })
        .expect("serialize openai-compatible");
        assert!(openai_json.contains("\"kind\":\"openai-compatible\""));
        assert!(openai_json.contains("\"apiKeyRef\":\"none\""));
    }

    #[test]
    fn custom_action_round_trip_matches_ts_wire_shape() {
        let action = CustomAction {
            id: "act-grammar".into(),
            name: "English Grammar".into(),
            system_prompt: "Analyze syntax.".into(),
            is_built_in: true,
            render_mode: RenderMode::Unified3Layer,
            html_template: None,
            css_template: None,
        };

        let json = serde_json::to_string(&action).expect("serialize custom action");
        assert!(json.contains("\"renderMode\":\"unified-3layer\""));
        assert!(json.contains("\"isBuiltIn\":true"));
        assert!(json.contains("\"htmlTemplate\":null"));

        let decoded: CustomAction = serde_json::from_str(&json).expect("deserialize custom action");
        assert_eq!(decoded, action);
    }

    #[test]
    fn book_ref_and_retrieval_hit_round_trip() {
        let book_ref = BookRef {
            chapter: "ch19".into(),
            section: "关系子句".into(),
            passage_id: Some("ch19-p04".into()),
        };
        let book_json = serde_json::to_string(&book_ref).expect("serialize book ref");
        assert!(book_json.contains("\"passageId\":\"ch19-p04\""));
        assert_eq!(
            serde_json::from_str::<BookRef>(&book_json).expect("deserialize book ref"),
            book_ref
        );

        let book_ref_no_passage = BookRef {
            chapter: "ch19".into(),
            section: "关系子句".into(),
            passage_id: None,
        };
        let no_passage_json =
            serde_json::to_string(&book_ref_no_passage).expect("serialize book ref without passage");
        assert!(!no_passage_json.contains("passageId"));

        let hit = RetrievalHit {
            chapter: "ch25".into(),
            section: "减化子句".into(),
            passage_id: Some("ch25-p12".into()),
            excerpt: "Although widely praised...".into(),
            score: 12.5,
        };
        let hit_json = serde_json::to_string(&hit).expect("serialize retrieval hit");
        assert_eq!(
            serde_json::from_str::<RetrievalHit>(&hit_json).expect("deserialize retrieval hit"),
            hit
        );
    }

    #[test]
    fn analysis_stream_event_round_trip_matches_ts_wire_shape() {
        let events = vec![
            AnalysisStreamEvent::RetrievalReady {
                hits: vec![RetrievalHit {
                    chapter: "ch19".into(),
                    section: "关系子句".into(),
                    passage_id: None,
                    excerpt: "excerpt".into(),
                    score: 3.5,
                }],
            },
            AnalysisStreamEvent::TokenDelta {
                chunk: "{\"meaning\":".into(),
            },
            AnalysisStreamEvent::Completed {
                raw_json: "{}".into(),
                latency_ms: 420,
            },
            AnalysisStreamEvent::Failed {
                code: "timeout".into(),
                message: "request timed out".into(),
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).expect("serialize event");
            let decoded: AnalysisStreamEvent =
                serde_json::from_str(&json).expect("deserialize event");
            assert_eq!(&decoded, event);
        }

        let completed_json = serde_json::to_string(&events[2]).expect("serialize completed");
        assert!(completed_json.contains("\"kind\":\"completed\""));
        assert!(completed_json.contains("\"rawJson\":\"{}\""));
        assert!(completed_json.contains("\"latencyMs\":420"));

        let delta_json = serde_json::to_string(&events[1]).expect("serialize token delta");
        assert!(delta_json.contains("\"kind\":\"token-delta\""));
    }
}
