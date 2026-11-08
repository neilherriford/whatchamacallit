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
            ("prop", "text|limitreportdata"),
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
            .post(self.endpoint.clone())
            .form(&self.build_params(page_title, markup))
            .send()
            .await?;

        Ok(response.json::<Value>().await?)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TemplateCost {
    pub name: String,
    pub calls: u64,
    pub millis: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RenderReport {
    pub text: String,
    pub parser_cpu_seconds: f64,
    pub parser_wall_seconds: f64,
    pub lua_seconds: f64,
    pub lua_memory_bytes: u64,
    pub lua_memory_limit_bytes: u64,
    pub lua_cpu_limit_seconds: f64,
    pub templates: Vec<TemplateCost>,
}

#[async_trait]
pub trait MarkupRenderer {
    async fn render(&self, page_title: &str, markup: &str) -> Result<String, Error>;
    async fn render_report(&self, page_title: &str, markup: &str) -> Result<RenderReport, Error>;
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

    async fn render_markup(&self, page_title: &str, markup: &str) -> Result<RenderReport, Error> {
        let json = self.media_wiki_client.request(page_title, markup).await?;

        let parsed = fetch_object_by_key(&json, "parse")?;
        let text = fetch_string_by_key(&parsed, "text")?;
        let dom = tl::parse(&text, tl::ParserOptions::default())?;
        let parser = dom.parser();
        check_for_wikimedia_errors(&dom, parser)?;

        let mut report = extract_limit_report(parsed);
        report.text = trim_to_string(
            &dom.children()
                .iter()
                .filter_map(|handle| handle.get(parser))
                .map(|node| node.inner_text(parser))
                .collect::<String>(),
        );
        Ok(report)
    }
}

#[async_trait]
impl MarkupRenderer for SimpleMarkupRenderer {
    async fn render(&self, page_title: &str, markup: &str) -> Result<String, Error> {
        Ok(self.render_report(page_title, markup).await?.text)
    }

    async fn render_report(&self, page_title: &str, markup: &str) -> Result<RenderReport, Error> {
        let buffer = strip_refs(&markup);
        let buffer = unwrap_math(&buffer);
        let mut report = self.render_markup(page_title, &buffer).await?;
        report.text = self.html_entity_renderer.render(&report.text);

        Ok(report)
    }
}

fn extract_limit_report(parsed: &Value) -> RenderReport {
    let mut report = RenderReport::default();

    if let Some(item) = find_report_item(parsed, "limitreport-cputime") {
        report.parser_cpu_seconds = report_number(item, "0").unwrap_or_default();
    }
    if let Some(item) = find_report_item(parsed, "limitreport-walltime") {
        report.parser_wall_seconds = report_number(item, "0").unwrap_or_default();
    }
    if let Some(item) = find_report_item(parsed, "scribunto-limitreport-timeusage") {
        report.lua_seconds = report_number(item, "0").unwrap_or_default();
        report.lua_cpu_limit_seconds = report_number(item, "1").unwrap_or_default();
    }
    if let Some(item) = find_report_item(parsed, "scribunto-limitreport-virtmemusage") {
        report.lua_memory_bytes = report_integer(item, "0").unwrap_or_default();
        report.lua_memory_limit_bytes = report_integer(item, "1").unwrap_or_default();
    }
    if let Some(item) = find_report_item(parsed, "limitreport-timingprofile") {
        report.templates = parse_timing_profile(item);
    }

    report
}

fn find_report_item<'a>(parsed: &'a Value, name: &str) -> Option<&'a Value> {
    parsed
        .get("limitreportdata")?
        .as_array()?
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
}

fn report_number(item: &Value, index: &str) -> Option<f64> {
    let value = item.get(index)?;
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

fn report_integer(item: &Value, index: &str) -> Option<u64> {
    let value = item.get(index)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

fn parse_timing_profile(item: &Value) -> Vec<TemplateCost> {
    let Some(object) = item.as_object() else {
        return Vec::new();
    };

    let mut lines = object
        .iter()
        .filter_map(|(key, value)| Some((key.parse::<usize>().ok()?, value.as_str()?)))
        .collect::<Vec<_>>();
    lines.sort_by_key(|(index, _)| *index);

    lines
        .into_iter()
        .filter_map(|(_, line)| parse_timing_line(line))
        .collect()
}

fn parse_timing_line(line: &str) -> Option<TemplateCost> {
    let mut parts = line.split_whitespace();
    let _percent = parts.next()?;
    let millis = parts.next()?.parse::<f64>().ok()?;
    let calls = parts.next()?.parse::<u64>().ok()?;
    let name = parts.collect::<Vec<_>>().join(" ");

    if name.is_empty() || name == "-total" {
        return None;
    }

    Some(TemplateCost {
        name,
        calls,
        millis,
    })
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
