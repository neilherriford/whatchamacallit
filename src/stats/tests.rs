use super::*;
use crate::markup_renderer::{Error, RenderReport, TemplateCost};
use async_trait::async_trait;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

type Calls = Arc<Mutex<Vec<(String, String)>>>;

fn record_call(calls: &Calls, title: &str, markup: &str) {
    let value = (title.to_string(), markup.to_string());
    match calls.lock() {
        Ok(mut guard) => guard.push(value),
        Err(poisoned) => poisoned.into_inner().push(value),
    }
}

fn recorded_calls(calls: &Calls) -> Vec<(String, String)> {
    match calls.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

#[derive(Clone)]
enum Reply {
    Report(RenderReport),
    Failure(String),
}

struct StubRenderer {
    reply: Reply,
    calls: Calls,
}

#[async_trait]
impl MarkupRenderer for StubRenderer {
    async fn render(&self, page_title: &str, markup: &str) -> Result<String, Error> {
        record_call(&self.calls, page_title, markup);
        Ok(String::new())
    }

    async fn render_report(&self, page_title: &str, markup: &str) -> Result<RenderReport, Error> {
        record_call(&self.calls, page_title, markup);
        match &self.reply {
            Reply::Report(report) => Ok(report.clone()),
            Reply::Failure(message) => Err(Error::RenderingError(message.clone())),
        }
    }
}

fn stub(reply: Reply) -> (StubRenderer, Calls) {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    (
        StubRenderer {
            reply,
            calls: calls.clone(),
        },
        calls,
    )
}

fn word(name: &str, markups: &[&str]) -> WordRecord {
    WordRecord {
        word: name.to_string(),
        definition_count: markups.len(),
        markups: markups.iter().map(|markup| markup.to_string()).collect(),
        ..WordRecord::default()
    }
}

fn report() -> RenderReport {
    RenderReport {
        parser_cpu_seconds: 0.8,
        parser_wall_seconds: 1.6,
        lua_seconds: 0.4,
        lua_memory_bytes: 5000,
        lua_memory_limit_bytes: 9000,
        lua_cpu_limit_seconds: 7.0,
        templates: vec![TemplateCost {
            name: "Template:lb".to_string(),
            calls: 3,
            millis: 12.5,
        }],
        ..RenderReport::default()
    }
}

async fn render_group(
    markup_renderer: &dyn MarkupRenderer,
    title: &str,
    records: &mut [WordRecord],
    observed: &mut Observed,
    templates_out: &mut dyn Write,
) -> std::io::Result<()> {
    let (rendered, elapsed) = request_group(markup_renderer, title, records).await;
    attribute(records, rendered, elapsed, observed, templates_out)
}

struct SlowRenderer {
    running: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl MarkupRenderer for SlowRenderer {
    async fn render(&self, _page_title: &str, _markup: &str) -> Result<String, Error> {
        Ok(String::new())
    }

    async fn render_report(&self, _page_title: &str, _markup: &str) -> Result<RenderReport, Error> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        Ok(RenderReport::default())
    }
}

fn slow() -> (Arc<dyn MarkupRenderer>, Arc<AtomicUsize>) {
    let peak = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(SlowRenderer {
            running: Arc::new(AtomicUsize::new(0)),
            peak: peak.clone(),
        }),
        peak,
    )
}

