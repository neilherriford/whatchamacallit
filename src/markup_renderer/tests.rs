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
async fn render_protects_math_before_sending() {
    let harness = harness(page("<p>ok</p>"));
    let _ = harness
        .renderer
        .render("word", "so <math>x^2</math> is")
        .await;

    assert_eq!(
        recorded(&harness.requests),
        vec![("word".to_string(), "so <nowiki>x^2</nowiki> is".to_string())]
    );
}

#[tokio::test]
async fn render_strips_refs_and_protects_math_in_the_same_request() {
    let harness = harness(page("<p>ok</p>"));
    let _ = harness
        .renderer
        .render("word", "<math>a</math><ref>c</ref><math>b</math>")
        .await;

    assert_eq!(
        recorded(&harness.requests),
        vec![(
            "word".to_string(),
            "<nowiki>a</nowiki><nowiki>b</nowiki>".to_string()
        )]
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
fn protect_math_wraps_the_formula_in_nowiki() {
    assert_eq!(protect_math("<math>x^2</math>"), "<nowiki>x^2</nowiki>");
}

#[test]
fn protect_math_replaces_tags_with_attributes() {
    assert_eq!(
        protect_math(r#"<math display="block">x</math>"#),
        "<nowiki>x</nowiki>"
    );
    assert_eq!(
        protect_math("<math chem>H2O</math>"),
        "<nowiki>H2O</nowiki>"
    );
    assert_eq!(
        protect_math(r#"<math alt= "y">y</math>"#),
        "<nowiki>y</nowiki>"
    );
}

#[test]
fn protect_math_keeps_surrounding_text() {
    assert_eq!(
        protect_math("so <math>a+b</math> is"),
        "so <nowiki>a+b</nowiki> is"
    );
}

#[test]
fn protect_math_handles_several_formulas() {
    assert_eq!(
        protect_math("<math>a</math> and <math>b</math>"),
        "<nowiki>a</nowiki> and <nowiki>b</nowiki>"
    );
}

#[test]
fn protect_math_keeps_equals_signs_and_pipes_inside_the_wrapper() {
    assert_eq!(
        protect_math("{{ux|en|if <math>f(x) = |x|</math>}}"),
        "{{ux|en|if <nowiki>f(x) = |x|</nowiki>}}"
    );
}

#[test]
fn protect_math_ignores_tags_that_only_start_with_math() {
    assert_eq!(protect_math("<mathematics>"), "<mathematics>");
}

#[test]
fn protect_math_keeps_an_unterminated_open_tag() {
    assert_eq!(protect_math("a <math x"), "a <math x");
}

#[test]
fn protect_math_strips_an_open_tag_that_is_never_closed() {
    assert_eq!(protect_math("a <math>b"), "a b");
}

#[test]
fn protect_math_removes_a_stray_closing_tag() {
    assert_eq!(protect_math("a</math>b"), "ab");
}

#[test]
fn protect_math_strips_a_nested_open_tag_inside_a_formula() {
    assert_eq!(protect_math("<math>a<math>b</math>"), "<nowiki>ab</nowiki>");
}

#[test]
fn protect_math_leaves_text_without_math_alone() {
    assert_eq!(protect_math("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
}

#[test]
fn protect_math_handles_empty_text() {
    assert_eq!(protect_math(""), "");
}

#[test]
fn protect_math_preserves_non_ascii_text() {
    assert_eq!(
        protect_math("π <math>∑x</math> ≈ ∞"),
        "π <nowiki>∑x</nowiki> ≈ ∞"
    );
}

#[test]
fn has_closing_math_finds_a_later_closing_tag() {
    let text: Vec<char> = "a<math>b</math>".chars().collect();

    assert!(has_closing_math(&text, 7));
    assert!(!has_closing_math(&text, 12));
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
async fn render_report_reads_the_luasandbox_memory_item_when_there_is_no_virtual_size() {
    let harness = harness(page_with_report(
        "<p>x</p>",
        json!([{"name": "scribunto-limitreport-memusage", "0": 7849362, "1": 524288000}]),
    ));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.as_ref().map(|r| r.lua_memory_bytes), Some(7849362));
    assert_eq!(report.map(|r| r.lua_memory_limit_bytes), Some(524288000));
}

#[tokio::test]
async fn render_report_prefers_the_virtual_size_item_when_both_are_present() {
    let harness = harness(page_with_report(
        "<p>x</p>",
        json!([
            {"name": "scribunto-limitreport-memusage", "0": 1, "1": 2},
            {"name": "scribunto-limitreport-virtmemusage", "0": 3, "1": 4}
        ]),
    ));
    let report = harness.renderer.render_report("word", "text").await.ok();

    assert_eq!(report.as_ref().map(|r| r.lua_memory_bytes), Some(3));
    assert_eq!(report.map(|r| r.lua_memory_limit_bytes), Some(4));
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
        report.map(|r| r
            .templates
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>()),
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

#[test]
fn remove_score_removes_the_tag_and_its_content() {
    assert_eq!(remove_score("a <score>c d e</score> b"), "a  b");
}

#[test]
fn remove_score_removes_a_tag_with_attributes() {
    assert_eq!(
        remove_score(r#"x <score sound="1" raw>{\clef bass}</score> y"#),
        "x  y"
    );
}

#[test]
fn remove_score_removes_a_self_closing_tag() {
    assert_eq!(remove_score("x <score a=\"b\"/> y"), "x  y");
}

#[test]
fn remove_score_removes_several_tags() {
    assert_eq!(remove_score("<score>a</score>m<score>b</score>"), "m");
}

#[test]
fn remove_score_keeps_text_without_a_score_tag() {
    assert_eq!(
        remove_score("plain {{ux|en|x}} text"),
        "plain {{ux|en|x}} text"
    );
}

#[test]
fn remove_score_ignores_tags_that_only_start_with_score() {
    assert_eq!(remove_score("<scores>a</scores>"), "<scores>a</scores>");
}

#[test]
fn remove_score_keeps_a_tag_that_is_never_closed() {
    assert_eq!(remove_score("a <score>b"), "a <score>b");
}

#[test]
fn remove_score_handles_empty_text() {
    assert_eq!(remove_score(""), "");
}

#[test]
fn remove_score_preserves_non_ascii_text() {
    assert_eq!(remove_score("π <score>∑</score> ≈ ∞"), "π  ≈ ∞");
}

#[test]
fn has_unterminated_template_is_false_for_balanced_templates() {
    assert!(!has_unterminated_template(
        "{{lb|en|x}} text {{plural of|en|y}}"
    ));
}

#[test]
fn has_unterminated_template_is_true_for_an_open_template() {
    assert!(has_unterminated_template("{{quote-text|en|year=1819|"));
}

#[test]
fn has_unterminated_template_is_true_when_a_nested_template_is_left_open() {
    assert!(has_unterminated_template("{{a|{{b|c}}"));
}

#[test]
fn has_unterminated_template_ignores_a_stray_closing_pair() {
    assert!(!has_unterminated_template("text }} more {{a}}"));
}

#[test]
fn has_unterminated_template_is_false_for_plain_text() {
    assert!(!has_unterminated_template("a { b } c"));
}

#[test]
fn has_unterminated_template_is_false_for_empty_text() {
    assert!(!has_unterminated_template(""));
}

#[test]
fn has_unterminated_template_ignores_braces_inside_nowiki() {
    assert!(!has_unterminated_template(
        "{{ux|en|<nowiki>x_{i_{j}} {{</nowiki>}}"
    ));
}

#[test]
fn has_unterminated_template_ignores_an_unclosed_nowiki_remainder() {
    assert!(!has_unterminated_template("a <nowiki>{{b"));
}

#[test]
fn prepare_markup_strips_refs_removes_scores_and_protects_math() {
    assert_eq!(
        prepare_markup("a<ref>c</ref> <score>s</score><math>x=1</math>"),
        "a <nowiki>x=1</nowiki>"
    );
}

#[test]
fn prepare_markup_leaves_clean_markup_unchanged() {
    assert_eq!(prepare_markup("{{lb|en|x}} text"), "{{lb|en|x}} text");
}

#[test]
fn has_error_class_matches_the_exact_token() {
    assert!(has_error_class(r#"<strong class="error""#));
    assert!(has_error_class(r#"<span class="mw-x error mw-ext""#));
}

#[test]
fn has_error_class_ignores_similar_class_names() {
    assert!(!has_error_class(r#"<span class="scribunto-error""#));
    assert!(!has_error_class(r#"<span class="errors""#));
    assert!(!has_error_class("<span>"));
}

#[test]
fn error_starts_lists_the_start_of_each_error_element() {
    let html = r#"<p>a</p><strong class="error">x</strong><p><span class="error">y</span></p>"#;

    assert_eq!(error_starts(html), vec![8, 43]);
}

#[test]
fn error_starts_is_empty_without_errors() {
    assert!(error_starts("<p>ok</p><span class=\"x\">y</span>").is_empty());
}

#[test]
fn error_message_reads_the_text_inside_the_element() {
    let html = r#"<p>a</p><strong class="error">Lua error in Module:x: bad</strong>"#;

    assert_eq!(error_message(html, 8), "Lua error in Module:x: bad");
}

#[test]
fn error_message_reads_through_a_nested_element() {
    let html = r#"<strong class="error"><span class="scribunto-error" id="e">Lua error: nested</span></strong>"#;

    assert_eq!(error_message(html, 0), "Lua error: nested");
}

#[test]
fn error_message_collapses_whitespace() {
    let html = "<span class=\"error\">a\n   b</span>";

    assert_eq!(error_message(html, 0), "a b");
}

#[test]
fn error_message_is_limited_to_four_hundred_characters() {
    let html = format!("<span class=\"error\">{}</span>", "x".repeat(900));

    assert_eq!(error_message(&html, 0).chars().count(), 400);
}

const SEP: &str = "wm.\u{241f}";

fn definitions_html(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| format!("<p>{part}</p>"))
        .collect::<Vec<_>>()
        .join(&format!("\n<p>{SEP}</p>\n"))
}

#[test]
fn split_response_returns_each_definition_rendered() {
    let harness = harness(page("<p>x</p>"));
    let html = definitions_html(&["one", "two", "three"]);

    assert_eq!(
        harness.renderer.split_response(&html, 3).ok(),
        Some(vec![
            DefinitionOutcome::Rendered("mock:one".to_string()),
            DefinitionOutcome::Rendered("mock:two".to_string()),
            DefinitionOutcome::Rendered("mock:three".to_string()),
        ])
    );
}

#[test]
fn split_response_marks_only_the_definition_containing_an_error() {
    let harness = harness(page("<p>x</p>"));
    let html = definitions_html(&[
        "one",
        r#"<strong class="error">Lua error: bad</strong>"#,
        "three",
    ]);

    assert_eq!(
        harness.renderer.split_response(&html, 3).ok(),
        Some(vec![
            DefinitionOutcome::Rendered("mock:one".to_string()),
            DefinitionOutcome::Failed("mock:Lua error: bad".to_string()),
            DefinitionOutcome::Rendered("mock:three".to_string()),
        ])
    );
}

#[test]
fn split_response_marks_the_first_and_last_definitions() {
    let harness = harness(page("<p>x</p>"));
    let html = definitions_html(&[
        r#"<span class="error">first</span>"#,
        "middle",
        r#"<span class="error">last</span>"#,
    ]);

    assert_eq!(
        harness.renderer.split_response(&html, 3).ok(),
        Some(vec![
            DefinitionOutcome::Failed("mock:first".to_string()),
            DefinitionOutcome::Rendered("mock:middle".to_string()),
            DefinitionOutcome::Failed("mock:last".to_string()),
        ])
    );
}

#[test]
fn split_response_keeps_the_first_error_of_a_definition() {
    let harness = harness(page("<p>x</p>"));
    let html =
        definitions_html(&[r#"<span class="error">one</span> <span class="error">two</span>"#]);

    assert_eq!(
        harness.renderer.split_response(&html, 1).ok(),
        Some(vec![DefinitionOutcome::Failed("mock:one".to_string())])
    );
}

#[test]
fn split_response_fails_when_the_definition_count_is_wrong() {
    let harness = harness(page("<p>x</p>"));
    let html = definitions_html(&["one", "two"]);

    assert!(harness.renderer.split_response(&html, 3).is_err());
}

#[test]
fn split_response_of_one_definition_needs_no_separator() {
    let harness = harness(page("<p>x</p>"));

    assert_eq!(
        harness.renderer.split_response("<p>only</p>", 1).ok(),
        Some(vec![DefinitionOutcome::Rendered("mock:only".to_string())])
    );
}

#[tokio::test]
async fn render_definitions_sends_prepared_definitions_joined_by_the_definition_separator() {
    let harness = harness(page(&definitions_html(&["a", "b"])));
    let definitions = vec!["a<ref>r</ref>".to_string(), "<math>x=1</math>".to_string()];
    let _ = harness
        .renderer
        .render_definitions("wm.batch", &definitions)
        .await;

    assert_eq!(
        recorded(&harness.requests),
        vec![(
            "wm.batch".to_string(),
            format!("a\n\n{SEP}\n\n<nowiki>x=1</nowiki>")
        )]
    );
}

#[tokio::test]
async fn render_definitions_returns_the_outcome_of_every_definition() {
    let harness = harness(page(&definitions_html(&[
        "a",
        r#"<span class="error">bad</span>"#,
    ])));
    let definitions = vec!["one".to_string(), "two".to_string()];

    assert_eq!(
        harness
            .renderer
            .render_definitions("wm.batch", &definitions)
            .await
            .ok(),
        Some(vec![
            DefinitionOutcome::Rendered("mock:a".to_string()),
            DefinitionOutcome::Failed("mock:bad".to_string()),
        ])
    );
}

#[tokio::test]
async fn render_definitions_of_no_definitions_makes_no_request() {
    let harness = harness(page("<p>x</p>"));

    assert_eq!(
        harness
            .renderer
            .render_definitions("wm.batch", &[])
            .await
            .ok(),
        Some(Vec::new())
    );
    assert!(recorded(&harness.requests).is_empty());
}

#[tokio::test]
async fn render_definitions_fails_when_the_count_does_not_match() {
    let harness = harness(page("<p>only one</p>"));
    let definitions = vec!["a".to_string(), "b".to_string()];

    assert!(
        harness
            .renderer
            .render_definitions("wm.batch", &definitions)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn render_definitions_passes_a_client_failure_through() {
    let harness = harness(Reply::Failure("down".to_string()));
    let definitions = vec!["a".to_string()];

    assert!(
        harness
            .renderer
            .render_definitions("wm.batch", &definitions)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn render_definitions_fails_when_the_response_has_no_parse_object() {
    let harness = harness(Reply::Json(json!({"error": "bad"})));
    let definitions = vec!["a".to_string()];

    assert!(
        harness
            .renderer
            .render_definitions("wm.batch", &definitions)
            .await
            .is_err()
    );
}

#[test]
fn strip_style_elements_removes_the_element_and_its_css() {
    assert_eq!(
        strip_style_elements("<p>a</p><style>.x{color:red}</style><p>b</p>"),
        "<p>a</p><p>b</p>"
    );
}

#[test]
fn strip_style_elements_removes_an_element_with_attributes() {
    assert_eq!(
        strip_style_elements(r#"a<style data-mw-deduplicate="TemplateStyles:r1">.x{}</style>b"#),
        "ab"
    );
}

#[test]
fn strip_style_elements_removes_several_elements() {
    assert_eq!(
        strip_style_elements("<style>a</style>m<style>b</style>"),
        "m"
    );
}

#[test]
fn strip_style_elements_keeps_html_without_style() {
    assert_eq!(strip_style_elements("<p>plain</p>"), "<p>plain</p>");
}

#[test]
fn strip_style_elements_ignores_tags_that_only_start_with_style() {
    assert_eq!(
        strip_style_elements("<styles>a</styles>"),
        "<styles>a</styles>"
    );
}

#[test]
fn strip_style_elements_keeps_an_element_that_is_never_closed() {
    assert_eq!(strip_style_elements("a<style>b"), "a<style>b");
}

#[test]
fn strip_style_elements_handles_empty_html() {
    assert_eq!(strip_style_elements(""), "");
}

#[test]
fn split_response_leaves_style_css_out_of_the_text() {
    let harness = harness(page("<p>x</p>"));
    let html = format!(
        "<style>.mw-parser-output .defdate{{font-size:smaller}}</style>{}",
        definitions_html(&["one", "two"])
    );

    assert_eq!(
        harness.renderer.split_response(&html, 2).ok(),
        Some(vec![
            DefinitionOutcome::Rendered("mock:one".to_string()),
            DefinitionOutcome::Rendered("mock:two".to_string()),
        ])
    );
}

#[tokio::test]
async fn render_leaves_style_css_out_of_the_text() {
    let harness = harness(page("<style>.x{color:red}</style><p>visible</p>"));

    assert_eq!(
        harness.renderer.render("word", "text").await.ok(),
        Some("mock:visible".to_string())
    );
}

#[test]
fn error_message_reads_through_links_inside_the_element() {
    let html = r#"<strong class="error">Lua error in <a href="/x">Module:y</a> at <a href="/z">line 247</a>: bad value</strong> after"#;

    assert_eq!(
        error_message(html, 0),
        "Lua error in Module:y at line 247: bad value"
    );
}

#[test]
fn error_message_reads_through_nested_elements_of_the_same_name() {
    let html = r#"<span class="error">a <span>b</span> c</span> tail"#;

    assert_eq!(error_message(html, 0), "a b c");
}

#[test]
fn error_message_stops_at_the_end_of_the_error_element() {
    let html = r#"<p><span class="error">bad</span> and more text</p>"#;

    assert_eq!(error_message(html, 3), "bad");
}

#[test]
fn error_message_of_an_element_that_is_never_closed_reads_to_the_end() {
    assert_eq!(error_message(r#"<span class="error">open"#, 0), "open");
}

#[test]
fn error_message_of_a_truncated_start_tag_is_empty() {
    assert_eq!(error_message(r#"<span class="error""#, 0), "");
}

fn missing_link(name: &str) -> String {
    format!(
        r#"<a href="/index.php?title=Template:x&amp;action=edit&amp;redlink=1" class="new" title="Template:{name} (page does not exist)">Template:{name}</a>"#
    )
}

#[test]
fn missing_templates_names_a_template_page_that_does_not_exist() {
    assert_eq!(
        missing_templates(&missing_link("no such template")),
        vec![(0, "no such template".to_string())]
    );
}

#[test]
fn missing_templates_ignores_a_template_page_that_exists() {
    let html = r#"<a href="/index.php/Template:lb" title="Template:lb">lb</a>"#;

    assert!(missing_templates(html).is_empty());
}

#[test]
fn missing_templates_ignores_a_missing_page_outside_the_template_namespace() {
    let html = r#"<a class="new" title="w:Edward Lear (page does not exist)">Edward Lear</a>"#;

    assert!(missing_templates(html).is_empty());
}

#[test]
fn missing_templates_reports_the_definition_each_one_is_in() {
    let html = definitions_html(&[&missing_link("one"), &missing_link("two")]);

    assert_eq!(
        missing_templates(&html),
        vec![(0, "one".to_string()), (1, "two".to_string())]
    );
}

#[test]
fn missing_templates_of_text_without_links_is_empty() {
    assert!(missing_templates("<p>plain</p>").is_empty());
}

#[test]
fn calls_template_finds_a_call_by_name() {
    assert!(calls_template("a {{quote song|en|x=1}} b", "quote song"));
}

#[test]
fn calls_template_allows_spaces_after_the_braces() {
    assert!(calls_template("{{ quote song }}", "quote song"));
}

#[test]
fn calls_template_reads_underscores_as_spaces() {
    assert!(calls_template("{{quote_song|en}}", "quote song"));
}

#[test]
fn calls_template_ignores_plain_text() {
    assert!(!calls_template(
        "see Template:quote song here",
        "quote song"
    ));
}

#[test]
fn calls_template_ignores_a_link_to_the_page() {
    assert!(!calls_template("[[Template:quote song]]", "quote song"));
}

#[tokio::test]
async fn render_definitions_fails_only_the_definition_that_calls_a_missing_template() {
    let harness = harness(page(&definitions_html(&["fine", &missing_link("gone")])));
    let definitions = vec!["a".to_string(), "{{gone|x}}".to_string()];

    let found = harness
        .renderer
        .render_definitions("wm.batch", &definitions)
        .await;

    assert_eq!(
        found.ok(),
        Some(vec![
            DefinitionOutcome::Rendered("mock:fine".to_string()),
            DefinitionOutcome::Failed("Missing template: gone".to_string()),
        ])
    );
}

#[tokio::test]
async fn render_definitions_keeps_a_definition_that_only_links_to_a_missing_template_page() {
    let harness = harness(page(&definitions_html(&["a", &missing_link("gone")])));
    let definitions = vec!["a".to_string(), "see [[Template:gone]]".to_string()];

    let found = harness
        .renderer
        .render_definitions("wm.batch", &definitions)
        .await;

    assert!(matches!(
        found.ok().as_deref(),
        Some([
            DefinitionOutcome::Rendered(_),
            DefinitionOutcome::Rendered(_)
        ])
    ));
}

#[tokio::test]
async fn render_definitions_accepts_a_link_to_a_missing_page_outside_the_template_namespace() {
    let harness = harness(page(&definitions_html(&[
        r#"<a class="new" title="w:Edward Lear (page does not exist)">Edward Lear</a>"#,
        "b",
    ])));
    let definitions = vec!["{{w|Edward Lear}}".to_string(), "b".to_string()];

    let found = harness
        .renderer
        .render_definitions("wm.batch", &definitions)
        .await;

    assert!(matches!(
        found.ok().as_deref(),
        Some([
            DefinitionOutcome::Rendered(_),
            DefinitionOutcome::Rendered(_)
        ])
    ));
}
