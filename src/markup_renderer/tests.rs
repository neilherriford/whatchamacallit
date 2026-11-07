use super::*;
use serde_json::json;
use std::sync::{Arc, Mutex};

type Recorded<T> = Arc<Mutex<Vec<T>>>;

fn record<T>(store: &Recorded<T>, value: T) {
    match store.lock() {
        Ok(mut guard) => guard.push(value),
        Err(poisoned) => poisoned.into_inner().push(value),
    }
}

fn recorded<T: Clone>(store: &Recorded<T>) -> Vec<T> {
    match store.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

struct MockHtmlEntityRenderer {
    inputs: Recorded<String>,
}

impl HtmlEntityRenderer for MockHtmlEntityRenderer {
    fn render(&self, html: &str) -> String {
        record(&self.inputs, html.to_string());
        format!("mock:{html}")
    }
}

#[derive(Clone)]
enum Reply {
    Json(Value),
    Failure(String),
}

struct MockMediaWikiClient {
    reply: Reply,
    requests: Recorded<(String, String)>,
}

#[async_trait]
impl MediaWikiClient for MockMediaWikiClient {
    async fn request(&self, page_title: &str, markup: &str) -> Result<Value, Error> {
        record(&self.requests, (page_title.to_string(), markup.to_string()));

        match &self.reply {
            Reply::Json(value) => Ok(value.clone()),
            Reply::Failure(message) => Err(Error::RenderingError(message.clone())),
        }
    }
}

struct Harness {
    renderer: SimpleMarkupRenderer,
    entity_inputs: Recorded<String>,
    requests: Recorded<(String, String)>,
}

fn harness(reply: Reply) -> Harness {
    let entity_inputs: Recorded<String> = Arc::new(Mutex::new(Vec::new()));
    let requests: Recorded<(String, String)> = Arc::new(Mutex::new(Vec::new()));
    let renderer = SimpleMarkupRenderer::new(
        Box::new(MockHtmlEntityRenderer {
            inputs: entity_inputs.clone(),
        }),
        Box::new(MockMediaWikiClient {
            reply,
            requests: requests.clone(),
        }),
    );

    Harness {
        renderer,
        entity_inputs,
        requests,
    }
}

fn page(html: &str) -> Reply {
    Reply::Json(json!({"parse": {"text": html}}))
}

fn check(html: &str) -> Result<(), Error> {
    match tl::parse(html, tl::ParserOptions::default()) {
        Ok(dom) => check_for_wikimedia_errors(&dom, dom.parser()),
        Err(err) => Err(Error::ParseError(err)),
    }
}

#[test]
fn local_client_defaults_to_the_local_mediawiki_endpoint() {
    assert_eq!(
        LocalMediaWikiClient::default().endpoint,
        "http://localhost:8080/api.php"
    );
}

#[test]
fn build_params_asks_mediawiki_to_parse_the_markup() {
    let params =
        LocalMediaWikiClient::default().build_params("dictionary", "{{lb|en|informal}} meaning");

    assert_eq!(params.get("action").map(String::as_str), Some("parse"));
    assert_eq!(params.get("title").map(String::as_str), Some("dictionary"));
    assert_eq!(
        params.get("text").map(String::as_str),
        Some("{{lb|en|informal}} meaning")
    );
    assert_eq!(params.get("prop").map(String::as_str), Some("text"));
    assert_eq!(
        params.get("disablelimitreport").map(String::as_str),
        Some("1")
    );
    assert_eq!(params.get("wrapoutputclass").map(String::as_str), Some(""));
    assert_eq!(params.get("format").map(String::as_str), Some("json"));
    assert_eq!(params.get("formatversion").map(String::as_str), Some("2"));
}

#[test]
fn build_params_sends_exactly_eight_parameters() {
    assert_eq!(
        LocalMediaWikiClient::default()
            .build_params("word", "text")
            .len(),
        8
    );
}

#[test]
fn build_params_passes_markup_through_unchanged() {
    let markup = "a & b <ref>x</ref> {{t|é}}";
    let params = LocalMediaWikiClient::default().build_params("word", markup);

    assert_eq!(params.get("text").map(String::as_str), Some(markup));
}

#[test]
fn mock_entity_renderer_is_accepted_as_a_trait_object() {
    let inputs: Recorded<String> = Arc::new(Mutex::new(Vec::new()));
    let renderer: Box<dyn HtmlEntityRenderer> = Box::new(MockHtmlEntityRenderer {
        inputs: inputs.clone(),
    });

    assert_eq!(renderer.render("x"), "mock:x");
    assert_eq!(recorded(&inputs), vec!["x".to_string()]);
}

#[tokio::test]
async fn render_sends_the_page_title_and_markup_to_the_client() {
    let harness = harness(page("<p>ok</p>"));
    let result = harness
        .renderer
        .render("dictionary", "{{lb|en|informal}} meaning")
        .await;

    assert!(result.is_ok());
    assert_eq!(
        recorded(&harness.requests),
        vec![(
            "dictionary".to_string(),
            "{{lb|en|informal}} meaning".to_string()
        )]
    );
}

#[tokio::test]
async fn render_asks_the_client_exactly_once() {
    let harness = harness(page("<p>ok</p>"));
    let _ = harness.renderer.render("word", "text").await;

    assert_eq!(recorded(&harness.requests).len(), 1);
}

#[tokio::test]
async fn render_strips_refs_before_sending() {
    let harness = harness(page("<p>ok</p>"));
    let _ = harness
        .renderer
        .render("word", "a<ref>cite</ref>b<ref name=\"x\"/>c")
        .await;

    assert_eq!(
        recorded(&harness.requests),
        vec![("word".to_string(), "abc".to_string())]
    );
}

#[tokio::test]
async fn render_unwraps_math_before_sending() {
    let harness = harness(page("<p>ok</p>"));
    let _ = harness
        .renderer
        .render("word", "so <math>x^2</math> is")
        .await;

    assert_eq!(
        recorded(&harness.requests),
        vec![("word".to_string(), "so x^2 is".to_string())]
    );
}

#[tokio::test]
async fn render_strips_refs_and_unwraps_math_in_the_same_request() {
    let harness = harness(page("<p>ok</p>"));
    let _ = harness
        .renderer
        .render("word", "<math>a</math><ref>c</ref><math>b</math>")
        .await;

    assert_eq!(
        recorded(&harness.requests),
        vec![("word".to_string(), "ab".to_string())]
    );
}

#[tokio::test]
async fn render_sends_clean_markup_unchanged() {
    let harness = harness(page("<p>ok</p>"));
    let _ = harness
        .renderer
        .render("word", "{{ux|en|It is <b>bold</b>.}}")
        .await;

    assert_eq!(
        recorded(&harness.requests),
        vec![(
            "word".to_string(),
            "{{ux|en|It is <b>bold</b>.}}".to_string()
        )]
    );
}

#[tokio::test]
async fn render_passes_the_page_text_to_the_entity_renderer() {
    let harness = harness(page("<p>Hello &amp; bye</p>"));
    let _ = harness.renderer.render("word", "text").await;

    assert_eq!(
        recorded(&harness.entity_inputs),
        vec!["Hello &amp; bye".to_string()]
    );
}

#[tokio::test]
async fn render_returns_what_the_entity_renderer_produced() {
    let harness = harness(page("<p>Hello</p>"));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(result, Ok(text) if text == "mock:Hello"));
}

