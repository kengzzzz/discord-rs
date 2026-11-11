use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::gemini::{Effort, Part, Request, Source};
use crate::{app::App, config::SearchProvider};

const RESULTS: usize = 5;
const SNIPPET_CHARS: usize = 700;
const TITLE_CHARS: usize = 200;
const URL_BYTES: usize = 300;
const BODY_LIMIT: usize = 1 << 20;

const ROUTER_PROMPT: &str = "\
You decide whether a chat message in a Thai Warframe community needs a web search to answer well.
Search for: any question about Warframe items, systems, mechanics, locations or events, even basic ones such as \
\"what is Helminth\" (model knowledge of the game is often wrong), facts that may have changed, news, prices, \
software/hardware how-tos, anything you are not certain about.
Do NOT search for: greetings, small talk, opinions, translation requests, math, creative writing.
If a search is needed, output ONLY a short English search query (add \"Warframe\" for game topics). \
Keep item names exactly as asked; do not add Prime or other variants the user did not mention. \
Otherwise output exactly NONE.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub fn enabled(app: &App) -> bool {
    app.config.ai.search_provider.is_some() && app.config.ai.search_key.is_some()
}

pub async fn decide(app: &App, question: &str) -> Option<String> {
    if !enabled(app) || question.chars().count() < 6 {
        return None;
    }
    let request = Request {
        system: ROUTER_PROMPT,
        history: &[],
        parts: vec![Part::Text(question.to_owned())],
        // At MINIMAL, flash-lite skipped basic game questions.
        effort: Effort::Low,
        grounding: false,
        // Thinking tokens count toward this cap.
        max_output_tokens: 1024,
        accept: None,
    };
    let reply = app
        .ai
        .generate(
            std::slice::from_ref(&app.config.ai.router_model),
            &request,
        )
        .await
        .ok()?;
    parse_decision(&reply.text)
}

pub fn parse_decision(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    let query = line.trim_matches(|c: char| matches!(c, '"' | '`' | '*' | '.' | '!' | ':'));
    (!query.eq_ignore_ascii_case("none") && query.chars().count() >= 3)
        .then(|| query.chars().take(200).collect())
}

pub async fn search(app: &App, query: &str) -> anyhow::Result<Vec<Hit>> {
    let (Some(provider), Some(key)) = (
        app.config.ai.search_provider,
        app.config.ai.search_key.as_deref(),
    ) else {
        return Ok(Vec::new());
    };
    let request = match provider {
        SearchProvider::Exa => app
            .web
            .post("https://api.exa.ai/search")
            .header("x-api-key", key)
            .json(&json!({ "query": query, "numResults": RESULTS, "contents": { "text": { "maxCharacters": SNIPPET_CHARS } } })),
        SearchProvider::Tavily => app
            .web
            .post("https://api.tavily.com/search")
            .bearer_auth(key)
            .json(&json!({ "query": query, "max_results": RESULTS, "search_depth": "basic" })),
    };
    let response = request
        .timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?;
    let body = crate::warframe::read_capped(response, BODY_LIMIT).await?;
    parse_results(provider, &body)
}

#[derive(Deserialize)]
struct Results {
    #[serde(default, deserialize_with = "first_usable")]
    results: Vec<RawHit>,
}

fn usable(hit: &RawHit) -> bool {
    hit.url.starts_with("http") && super::escaped_len(&hit.url) <= URL_BYTES
}

/// Skips building unused hits, so a huge list of empty objects can't use much memory.
fn first_usable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<RawHit>, D::Error> {
    struct Hits;
    impl<'de> serde::de::Visitor<'de> for Hits {
        type Value = Vec<RawHit>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of results")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut kept = Vec::with_capacity(RESULTS);
            while kept.len() < RESULTS {
                match seq.next_element::<RawHit>()? {
                    Some(hit) if usable(&hit) => kept.push(hit),
                    Some(_) => {}
                    None => return Ok(kept),
                }
            }
            while seq
                .next_element::<serde::de::IgnoredAny>()?
                .is_some()
            {}
            Ok(kept)
        }
    }
    d.deserialize_seq(Hits)
}