fn approx(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

#[test]
fn first_template_name_reads_up_to_the_first_pipe() {
    assert_eq!(
        first_template_name("{{plural of|en|dog}}"),
        Some("plural of")
    );
}

#[test]
fn first_template_name_reads_up_to_the_closing_braces() {
    assert_eq!(first_template_name("{{...}} rest"), Some("..."));
}

#[test]
fn first_template_name_skips_leading_whitespace() {
    assert_eq!(
        first_template_name("  {{inflection of|en}}"),
        Some("inflection of")
    );
}

#[test]
fn first_template_name_trims_the_name() {
    assert_eq!(first_template_name("{{ plural of |en}}"), Some("plural of"));
}

#[test]
fn first_template_name_is_none_for_plain_text() {
    assert_eq!(first_template_name("a plain definition"), None);
}

#[test]
fn first_template_name_is_none_when_a_template_is_not_first() {
    assert_eq!(first_template_name("text {{plural of|en|dog}}"), None);
}

#[test]
fn first_template_name_handles_an_unterminated_template() {
    assert_eq!(first_template_name("{{plural of"), Some("plural of"));
}

#[test]
fn first_template_name_is_none_for_empty_input() {
    assert_eq!(first_template_name(""), None);
}

#[test]
fn is_form_of_accepts_plural_of() {
    assert!(is_form_of("{{plural of|en|dog}}"));
}

#[test]
fn is_form_of_accepts_inflection_of() {
    assert!(is_form_of("{{inflection of|en|run||3|s|pres}}"));
}

#[test]
fn is_form_of_accepts_infl_of() {
    assert!(is_form_of("{{infl of|en|run||ing}}"));
}

#[test]
fn is_form_of_is_case_insensitive() {
    assert!(is_form_of("{{Plural Of|en|dog}}"));
}

#[test]
fn is_form_of_rejects_a_label_template() {
    assert!(!is_form_of("{{lb|en|informal}} meaning"));
}

#[test]
fn is_form_of_rejects_plain_text() {
    assert!(!is_form_of("a domesticated canine"));
}

#[test]
fn is_form_of_rejects_a_name_that_only_contains_of() {
    assert!(!is_form_of("{{offer|en}}"));
}

#[test]
fn is_form_of_rejects_a_form_of_template_that_is_not_first() {
    assert!(!is_form_of("meaning {{plural of|en|dog}}"));
}

#[test]
fn is_form_of_rejects_empty_input() {
    assert!(!is_form_of(""));
}

#[test]
fn csv_field_wraps_in_quotes() {
    assert_eq!(csv_field("dog"), "\"dog\"");
}

#[test]
fn csv_field_doubles_embedded_quotes() {
    assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
}

#[test]
fn csv_field_keeps_commas_inside_the_quotes() {
    assert_eq!(csv_field("a,b"), "\"a,b\"");
}

#[test]
fn csv_field_quotes_an_empty_value() {
    assert_eq!(csv_field(""), "\"\"");
}

#[test]
fn now_millis_is_after_the_year_2020() {
    assert!(now_millis() > 1_577_836_800_000);
}

#[test]
fn stopwatch_reports_the_same_elapsed_time_after_the_first_stop() {
    let mut watch = Stopwatch::start();
    let first = watch.stop();

    assert_eq!(watch.stop(), first);
}

#[test]
fn definition_separator_is_wm_dot_file_separator_symbol() {
    assert_eq!(DEFINITION_SEPARATOR, "wm.\u{241c}");
}

#[test]
fn join_definitions_places_the_separator_on_its_own_paragraph() {
    let markups = vec!["one".to_string(), "two".to_string()];

    assert_eq!(join_definitions(&markups), "one\n\nwm.\u{241c}\n\ntwo");
}

#[test]
fn join_definitions_of_one_definition_has_no_separator() {
    assert_eq!(join_definitions(&["only".to_string()]), "only");
}

#[test]
fn join_definitions_of_none_is_empty() {
    assert_eq!(join_definitions(&[]), "");
}

#[test]
fn word_separator_is_wm_dot_record_separator_symbol() {
    assert_eq!(WORD_SEPARATOR, "wm.\u{241e}");
}

#[test]
fn batch_size_is_200_words() {
    assert_eq!(BATCH_SIZE, 200);
}

#[test]
fn batch_byte_cap_is_120000_bytes() {
    assert_eq!(BATCH_MAX_BYTES, 120_000);
}

#[test]
fn batch_is_not_full_below_both_limits() {
    assert!(!batch_is_full(BATCH_SIZE - 1, BATCH_MAX_BYTES - 1));
}

#[test]
fn batch_is_full_at_the_word_limit() {
    assert!(batch_is_full(BATCH_SIZE, 10));
}

#[test]
fn batch_is_full_at_the_byte_cap() {
    assert!(batch_is_full(3, BATCH_MAX_BYTES));
}

#[test]
fn batch_is_full_above_both_limits() {
    assert!(batch_is_full(BATCH_SIZE + 30, BATCH_MAX_BYTES * 3));
}

#[test]
fn an_empty_batch_is_not_full() {
    assert!(!batch_is_full(0, 0));
}

#[test]
fn join_words_places_the_word_separator_on_its_own_paragraph() {
    let chunks = vec!["one".to_string(), "two".to_string()];

    assert_eq!(join_words(&chunks), "one\n\nwm.\u{241e}\n\ntwo");
}

#[test]
fn join_words_of_one_chunk_has_no_separator() {
    assert_eq!(join_words(&["only".to_string()]), "only");
}

#[test]
fn group_markup_nests_definition_separators_inside_word_separators() {
    let records = vec![word("a", &["a1", "a2"]), word("b", &["b1"])];

    assert_eq!(
        group_markup(&records),
        "a1\n\nwm.\u{241c}\n\na2\n\nwm.\u{241e}\n\nb1"
    );
}

#[test]
fn group_markup_of_one_word_is_its_definitions_only() {
    assert_eq!(
        group_markup(&[word("a", &["a1", "a2"])]),
        "a1\n\nwm.\u{241c}\n\na2"
    );
}

#[test]
fn format_row_lists_the_columns_in_header_order() {
    let record = WordRecord {
        word: "pi".to_string(),
        dump_position: 2000,
        elapsed_page_load: 1.5,
        elapsed_parse_markup: 2.5,
        elapsed_parse_entry: 3.5,
        definition_count: 8,
        definition_bytes: 2087,
        definition_templates: 9,
        definition_quotes: 3,
        form_of_count: 4,
        elapsed_render_time: 313.25,
        render_errors: 1,
        parser_cpu_seconds: 0.5,
        parser_wall_seconds: 0.75,
        lua_seconds: 0.25,
        lua_peak_memory_bytes: 41222144,
        ..WordRecord::default()
    };

    assert_eq!(
        format_row(&record, 99),
        "\"pi\",1.5,2.5,3.5,313.25,8,2087,9,3,2000,99,4,0,1,0.5,0.75,0.25,41222144"
    );
}

#[tokio::test]
async fn render_group_sends_the_title_and_the_group_markup() {
    let (renderer, calls) = stub(Reply::Report(report()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1", "b2"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert_eq!(
        recorded_calls(&calls),
        vec![(
            "wm.batch".to_string(),
            "a1\n\nwm.\u{241e}\n\nb1\n\nwm.\u{241c}\n\nb2".to_string()
        )]
    );
}

#[tokio::test]
async fn render_group_makes_one_request_for_the_whole_group() {
    let (renderer, calls) = stub(Reply::Report(report()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1"]), word("c", &["c1"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert_eq!(recorded_calls(&calls).len(), 1);
    assert_eq!(observed.requests, 1);
}

#[tokio::test]
async fn render_group_splits_report_times_by_definition_count() {
    let (renderer, _) = stub(Reply::Report(report()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1", "b2", "b3"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert!(approx(records[0].parser_wall_seconds, 0.4));
    assert!(approx(records[1].parser_wall_seconds, 1.2));
    assert!(approx(records[0].parser_cpu_seconds, 0.2));
    assert!(approx(records[1].parser_cpu_seconds, 0.6));
    assert!(approx(records[0].lua_seconds, 0.1));
    assert!(approx(records[1].lua_seconds, 0.3));
}

#[tokio::test]
async fn render_group_splits_elapsed_time_by_definition_count() {
    let (renderer, _) = stub(Reply::Report(report()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1", "b2", "b3"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert!(records[0].elapsed_render_time >= 0.0);
    assert!(approx(
        records[1].elapsed_render_time,
        records[0].elapsed_render_time * 3.0
    ));
}

#[tokio::test]
async fn render_group_gives_every_word_the_request_peak_memory() {
    let (renderer, _) = stub(Reply::Report(report()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert_eq!(records[0].lua_peak_memory_bytes, 5000);
    assert_eq!(records[1].lua_peak_memory_bytes, 5000);
}

#[tokio::test]
async fn render_group_records_the_lua_limits() {
    let (renderer, _) = stub(Reply::Report(report()));
    let mut records = vec![word("a", &["a1"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(&renderer, "a", &mut records, &mut observed, &mut templates)
        .await
        .unwrap_or_default();

    assert_eq!(observed.lua_memory_limit_bytes, 9000);
    assert!(approx(observed.lua_cpu_limit_seconds, 7.0));
}

#[tokio::test]
async fn render_group_writes_templates_once_under_the_first_word() {
    let (renderer, _) = stub(Reply::Report(report()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert_eq!(
        String::from_utf8_lossy(&templates),
        "\"a\",\"Template:lb\",3,12.5\n"
    );
}

#[tokio::test]
async fn render_group_quotes_template_rows_for_a_word_with_a_quote() {
    let (renderer, _) = stub(Reply::Report(report()));
    let mut records = vec![word("say \"hi\"", &["a1"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(&renderer, "x", &mut records, &mut observed, &mut templates)
        .await
        .unwrap_or_default();

    assert_eq!(
        String::from_utf8_lossy(&templates),
        "\"say \"\"hi\"\"\",\"Template:lb\",3,12.5\n"
    );
}

#[tokio::test]
async fn render_group_of_one_word_uses_the_given_title() {
    let (renderer, calls) = stub(Reply::Report(report()));
    let mut records = vec![word("third place", &["x", "y"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "third place",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert_eq!(
        recorded_calls(&calls),
        vec![(
            "third place".to_string(),
            "x\n\nwm.\u{241c}\n\ny".to_string()
        )]
    );
    assert!(approx(records[0].parser_wall_seconds, 1.6));
}

#[tokio::test]
async fn render_group_failure_counts_one_error_on_the_first_word() {
    let (renderer, _) = stub(Reply::Failure("boom".to_string()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert_eq!(records[0].render_errors, 1);
    assert_eq!(records[1].render_errors, 0);
    assert_eq!(observed.error_count, 1);
    assert_eq!(observed.requests, 1);
}

#[tokio::test]
async fn render_group_failure_writes_no_templates_and_no_report_times() {
    let (renderer, _) = stub(Reply::Failure("boom".to_string()));
    let mut records = vec![word("a", &["a1"]), word("b", &["b1"])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    render_group(
        &renderer,
        "wm.batch",
        &mut records,
        &mut observed,
        &mut templates,
    )
    .await
    .unwrap_or_default();

    assert!(templates.is_empty());
    assert!(approx(records[0].parser_wall_seconds, 0.0));
    assert_eq!(records[0].lua_peak_memory_bytes, 0);
    assert!(records[0].elapsed_render_time >= 0.0);
}

#[test]
fn attribute_without_definitions_counts_no_request() {
    let mut records = vec![word("a", &[]), word("b", &[])];
    let mut observed = Observed::default();
    let mut templates = Vec::new();

    attribute(
        &mut records,
        Ok(report()),
        10.0,
        &mut observed,
        &mut templates,
    )
    .unwrap_or_default();

    assert_eq!(observed.requests, 0);
    assert!(templates.is_empty());
}

#[test]
fn emit_row_adds_the_definition_count_to_the_total() {
    let mut total = 5;

    emit_row(&word("a", &["x", "y"]), &mut total);

    assert_eq!(total, 7);
}

#[test]
fn concurrency_is_20_requests() {
    assert_eq!(CONCURRENCY, 20);
}

#[tokio::test]
async fn dispatch_runs_up_to_ten_requests_at_once() {
    let (renderer, peak) = slow();
    let mut in_flight = JoinSet::new();
    let mut observed = Observed::default();
    let mut templates = Vec::new();
    let mut total = 0;

    for index in 0..10 {
        dispatch(
            &mut in_flight,
            &renderer,
            format!("title {index}"),
            vec![word(&format!("w{index}"), &["x"])],
        );
    }
    drain_to(&mut in_flight, 0, &mut observed, &mut templates, &mut total)
        .await
        .unwrap_or_default();

    assert_eq!(peak.load(Ordering::SeqCst), 10);
    assert_eq!(observed.requests, 10);
    assert_eq!(total, 10);
    assert!(in_flight.is_empty());
}

#[tokio::test]
async fn drain_to_waits_until_the_set_is_at_the_limit() {
    let (renderer, _) = slow();
    let mut in_flight = JoinSet::new();
    let mut observed = Observed::default();
    let mut templates = Vec::new();
    let mut total = 0;

    for index in 0..4 {
        dispatch(
            &mut in_flight,
            &renderer,
            format!("title {index}"),
            vec![word(&format!("w{index}"), &["x"])],
        );
    }
    drain_to(&mut in_flight, 2, &mut observed, &mut templates, &mut total)
        .await
        .unwrap_or_default();

    assert!(in_flight.len() <= 2);
    assert!(observed.requests >= 2);
}

#[tokio::test]
async fn drain_to_on_an_empty_set_does_nothing() {
    let mut in_flight: JoinSet<Completed> = JoinSet::new();
    let mut observed = Observed::default();
    let mut templates = Vec::new();
    let mut total = 0;

    drain_to(&mut in_flight, 0, &mut observed, &mut templates, &mut total)
        .await
        .unwrap_or_default();

    assert_eq!(observed.requests, 0);
}

#[tokio::test]
async fn dispatch_sends_the_title_and_group_markup() {
    let (stub_renderer, calls) = stub(Reply::Report(report()));
    let renderer: Arc<dyn MarkupRenderer> = Arc::new(stub_renderer);
    let mut in_flight = JoinSet::new();
    let mut observed = Observed::default();
    let mut templates = Vec::new();
    let mut total = 0;

    dispatch(
        &mut in_flight,
        &renderer,
        "wm.batch".to_string(),
        vec![word("a", &["a1"]), word("b", &["b1"])],
    );
    drain_to(&mut in_flight, 0, &mut observed, &mut templates, &mut total)
        .await
        .unwrap_or_default();

    assert_eq!(
        recorded_calls(&calls),
        vec![(
            "wm.batch".to_string(),
            "a1\n\nwm.\u{241e}\n\nb1".to_string()
        )]
    );
    assert_eq!(total, 2);
}