#[tokio::test]
async fn render_calls_the_entity_renderer_exactly_once() {
    let harness = harness(page("<p>Hello</p>"));
    let _ = harness.renderer.render("word", "text").await;

    assert_eq!(recorded(&harness.entity_inputs).len(), 1);
}

#[tokio::test]
async fn render_removes_html_tags_from_the_page_text() {
    let harness = harness(page("<p>a <b>bold</b> and <i>slanted</i> word</p>"));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(result, Ok(text) if text == "mock:a bold and slanted word"));
}

#[tokio::test]
async fn render_trims_the_page_text() {
    let harness = harness(page("  <p>padded</p>\n\n"));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(result, Ok(text) if text == "mock:padded"));
}

#[tokio::test]
async fn render_of_an_empty_page_gives_an_empty_text_to_the_entity_renderer() {
    let harness = harness(page(""));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(result, Ok(text) if text == "mock:"));
    assert_eq!(recorded(&harness.entity_inputs), vec![String::new()]);
}

#[tokio::test]
async fn render_keeps_entities_for_the_entity_renderer_to_decode() {
    let harness = harness(page("<p>&lt;b&gt; &#39;x&#39;</p>"));
    let _ = harness.renderer.render("word", "text").await;

    assert_eq!(
        recorded(&harness.entity_inputs),
        vec!["&lt;b&gt; &#39;x&#39;".to_string()]
    );
}

