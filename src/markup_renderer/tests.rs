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
    assert_eq!(
        params.get("prop").map(String::as_str),
        Some("text|limitreportdata")
    );
    assert!(!params.contains_key("disablelimitreport"));
    assert_eq!(params.get("wrapoutputclass").map(String::as_str), Some(""));
    assert_eq!(params.get("format").map(String::as_str), Some("json"));
    assert_eq!(params.get("formatversion").map(String::as_str), Some("2"));
}

#[test]
fn build_params_sends_exactly_seven_parameters() {
    assert_eq!(
        LocalMediaWikiClient::default()
            .build_params("word", "text")
            .len(),
        7
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

fn page_with_report(html: &str, report: Value) -> Reply {
    Reply::Json(json!({"parse": {"text": html, "limitreportdata": report}}))
}

fn full_report() -> Value {
    json!([
        {"name": "limitreport-cputime", "0": "0.074"},
        {"name": "limitreport-walltime", "0": "0.293"},
        {"name": "limitreport-ppvisitednodes", "0": 237, "1": 1000000},
        {"name": "scribunto-limitreport-timeusage", "0": "0.180", "1": "7"},
        {"name": "scribunto-limitreport-virtmemusage", "0": 38928384, "1": 524288000},
        {"name": "limitreport-timingprofile",
            "0": "100.00%  289.314      1 -total",
            "1": " 56.18%  162.545      1 Template:lb",
            "2": " 43.79%  126.690      2 Template:RQ:Dickens_Old_Curiosity_Shop"
        },
        {"name": "cachereport-ttl", "0": 86400}
    ])
}

#[tokio::test]
async fn render_report_returns_the_rendered_text() {
    let harness = harness(page_with_report("<p>hello</p>", full_report()));
    let report = harness.renderer.render_report("word", "text").await;

    assert_eq!(report.map(|r| r.text).ok(), Some("mock:hello".to_string()));
}

#[tokio::test]
async fn render_report_reads_the_parser_times() {
    let harness = harness(page_with_report("<p>x</p>", full_report()));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.as_ref().map(|r| r.parser_cpu_seconds), Some(0.074));
    assert_eq!(report.as_ref().map(|r| r.parser_wall_seconds), Some(0.293));
}

#[tokio::test]
async fn render_report_reads_the_lua_time_and_its_limit() {
    let harness = harness(page_with_report("<p>x</p>", full_report()));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.as_ref().map(|r| r.lua_seconds), Some(0.18));
    assert_eq!(report.as_ref().map(|r| r.lua_cpu_limit_seconds), Some(7.0));
}

#[tokio::test]
async fn render_report_reads_the_lua_memory_and_its_limit() {
    let harness = harness(page_with_report("<p>x</p>", full_report()));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.as_ref().map(|r| r.lua_memory_bytes), Some(38928384));
    assert_eq!(
        report.as_ref().map(|r| r.lua_memory_limit_bytes),
        Some(524288000)
    );
}

#[tokio::test]
async fn render_report_reads_template_costs_without_the_total_line() {
    let harness = harness(page_with_report("<p>x</p>", full_report()));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(
        report.map(|r| r.templates),
        Some(vec![
            TemplateCost {
                name: "Template:lb".to_string(),
                calls: 1,
                millis: 162.545
            },
            TemplateCost {
                name: "Template:RQ:Dickens_Old_Curiosity_Shop".to_string(),
                calls: 2,
                millis: 126.69
            },
        ])
    );
}

#[tokio::test]
async fn render_report_defaults_to_zeros_when_the_report_is_missing() {
    let harness = harness(page("<p>x</p>"));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(
        report,
        Some(RenderReport {
            text: "mock:x".to_string(),
            ..RenderReport::default()
        })
    );
}

#[tokio::test]
async fn render_report_ignores_unrelated_report_items() {
    let harness = harness(page_with_report(
        "<p>x</p>",
        json!([{"name": "cachereport-ttl", "0": 86400}]),
    ));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.map(|r| r.lua_memory_bytes), Some(0));
}

#[tokio::test]
async fn render_report_ignores_a_report_that_is_not_an_array() {
    let harness = harness(page_with_report("<p>x</p>", json!({"name": "oops"})));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.map(|r| r.lua_seconds), Some(0.0));
}

