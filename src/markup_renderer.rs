use crate::{
    html_entity_renderer::{HtmlEntityRenderer, SimpleHtmlEntityRenderer},
    utils::trim_to_string,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use tl::{Parser, VDom};

#[derive(Error, Debug)]
pub enum Error {
    #[error("MediaWiki server error: {0}")]
    MediaWikiServerError(#[from] reqwest::Error),
    #[error("Unexpected MediaWiki response: {message}")]
    UnexpectedMediaWikiResponse { message: String, response: String },
    #[error("Failed to render fragment: {0}")]
    RenderingError(String),
    #[error("Parse error: {0}")]
    ParseError(#[from] tl::ParseError),
    #[error("General error: {0}")]
    GeneralError(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[async_trait::async_trait]
pub trait MediaWikiClient: Send + Sync {
    async fn request(&self, page_title: &str, markup: &str) -> Result<Value, Error>;
}

pub struct LocalMediaWikiClient {
    endpoint: String,
    client: reqwest::Client,
}

impl Default for LocalMediaWikiClient {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8080/api.php".to_string(),
            client: reqwest::Client::new(),
        }
    }
}

impl LocalMediaWikiClient {
    fn build_params(&self, page_title: &str, markup: &str) -> HashMap<String, String> {
        [
            ("action", "parse"),
            ("title", page_title),
            ("text", &markup),
            ("prop", "text"),
            ("disablelimitreport", "1"),
            ("wrapoutputclass", ""),
            ("format", "json"),
            ("formatversion", "2"),
        ]
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect::<HashMap<_, _>>()
    }
}

#[async_trait::async_trait]
impl MediaWikiClient for LocalMediaWikiClient {
    async fn request(&self, page_title: &str, markup: &str) -> Result<Value, Error> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .query(&self.build_params(page_title, markup))
            .send()
            .await?;

        Ok(response.json::<Value>().await?)
    }
}

#[async_trait]
pub trait MarkupRenderer {
    async fn render(&self, page_title: &str, markup: &str) -> Result<String, Error>;
}

pub struct SimpleMarkupRenderer {
    html_entity_renderer: Box<dyn HtmlEntityRenderer>,
    media_wiki_client: Box<dyn MediaWikiClient>,
}

impl Default for SimpleMarkupRenderer {
    fn default() -> Self {
        Self {
            html_entity_renderer: Box::new(SimpleHtmlEntityRenderer::default()),
            media_wiki_client: Box::new(LocalMediaWikiClient::default()),
        }
    }
}

impl SimpleMarkupRenderer {
    pub fn new(
        html_entity_renderer: Box<dyn HtmlEntityRenderer>,
        media_wiki_client: Box<dyn MediaWikiClient>,
    ) -> Self {
        Self {
            html_entity_renderer,
            media_wiki_client,
        }
    }

    async fn render_markup(&self, page_title: &str, markup: &str) -> Result<String, Error> {
        let json = self.media_wiki_client.request(page_title, markup).await?;

        let parsed = fetch_object_by_key(&json, "parse")?;
        let text = fetch_string_by_key(&parsed, "text")?;
        let dom = tl::parse(&text, tl::ParserOptions::default())?;
        let parser = dom.parser();
        check_for_wikimedia_errors(&dom, parser)?;
        Ok(trim_to_string(
            &dom.children()
                .iter()
                .filter_map(|handle| handle.get(parser))
                .map(|node| node.inner_text(parser))
                .collect::<String>(),
        ))
    }
}

#[async_trait]
impl MarkupRenderer for SimpleMarkupRenderer {
    async fn render(&self, page_title: &str, markup: &str) -> Result<String, Error> {
        let buffer = strip_refs(&markup);
        let buffer = unwrap_math(&buffer);
        let buffer = self.render_markup(page_title, &buffer).await?;
        let result = self.html_entity_renderer.render(&buffer);

        Ok(result)
    }
}

fn fetch_object_by_key<'a>(json: &'a Value, key: &str) -> Result<&'a Value, Error> {
    json.get(key)
        .filter(|value| value.is_object())
        .ok_or_else(|| Error::UnexpectedMediaWikiResponse {
            message: format!("'{key}' was missing or not an object"),
            response: json.to_string(),
        })
}

fn fetch_string_by_key(json: &Value, key: &str) -> Result<String, Error> {
    if let Some(value) = json.get(key).and_then(|value| value.as_str()) {
        Ok(value.to_string())
    } else {
        return Err(Error::UnexpectedMediaWikiResponse {
            message: format!("'{key}' was missing or not a string"),
            response: json.to_string(),
        });
    }
}

fn check_for_wikimedia_errors(dom: &VDom, parser: &Parser) -> Result<(), Error> {
    if let Some(iter) = dom.query_selector(".error") {
        for handle in iter {
            if let Some(node) = handle.get(parser) {
                return Err(Error::RenderingError(format!(
                    "Found error element: '{}'",
                    node.inner_text(parser)
                )));
            }
        }
    }
    Ok(())
}

fn strip_refs(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;

    while let Some(start) = rest.find("<ref") {
        let after = &rest[start + 4..];
        let is_ref_tag = after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/');
        let Some(open_end) = after.find('>').filter(|_| is_ref_tag) else {
            out.push_str(&rest[..start + 4]);
            rest = after;
            continue;
        };
        let tag_end = open_end + 1;

        if after[..open_end].ends_with('/') {
            out.push_str(&rest[..start]);
            rest = &after[tag_end..];
        } else if let Some(close) = after[tag_end..].find("</ref>") {
            out.push_str(&rest[..start]);
            rest = &after[tag_end + close + "</ref>".len()..];
        } else {
            out.push_str(&rest[..start + 4 + tag_end]);
            rest = &after[tag_end..];
        }
    }

    out.push_str(rest);
    out
}

const OPEN_MATH: &str = "<math";
const CLOSE_MATH: &str = "</math>";

fn unwrap_math(text: &str) -> String {
    let mut result = String::new();
    let text = text.chars().collect::<Vec<_>>();
    let mut index = 0;

    loop {
        if index >= text.len() {
            break;
        }

        if is_entering_math(&text, index) {
            let mut new_index = index + OPEN_MATH.len();
            let mut valid = false;

            while new_index < text.len() {
                let ch = text[new_index];
                new_index += 1;

                if ch == '>' {
                    valid = true;
                    break;
                }
            }

            if valid {
                index = new_index;
            } else {
                result.push(text[index]);
                index += 1;
            }

            continue;
        }

        if is_exiting_math(&text, index) {
            index += CLOSE_MATH.len();
            continue;
        }

        result.push(text[index]);
        index += 1;
    }

    result
}

fn is_entering_math(text: &[char], index: usize) -> bool {
    let open = OPEN_MATH.chars().collect::<Vec<_>>();

    text[index..].starts_with(&open)
        && matches!(text.get(index + open.len()), Some(ch) if ch.is_whitespace() || *ch == '>')
}

fn is_exiting_math(text: &[char], index: usize) -> bool {
    text[index..].starts_with(&CLOSE_MATH.chars().collect::<Vec<_>>())
}

#[cfg(test)]
mod tests;