#[tokio::test]
async fn render_returns_client_errors_without_calling_the_entity_renderer() {
    let harness = harness(Reply::Failure("connection refused".to_string()));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(
        result,
        Err(Error::RenderingError(message)) if message == "connection refused"
    ));
    assert!(recorded(&harness.entity_inputs).is_empty());
}

#[tokio::test]
async fn render_rejects_a_response_without_a_parse_object() {
    let harness = harness(Reply::Json(json!({"error": {"code": "badtitle"}})));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(
        result,
        Err(Error::UnexpectedMediaWikiResponse { message, .. })
            if message == "'parse' was missing or not an object"
    ));
    assert!(recorded(&harness.entity_inputs).is_empty());
}

#[tokio::test]
async fn render_rejects_a_parse_object_without_text() {
    let harness = harness(Reply::Json(json!({"parse": {"title": "word"}})));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(
        result,
        Err(Error::UnexpectedMediaWikiResponse { message, .. })
            if message == "'text' was missing or not a string"
    ));
    assert!(recorded(&harness.entity_inputs).is_empty());
}

#[tokio::test]
async fn render_rejects_text_that_is_not_a_string() {
    let harness = harness(Reply::Json(json!({"parse": {"text": 5}})));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(
        result,
        Err(Error::UnexpectedMediaWikiResponse { .. })
    ));
}

#[tokio::test]
async fn render_rejects_page_text_containing_a_mediawiki_error() {
    let harness = harness(page(
        r#"<p>x</p><strong class="error">Template loop detected</strong>"#,
    ));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(
        result,
        Err(Error::RenderingError(message)) if message.contains("Template loop detected")
    ));
    assert!(recorded(&harness.entity_inputs).is_empty());
}

#[tokio::test]
async fn render_rejects_a_scribunto_error_in_the_page_text() {
    let harness = harness(page(
        r#"<strong class="error"><span class="scribunto-error">Lua error in Module:usex</span></strong>"#,
    ));
    let result = harness.renderer.render("word", "text").await;

    assert!(matches!(
        result,
        Err(Error::RenderingError(message)) if message.contains("Lua error in Module:usex")
    ));
}

#[tokio::test]
async fn render_works_through_the_markup_renderer_trait_object() {
    let harness = harness(page("<p>Hello</p>"));
    let renderer: Box<dyn MarkupRenderer> = Box::new(harness.renderer);
    let result = renderer.render("word", "text").await;

    assert!(matches!(result, Ok(text) if text == "mock:Hello"));
}

#[tokio::test]
async fn mock_client_is_accepted_as_a_trait_object() {
    let requests: Recorded<(String, String)> = Arc::new(Mutex::new(Vec::new()));
    let client: Box<dyn MediaWikiClient> = Box::new(MockMediaWikiClient {
        reply: page("<p>x</p>"),
        requests: requests.clone(),
    });
    let result = client.request("title", "markup").await;

    assert!(matches!(result, Ok(value) if value == json!({"parse": {"text": "<p>x</p>"}})));
    assert_eq!(
        recorded(&requests),
        vec![("title".to_string(), "markup".to_string())]
    );
}