#[tokio::test]
async fn render_report_accepts_numeric_values_for_string_fields() {
    let harness = harness(page_with_report(
        "<p>x</p>",
        json!([{"name": "scribunto-limitreport-timeusage", "0": 0.25, "1": 7}]),
    ));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.as_ref().map(|r| r.lua_seconds), Some(0.25));
    assert_eq!(report.map(|r| r.lua_cpu_limit_seconds), Some(7.0));
}

#[tokio::test]
async fn render_report_accepts_string_values_for_integer_fields() {
    let harness = harness(page_with_report(
        "<p>x</p>",
        json!([{"name": "scribunto-limitreport-virtmemusage", "0": "1024", "1": "2048"}]),
    ));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.as_ref().map(|r| r.lua_memory_bytes), Some(1024));
    assert_eq!(report.map(|r| r.lua_memory_limit_bytes), Some(2048));
}

#[tokio::test]
async fn render_report_skips_unparseable_timing_lines() {
    let harness = harness(page_with_report(
        "<p>x</p>",
        json!([{"name": "limitreport-timingprofile",
            "0": "garbage",
            "1": " 10.00%  5.5      3 Template:ok",
            "2": " 10.00%  fast      3 Template:bad",
            "3": " 10.00%  5.5      many Template:bad2",
            "4": " 10.00%  5.5      3"
        }]),
    ));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(
        report.map(|r| r.templates),
        Some(vec![TemplateCost {
            name: "Template:ok".to_string(),
            calls: 3,
            millis: 5.5
        }])
    );
}

#[tokio::test]
async fn render_report_orders_timing_lines_numerically() {
    let lines = (0..12)
        .map(|index| {
            (
                index.to_string(),
                Value::String(format!("1.00%  1.0      {index} Template:t{index}")),
            )
        })
        .chain(std::iter::once((
            "name".to_string(),
            Value::String("limitreport-timingprofile".to_string()),
        )))
        .collect::<serde_json::Map<_, _>>();
    let harness = harness(page_with_report("<p>x</p>", json!([Value::Object(lines)])));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(
        report.map(|r| r.templates.iter().map(|t| t.calls).collect::<Vec<_>>()),
        Some((0..12).collect::<Vec<u64>>())
    );
}

#[tokio::test]
async fn render_report_keeps_template_names_with_spaces() {
    let harness = harness(page_with_report(
        "<p>x</p>",
        json!([{"name": "limitreport-timingprofile", "0": " 5.00%  2.5      1 Template:two words"}]),
    ));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(
        report.map(|r| r.templates.iter().map(|t| t.name.clone()).collect::<Vec<_>>()),
        Some(vec!["Template:two words".to_string()])
    );
}

#[tokio::test]
async fn render_report_leaves_the_report_comment_out_of_the_text() {
    let html = "<p>hello</p>\n<!-- \nNewPP limit report\nLua time usage: 0.1\n-->";
    let harness = harness(page_with_report(html, full_report()));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.map(|r| r.text), Some("mock:hello".to_string()));
}

#[tokio::test]
async fn render_report_fails_when_the_fragment_has_an_error_element() {
    let harness = harness(page_with_report(
        "<span class=\"error\">Lua error</span>",
        full_report(),
    ));
    let report = harness.renderer.render_report("word", "text").await;

    assert!(matches!(report, Err(Error::RenderingError(_))));
}

#[tokio::test]
async fn render_report_propagates_client_failures() {
    let harness = harness(Reply::Failure("boom".to_string()));
    let report = harness.renderer.render_report("word", "text").await;

    assert!(matches!(report, Err(Error::RenderingError(message)) if message == "boom"));
}

#[tokio::test]
async fn render_and_render_report_return_the_same_text() {
    let harness = harness(page_with_report("<p>same</p>", full_report()));
    let plain = harness.renderer.render("word", "text").await.ok();
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(plain, report.map(|r| r.text));
}

#[test]
fn parse_timing_line_reads_percent_millis_calls_and_name() {
    assert_eq!(
        parse_timing_line("  5.89%   17.029      3 Template:num"),
        Some(TemplateCost {
            name: "Template:num".to_string(),
            calls: 3,
            millis: 17.029
        })
    );
}

#[test]
fn parse_timing_line_rejects_the_total_line() {
    assert_eq!(parse_timing_line("100.00%  289.314      1 -total"), None);
}

#[test]
fn parse_timing_line_rejects_an_empty_line() {
    assert_eq!(parse_timing_line(""), None);
}