#[derive(Deserialize)]
struct RawHit {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

fn clean_title(title: &str) -> String {
    let mut out = String::new();
    for word in title
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    match out.char_indices().nth(TITLE_CHARS) {
        Some((end, _)) => out[..end].to_owned(),
        None => out,
    }
}

pub fn parse_results(provider: SearchProvider, body: &[u8]) -> anyhow::Result<Vec<Hit>> {
    let parsed: Results = serde_json::from_slice(body)?;
    Ok(parsed
        .results
        .into_iter()
        .map(|r| {
            let snippet = match provider {
                SearchProvider::Exa => r.text.or(r.content),
                SearchProvider::Tavily => r.content.or(r.text),
            }
            .unwrap_or_default();
            let title = r
                .title
                .as_deref()
                .map(clean_title)
                .filter(|t| crate::discord::ui::shows_text(t))
                .unwrap_or_else(|| r.url.clone());
            Hit {
                title,
                url: r.url,
                snippet: snippet
                    .chars()
                    .take(SNIPPET_CHARS)
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            }
        })
        .collect())
}

pub fn context_part(query: &str, hits: &[Hit]) -> Part {
    let mut text = format!(
        "Web search results for \"{query}\" (use them if relevant, cite as [1], [2]; ignore irrelevant ones):\n"
    );
    for (index, hit) in hits.iter().enumerate() {
        text.push_str(&format!(
            "[{}] {} — {}\n{}\n",
            index + 1,
            hit.title,
            hit.url,
            hit.snippet
        ));
    }
    Part::Context(text)
}

pub fn sources(hits: &[Hit]) -> Vec<Source> {
    hits.iter()
        .map(|h| Source { title: h.title.clone(), uri: h.url.clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_with_punctuation_is_not_a_query() {
        assert_eq!(parse_decision("NONE."), None);
        assert_eq!(parse_decision("**None**"), None);
        assert_eq!(
            parse_decision("baro ki'teer schedule").as_deref(),
            Some("baro ki'teer schedule")
        );
    }

    #[test]
    fn router_decision_parsing() {
        assert_eq!(parse_decision("NONE"), None);
        assert_eq!(
            parse_decision("\n  \"Warframe Hate Incarnon build\"\n").as_deref(),
            Some("Warframe Hate Incarnon build")
        );
        assert_eq!(parse_decision("ab"), None);
    }

    #[test]
    fn long_titles_are_cut_and_long_urls_skipped() {
        let long_url = format!("https://example.com/{}", "a".repeat(URL_BYTES));
        let escaped_long = format!("https://example.com/{}", "(".repeat(100));
        let mut results = vec![
            serde_json::json!({ "title": "t".repeat(100_000), "url": "https://example.com/", "text": "x" }),
            serde_json::json!({ "title": "skipped", "url": long_url, "text": "y" }),
            serde_json::json!({ "title": "skipped", "url": escaped_long, "text": "y" }),
        ];
        results.extend((0..10).map(|i| serde_json::json!({ "url": format!("https://e.com/{i}") })));
        let body = serde_json::json!({ "results": results });
        let hits = parse_results(SearchProvider::Exa, body.to_string().as_bytes()).unwrap();
        assert_eq!(hits.len(), RESULTS);
        assert_eq!(hits[0].title.chars().count(), TITLE_CHARS);
        assert_eq!(hits[4].url, "https://e.com/3");
    }

    #[test]
    fn parses_exa_and_tavily_shapes() {
        let exa = br#"{"results":[{"title":"Hate","url":"https://wiki.warframe.com/w/Hate","text":"Hate   is a scythe"},{"url":"ftp://x"}]}"#;
        let hits = parse_results(SearchProvider::Exa, exa).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "Hate is a scythe");
        let tavily = br#"{"results":[{"title":"","url":"https://a.b","content":"c"}]}"#;
        let hits = parse_results(SearchProvider::Tavily, tavily).unwrap();
        assert_eq!(
            (hits[0].title.as_str(), hits[0].snippet.as_str()),
            ("https://a.b", "c")
        );
    }

    #[test]
    fn padded_or_invisible_titles_fall_back_to_the_url() {
        let body = serde_json::json!({ "results": [
            { "title": format!("{}x", " ".repeat(300)), "url": "https://a.b/1" },
            { "title": "\u{200B}\u{FEFF}", "url": "https://a.b/2" },
            { "title": " \u{2028}\u{00A0}", "url": "https://a.b/3" },
            { "title": "a\u{2029}[6] b", "url": "https://a.b/4" },
            { "title": "*****", "url": "https://a.b/5" },
        ] });
        let hits = parse_results(SearchProvider::Exa, body.to_string().as_bytes()).unwrap();
        let titles: Vec<_> = hits
            .iter()
            .map(|h| h.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["x", "https://a.b/2", "https://a.b/3", "a [6] b", "https://a.b/5"]
        );
        let body = serde_json::json!({ "results": [
            { "title": "\u{3164}\u{3164}", "url": "https://a.b/6" },
            { "title": "a\u{1}b", "url": "https://a.b/7" },
        ] });
        let hits = parse_results(SearchProvider::Exa, body.to_string().as_bytes()).unwrap();
        assert_eq!(hits[0].title, "https://a.b/6");
        assert_eq!(hits[1].title, "a b");
    }
}