#[test]
fn fetch_object_by_key_returns_the_nested_object() {
    let value = json!({"parse": {"text": "hello"}});

    assert!(matches!(
        fetch_object_by_key(&value, "parse"),
        Ok(found) if found == &json!({"text": "hello"})
    ));
}

#[test]
fn fetch_object_by_key_rejects_a_missing_key() {
    let value = json!({"other": {}});

    assert!(matches!(
        fetch_object_by_key(&value, "parse"),
        Err(Error::UnexpectedMediaWikiResponse { message, .. })
            if message == "'parse' was missing or not an object"
    ));
}

#[test]
fn fetch_object_by_key_rejects_a_value_that_is_not_an_object() {
    let value = json!({"parse": "text"});

    assert!(matches!(
        fetch_object_by_key(&value, "parse"),
        Err(Error::UnexpectedMediaWikiResponse { .. })
    ));
}

#[test]
fn fetch_object_by_key_reports_the_response_it_rejected() {
    let value = json!({"error": {"code": "badtitle"}});

    assert!(matches!(
        fetch_object_by_key(&value, "parse"),
        Err(Error::UnexpectedMediaWikiResponse { response, .. }) if response == value.to_string()
    ));
}

#[test]
fn fetch_string_by_key_returns_the_string() {
    let value = json!({"text": "<p>hello</p>"});

    assert!(matches!(
        fetch_string_by_key(&value, "text"),
        Ok(text) if text == "<p>hello</p>"
    ));
}

#[test]
fn fetch_string_by_key_rejects_a_missing_key() {
    let value = json!({});

    assert!(matches!(
        fetch_string_by_key(&value, "text"),
        Err(Error::UnexpectedMediaWikiResponse { message, .. })
            if message == "'text' was missing or not a string"
    ));
}

#[test]
fn fetch_string_by_key_rejects_a_value_that_is_not_a_string() {
    let value = json!({"text": 5});

    assert!(matches!(
        fetch_string_by_key(&value, "text"),
        Err(Error::UnexpectedMediaWikiResponse { .. })
    ));
}

#[test]
fn check_for_wikimedia_errors_accepts_clean_html() {
    assert!(check("<p>a plain definition</p>").is_ok());
}

#[test]
fn check_for_wikimedia_errors_accepts_empty_html() {
    assert!(check("").is_ok());
}

#[test]
fn check_for_wikimedia_errors_rejects_an_error_element() {
    let result = check(r#"<strong class="error">Template broke</strong>"#);

    assert!(matches!(
        result,
        Err(Error::RenderingError(message)) if message.contains("Template broke")
    ));
}

#[test]
fn check_for_wikimedia_errors_rejects_a_scribunto_error() {
    let result = check(
        r#"<p>text</p><strong class="error"><span class="scribunto-error">Lua error in Module:x</span></strong>"#,
    );

    assert!(matches!(
        result,
        Err(Error::RenderingError(message)) if message.contains("Lua error in Module:x")
    ));
}

#[test]
fn check_for_wikimedia_errors_matches_error_among_several_classes() {
    let result = check(r#"<span class="mw-ext error">bad</span>"#);

    assert!(matches!(result, Err(Error::RenderingError(_))));
}

#[test]
fn check_for_wikimedia_errors_ignores_similar_class_names() {
    assert!(check(r#"<span class="errors">fine</span>"#).is_ok());
    assert!(check(r#"<span class="scribunto-error">fine</span>"#).is_ok());
}

#[test]
fn strip_refs_leaves_text_without_refs_alone() {
    assert_eq!(strip_refs("plain text"), "plain text");
}

#[test]
fn strip_refs_removes_a_ref_and_its_contents() {
    assert_eq!(strip_refs("a<ref>cite</ref>b"), "ab");
}

#[test]
fn strip_refs_removes_a_ref_with_attributes() {
    assert_eq!(strip_refs(r#"a<ref name="x">cite</ref>b"#), "ab");
}

#[test]
fn strip_refs_removes_self_closing_refs() {
    assert_eq!(strip_refs(r#"a<ref name="x"/>b"#), "ab");
    assert_eq!(strip_refs(r#"a<ref name="x" />b"#), "ab");
    assert_eq!(strip_refs("a<ref/>b"), "ab");
}

#[test]
fn strip_refs_removes_every_ref() {
    assert_eq!(strip_refs("a<ref>1</ref>b<ref>2</ref>c"), "abc");
}

#[test]
fn strip_refs_removes_refs_that_contain_templates() {
    assert_eq!(strip_refs("a<ref>{{cite-book|title=T}}</ref>b"), "ab");
}

#[test]
fn strip_refs_keeps_tags_that_only_start_with_ref() {
    assert_eq!(strip_refs("a<references/>b"), "a<references/>b");
    assert_eq!(strip_refs("a<reference>b"), "a<reference>b");
}

#[test]
fn strip_refs_keeps_an_unclosed_ref() {
    assert_eq!(strip_refs("a<ref>b"), "a<ref>b");
}

#[test]
fn strip_refs_keeps_an_unterminated_open_tag() {
    assert_eq!(strip_refs("a<ref"), "a<ref");
}

#[test]
fn strip_refs_handles_empty_text() {
    assert_eq!(strip_refs(""), "");
}

#[test]
fn strip_refs_preserves_non_ascii_text() {
    assert_eq!(strip_refs("héllo<ref>x</ref> wörld"), "héllo wörld");
}

#[test]
fn unwrap_math_removes_the_tags_and_keeps_the_formula() {
    assert_eq!(unwrap_math("<math>x^2</math>"), "x^2");
}

#[test]
fn unwrap_math_removes_tags_with_attributes() {
    assert_eq!(
        unwrap_math(r#"<math display="block">x</math>"#),
        "x"
    );
    assert_eq!(unwrap_math("<math chem>H2O</math>"), "H2O");
    assert_eq!(unwrap_math(r#"<math alt= "y">y</math>"#), "y");
}

#[test]
fn unwrap_math_keeps_surrounding_text() {
    assert_eq!(unwrap_math("so <math>a+b</math> is"), "so a+b is");
}

#[test]
fn unwrap_math_handles_several_formulas() {
    assert_eq!(unwrap_math("<math>a</math> and <math>b</math>"), "a and b");
}

#[test]
fn unwrap_math_ignores_tags_that_only_start_with_math() {
    assert_eq!(unwrap_math("<mathematics>"), "<mathematics>");
}

#[test]
fn unwrap_math_keeps_an_unterminated_open_tag() {
    assert_eq!(unwrap_math("a <math x"), "a <math x");
}

#[test]
fn unwrap_math_removes_a_stray_closing_tag() {
    assert_eq!(unwrap_math("a</math>b"), "ab");
}

#[test]
fn unwrap_math_leaves_text_without_math_alone() {
    assert_eq!(unwrap_math("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
}

#[test]
fn unwrap_math_handles_empty_text() {
    assert_eq!(unwrap_math(""), "");
}

#[test]
fn unwrap_math_preserves_non_ascii_text() {
    assert_eq!(unwrap_math("π <math>∑x</math> ≈ ∞"), "π ∑x ≈ ∞");
}

#[test]
fn is_entering_math_needs_a_space_or_close_bracket_after_the_name() {
    let text: Vec<char> = "<math>".chars().collect();
    assert!(is_entering_math(&text, 0));

    let text: Vec<char> = "<math x>".chars().collect();
    assert!(is_entering_math(&text, 0));

    let text: Vec<char> = "<mathx>".chars().collect();
    assert!(!is_entering_math(&text, 0));
}

#[test]
fn is_exiting_math_matches_the_closing_tag_at_the_index() {
    let text: Vec<char> = "a</math>".chars().collect();

    assert!(is_exiting_math(&text, 1));
    assert!(!is_exiting_math(&text, 0));
}
