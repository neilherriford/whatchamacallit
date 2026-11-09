use super::*;
use crate::{failure_log::read_tsv, markup_renderer::Error};
use async_trait::async_trait;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

type Calls = Arc<Mutex<Vec<(String, Vec<String>)>>>;
type Handler = Box<dyn Fn(&str, &[String]) -> Result<Vec<DefinitionOutcome>, Error> + Send + Sync>;

fn record(calls: &Calls, title: &str, definitions: &[String]) {
    let value = (title.to_string(), definitions.to_vec());
    match calls.lock() {
        Ok(mut guard) => guard.push(value),
        Err(poisoned) => poisoned.into_inner().push(value),
    }
}

fn recorded(calls: &Calls) -> Vec<(String, Vec<String>)> {
    let mut found = match calls.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    found.sort();
    found
}

struct FnRenderer {
    handler: Handler,
    calls: Calls,
}

#[async_trait]
impl DefinitionRenderer for FnRenderer {
    async fn render_definitions(
        &self,
        page_title: &str,
        definitions: &[String],
    ) -> Result<Vec<DefinitionOutcome>, Error> {
        record(&self.calls, page_title, definitions);
        (self.handler)(page_title, definitions)
    }
}

fn echo(definitions: &[String]) -> Vec<DefinitionOutcome> {
    definitions
        .iter()
        .map(|markup| {
            if markup.contains("FAIL") {
                DefinitionOutcome::Failed(format!("err:{markup}"))
            } else {
                DefinitionOutcome::Rendered(format!("r:{markup}"))
            }
        })
        .collect()
}

fn renderer(handler: Handler) -> (Arc<dyn DefinitionRenderer>, Calls) {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    (
        Arc::new(FnRenderer {
            handler,
            calls: calls.clone(),
        }),
        calls,
    )
}

fn echo_renderer() -> (Arc<dyn DefinitionRenderer>, Calls) {
    renderer(Box::new(|_, definitions| Ok(echo(definitions))))
}

struct MarkerDependency;

impl TitleDependency for MarkerDependency {
    fn requires_title(&self, markup: &str) -> bool {
        markup.contains("TITLE")
    }
}

#[derive(Default)]
struct MemorySink {
    words: Vec<RenderedWord>,
}

impl RenderedSink for MemorySink {
    fn write_word(&mut self, word: &RenderedWord) -> io::Result<()> {
        self.words.push(word.clone());
        Ok(())
    }
}

fn definition(markup: &str) -> Definition {
    Definition {
        part_of_speech: PartOfSpeech::Verb,
        labels: Vec::new(),
        markup: markup.to_string(),
        antonyms: Vec::new(),
    }
}

fn entry(word: &str, markups: &[&str]) -> WordEntry {
    WordEntry {
        word: word.to_string(),
        definitions: markups
            .iter()
            .enumerate()
            .map(|(number, markup)| NumberedDefinition {
                number,
                definition: definition(markup),
            })
            .collect(),
    }
}

fn small(batch_size: usize) -> PipelineConfig {
    PipelineConfig {
        concurrency: 2,
        batch_size,
        batch_max_bytes: 1_000_000,
    }
}

struct Outcome {
    summary: PipelineSummary,
    words: Vec<RenderedWord>,
    failures: Vec<crate::failure_log::LoggedFailure>,
}

async fn run_with(
    words: Vec<WordEntry>,
    renderer: Arc<dyn DefinitionRenderer>,
    config: PipelineConfig,
    known: HashMap<String, String>,
) -> Outcome {
    let mut sink = MemorySink::default();
    let mut log = FailureLog::default();
    let summary = run(
        words.into_iter(),
        renderer,
        &MarkerDependency,
        config,
        &known,
        &mut sink,
        &mut log,
    )
    .await
    .unwrap_or_default();

    let mut buffer = Vec::new();
    log.write_tsv(&mut buffer, "tag").unwrap_or_default();
    let mut words = sink.words;
    words.sort_by(|left, right| left.word.cmp(&right.word));

    Outcome {
        summary,
        words,
        failures: read_tsv(&String::from_utf8_lossy(&buffer)),
    }
}

async fn simple(words: Vec<WordEntry>, renderer: Arc<dyn DefinitionRenderer>) -> Outcome {
    run_with(words, renderer, PipelineConfig::default(), HashMap::new()).await
}

#[test]
fn default_config_is_twenty_requests_of_two_hundred_words_and_120_kilobytes() {
    let config = PipelineConfig::default();

    assert_eq!(config.concurrency, 20);
    assert_eq!(config.batch_size, 200);
    assert_eq!(config.batch_max_bytes, 120_000);
}

#[test]
fn batch_title_is_wm_dot_batch() {
    assert_eq!(BATCH_TITLE, "wm.batch");
}

#[test]
fn batch_is_not_full_below_both_limits() {
    let config = small(3);

    assert!(!batch_is_full(&config, 2, 10));
}

#[test]
fn batch_is_full_at_the_word_limit() {
    assert!(batch_is_full(&small(3), 3, 10));
}

#[test]
fn batch_is_full_at_the_byte_limit() {
    let config = PipelineConfig {
        batch_max_bytes: 100,
        ..small(50)
    };

    assert!(batch_is_full(&config, 1, 100));
}

#[test]
fn collect_definitions_skips_definitions_that_are_already_resolved() {
    let mut work = prepare_work(entry("w", &["a", "b", "c"]), &HashMap::new());
    work.resolved[1] = Some(Resolved::Text("done".to_string()));

    let (definitions, owners) = collect_definitions(&[work]);

    assert_eq!(definitions, vec!["a".to_string(), "c".to_string()]);
    assert_eq!(owners, vec![(0, 0), (0, 2)]);
}

#[test]
fn collect_definitions_numbers_owners_by_word_and_definition_position() {
    let first = prepare_work(entry("a", &["x"]), &HashMap::new());
    let second = prepare_work(entry("b", &["y", "z"]), &HashMap::new());

    let (_, owners) = collect_definitions(&[first, second]);

    assert_eq!(owners, vec![(0, 0), (1, 0), (1, 1)]);
}

#[test]
fn prepare_work_resolves_an_unterminated_template() {
    let work = prepare_work(entry("w", &["{{oops", "fine"]), &HashMap::new());

    assert!(matches!(
        &work.resolved[0],
        Some(Resolved::Failed(FailureKind::Error, message)) if message.contains("unterminated")
    ));
    assert!(work.resolved[1].is_none());
}

#[test]
fn prepare_work_resolves_a_known_failure_with_its_old_error() {
    let known = HashMap::from([(markup_hash("bad"), "old error".to_string())]);

    let work = prepare_work(entry("w", &["bad", "good"]), &known);

    assert_eq!(
        work.resolved[0],
        Some(Resolved::Failed(
            FailureKind::Error,
            "old error".to_string()
        ))
    );
    assert!(work.resolved[1].is_none());
}

#[test]
fn prepare_work_ignores_braces_inside_math() {
    let work = prepare_work(entry("w", &["<math>x_{i_{j}}</math>"]), &HashMap::new());

    assert!(work.resolved[0].is_none());
}

#[tokio::test]
async fn words_are_batched_together_under_the_shared_title() {
    let (renderer, calls) = echo_renderer();

    simple(
        vec![
            entry("a", &["a1", "a2"]),
            entry("b", &["b1"]),
            entry("c", &["c1"]),
        ],
        renderer,
    )
    .await;

    assert_eq!(
        recorded(&calls),
        vec![(
            "wm.batch".to_string(),
            vec![
                "a1".to_string(),
                "a2".to_string(),
                "b1".to_string(),
                "c1".to_string()
            ]
        )]
    );
}

#[tokio::test]
async fn a_title_dependent_word_is_rendered_alone_under_its_own_title() {
    let (renderer, calls) = echo_renderer();

    let outcome = simple(
        vec![
            entry("a", &["x"]),
            entry("b", &["TITLE y", "TITLE z"]),
            entry("c", &["w"]),
        ],
        renderer,
    )
    .await;

    assert_eq!(
        recorded(&calls),
        vec![
            (
                "b".to_string(),
                vec!["TITLE y".to_string(), "TITLE z".to_string()]
            ),
            (
                "wm.batch".to_string(),
                vec!["x".to_string(), "w".to_string()]
            ),
        ]
    );
    assert_eq!(outcome.summary.solo_words, 1);
}

#[tokio::test]
async fn a_batch_is_sent_when_the_word_limit_is_reached() {
    let (renderer, calls) = echo_renderer();
    let words = (0..5).map(|n| entry(&format!("w{n}"), &["x"])).collect();

    run_with(words, renderer, small(2), HashMap::new()).await;

    let sizes = recorded(&calls)
        .iter()
        .map(|(_, definitions)| definitions.len())
        .collect::<Vec<_>>();
    assert_eq!(sizes, vec![1, 2, 2]);
}

#[tokio::test]
async fn a_batch_is_sent_when_the_byte_limit_is_reached() {
    let (renderer, calls) = echo_renderer();
    let config = PipelineConfig {
        batch_max_bytes: 10,
        ..small(100)
    };
    let words = vec![
        entry("a", &["123456"]),
        entry("b", &["123456"]),
        entry("c", &["1"]),
    ];

    run_with(words, renderer, config, HashMap::new()).await;

    let sizes = recorded(&calls)
        .iter()
        .map(|(_, definitions)| definitions.len())
        .collect::<Vec<_>>();
    assert_eq!(sizes, vec![1, 2]);
}

#[tokio::test]
async fn rendered_definitions_are_written_in_their_original_order() {
    let (renderer, _) = echo_renderer();

    let outcome = simple(vec![entry("a", &["one", "two", "three"])], renderer).await;

    let texts = outcome.words[0]
        .definitions
        .iter()
        .map(|definition| definition.text.clone())
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["r:one", "r:two", "r:three"]);
}

#[tokio::test]
async fn a_failed_definition_is_logged_and_its_neighbours_are_kept() {
    let (renderer, _) = echo_renderer();

    let outcome = simple(
        vec![
            entry("cat", &["one", "FAIL two", "three"]),
            entry("dog", &["four"]),
        ],
        renderer,
    )
    .await;

    let cat = &outcome.words[0];
    assert_eq!(cat.word, "cat");
    assert_eq!(
        cat.definitions.iter().map(|d| d.number).collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(outcome.words[1].definitions[0].text, "r:four");
    assert_eq!(outcome.failures.len(), 1);
    assert_eq!(outcome.failures[0].failure.word, "cat");
    assert_eq!(outcome.failures[0].failure.definition_number, 1);
    assert_eq!(outcome.failures[0].failure.kind, FailureKind::Error);
    assert_eq!(outcome.failures[0].failure.error, "err:FAIL two");
    assert_eq!(outcome.failures[0].failure.markup, "FAIL two");
}

#[tokio::test]
async fn a_word_whose_definitions_all_fail_is_not_written() {
    let (renderer, _) = echo_renderer();

    let outcome = simple(vec![entry("w", &["FAIL a", "FAIL b"])], renderer).await;

    assert!(outcome.words.is_empty());
    assert_eq!(outcome.failures.len(), 2);
    assert_eq!(outcome.summary.failed, 2);
}

#[tokio::test]
async fn a_definition_with_an_unterminated_template_is_logged_and_never_sent() {
    let (renderer, calls) = echo_renderer();

    let outcome = simple(vec![entry("w", &["{{oops", "fine"])], renderer).await;

    assert_eq!(
        recorded(&calls),
        vec![("wm.batch".to_string(), vec!["fine".to_string()])]
    );
    assert_eq!(outcome.failures.len(), 1);
    assert!(outcome.failures[0].failure.error.contains("unterminated"));
    assert_eq!(outcome.words[0].definitions.len(), 1);
}

#[tokio::test]
async fn a_known_failure_is_skipped_and_logged_with_its_old_error() {
    let (renderer, calls) = echo_renderer();
    let known = HashMap::from([(markup_hash("bad one"), "old error".to_string())]);

    let outcome = run_with(
        vec![entry("w", &["bad one", "good"])],
        renderer,
        PipelineConfig::default(),
        known,
    )
    .await;

    assert_eq!(
        recorded(&calls),
        vec![("wm.batch".to_string(), vec!["good".to_string()])]
    );
    assert_eq!(outcome.failures[0].failure.error, "old error");
    assert_eq!(outcome.summary.failed, 1);
}

#[tokio::test]
async fn a_word_with_only_known_failures_makes_no_request() {
    let (renderer, calls) = echo_renderer();
    let known = HashMap::from([(markup_hash("bad"), "old".to_string())]);

    run_with(
        vec![entry("w", &["bad"])],
        renderer,
        PipelineConfig::default(),
        known,
    )
    .await;

    assert!(recorded(&calls).is_empty());
}

#[tokio::test]
async fn a_failed_request_is_retried_once_and_then_succeeds() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let (renderer, _) = renderer(Box::new(move |_, definitions| {
        if counter.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(Error::RenderingError("hiccup".to_string()))
        } else {
            Ok(echo(definitions))
        }
    }));

    let outcome = simple(vec![entry("a", &["x", "y"])], renderer).await;

    assert_eq!(outcome.summary.requests, 2);
    assert_eq!(outcome.summary.rendered, 2);
    assert!(outcome.failures.is_empty());
}

#[tokio::test]
async fn a_failing_request_is_split_to_isolate_the_bad_definition() {
    let (renderer, _) = renderer(Box::new(|_, definitions| {
        if definitions.iter().any(|markup| markup.contains("POISON")) {
            Err(Error::RenderingError("down".to_string()))
        } else {
            Ok(echo(definitions))
        }
    }));

    let outcome = simple(vec![entry("w", &["a", "POISON", "b", "c"])], renderer).await;

    assert_eq!(outcome.summary.rendered, 3);
    assert_eq!(outcome.summary.transient, 1);
    assert_eq!(outcome.summary.failed, 0);
    assert_eq!(outcome.summary.requests, 9);
    assert_eq!(outcome.failures[0].failure.kind, FailureKind::Transient);
    assert_eq!(outcome.failures[0].failure.markup, "POISON");
    assert_eq!(
        outcome.failures[0].failure.error,
        "Failed to render fragment: down"
    );
}

#[tokio::test]
async fn a_wrong_outcome_count_is_handled_like_a_failed_request() {
    let (renderer, _) = renderer(Box::new(|_, definitions| {
        if definitions.len() > 1 {
            Ok(vec![DefinitionOutcome::Rendered("short".to_string())])
        } else {
            Ok(echo(definitions))
        }
    }));

    let outcome = simple(vec![entry("w", &["a", "b"])], renderer).await;

    assert_eq!(outcome.summary.rendered, 2);
    assert_eq!(outcome.summary.requests, 4);
    assert!(outcome.failures.is_empty());
}

#[tokio::test]
async fn render_resilient_of_no_definitions_makes_no_request() {
    let (renderer, calls) = echo_renderer();

    let (outcomes, requests) = render_resilient(renderer.as_ref(), "t", &[]).await;

    assert!(outcomes.is_empty());
    assert_eq!(requests, 0);
    assert!(recorded(&calls).is_empty());
}

#[tokio::test]
async fn render_resilient_returns_outcomes_in_definition_order() {
    let (renderer, _) = echo_renderer();
    let definitions = vec!["a".to_string(), "FAIL b".to_string(), "c".to_string()];

    let (outcomes, requests) = render_resilient(renderer.as_ref(), "t", &definitions).await;

    assert_eq!(requests, 2);
    assert_eq!(
        outcomes,
        vec![
            Resolved::Text("r:a".to_string()),
            Resolved::Failed(FailureKind::Error, "err:FAIL b".to_string()),
            Resolved::Text("r:c".to_string()),
        ]
    );
}

#[tokio::test]
async fn definitions_keep_their_numbers_part_of_speech_labels_and_antonyms() {
    let (renderer, _) = echo_renderer();
    let word = WordEntry {
        word: "w".to_string(),
        definitions: vec![NumberedDefinition {
            number: 5,
            definition: Definition {
                part_of_speech: PartOfSpeech::Adjective,
                labels: vec!["informal".to_string()],
                markup: "text".to_string(),
                antonyms: vec!["cold".to_string()],
            },
        }],
    };

    let outcome = simple(vec![word], renderer).await;

    assert_eq!(
        outcome.words[0].definitions,
        vec![RenderedDefinition {
            number: 5,
            part_of_speech: PartOfSpeech::Adjective,
            labels: vec!["informal".to_string()],
            antonyms: vec!["cold".to_string()],
            markup: "text".to_string(),
            text: "r:text".to_string(),
        }]
    );
}

#[tokio::test]
async fn the_summary_counts_words_definitions_and_requests() {
    let (renderer, _) = echo_renderer();

    let outcome = simple(
        vec![
            entry("a", &["x", "FAIL y"]),
            entry("b", &["z"]),
            entry("c", &["TITLE w"]),
        ],
        renderer,
    )
    .await;

    assert_eq!(
        outcome.summary,
        PipelineSummary {
            words: 3,
            definitions: 4,
            rendered: 3,
            failed: 1,
            transient: 0,
            requests: 3,
            solo_words: 1,
            title_rerenders: 0,
            title_leak_words: Vec::new(),
        }
    );
}

#[tokio::test]
async fn a_word_with_no_definitions_is_counted_and_not_written() {
    let (renderer, calls) = echo_renderer();

    let outcome = simple(vec![entry("empty", &[])], renderer).await;

    assert_eq!(outcome.summary.words, 1);
    assert_eq!(outcome.summary.definitions, 0);
    assert!(outcome.words.is_empty());
    assert!(recorded(&calls).is_empty());
}

#[tokio::test]
async fn no_words_means_no_requests_and_an_empty_summary() {
    let (renderer, calls) = echo_renderer();

    let outcome = simple(Vec::new(), renderer).await;

    assert_eq!(outcome.summary, PipelineSummary::default());
    assert!(recorded(&calls).is_empty());
}

#[tokio::test]
async fn every_word_is_written_once() {
    let (renderer, _) = echo_renderer();
    let words = (0..7).map(|n| entry(&format!("w{n}"), &["x"])).collect();

    let outcome = run_with(words, renderer, small(2), HashMap::new()).await;

    let names = outcome
        .words
        .iter()
        .map(|w| w.word.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["w0", "w1", "w2", "w3", "w4", "w5", "w6"]);
}

struct SlowRenderer {
    running: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl DefinitionRenderer for SlowRenderer {
    async fn render_definitions(
        &self,
        _page_title: &str,
        definitions: &[String],
    ) -> Result<Vec<DefinitionOutcome>, Error> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        Ok(echo(definitions))
    }
}

#[tokio::test]
async fn requests_in_flight_never_exceed_the_concurrency_limit() {
    let peak = Arc::new(AtomicUsize::new(0));
    let renderer: Arc<dyn DefinitionRenderer> = Arc::new(SlowRenderer {
        running: Arc::new(AtomicUsize::new(0)),
        peak: peak.clone(),
    });
    let config = PipelineConfig {
        concurrency: 3,
        ..small(1)
    };
    let words = (0..10).map(|n| entry(&format!("w{n}"), &["x"])).collect();

    let outcome = run_with(words, renderer, config, HashMap::new()).await;

    assert_eq!(peak.load(Ordering::SeqCst), 3);
    assert_eq!(outcome.summary.rendered, 10);
}

#[tokio::test]
async fn a_concurrency_of_one_sends_one_request_at_a_time() {
    let peak = Arc::new(AtomicUsize::new(0));
    let renderer: Arc<dyn DefinitionRenderer> = Arc::new(SlowRenderer {
        running: Arc::new(AtomicUsize::new(0)),
        peak: peak.clone(),
    });
    let config = PipelineConfig {
        concurrency: 1,
        ..small(1)
    };
    let words = (0..4).map(|n| entry(&format!("w{n}"), &["x"])).collect();

    run_with(words, renderer, config, HashMap::new()).await;

    assert_eq!(peak.load(Ordering::SeqCst), 1);
}

#[test]
fn jsonl_sink_writes_one_json_line_per_word() {
    let mut sink = JsonlSink::new(Vec::new());
    let word = RenderedWord {
        word: "cat".to_string(),
        definitions: vec![RenderedDefinition {
            number: 2,
            part_of_speech: PartOfSpeech::Verb,
            labels: vec!["informal".to_string()],
            antonyms: vec![],
            markup: "a [[small]] animal".to_string(),
            text: "a \"small\" animal".to_string(),
        }],
    };

    sink.write_word(&word).unwrap_or_default();

    let text = String::from_utf8_lossy(&sink.writer).to_string();
    let value: serde_json::Value = serde_json::from_str(text.trim_end()).unwrap_or_default();
    assert!(text.ends_with('\n'));
    assert_eq!(text.matches('\n').count(), 1);
    assert_eq!(value["word"], "cat");
    assert_eq!(value["definitions"][0]["number"], 2);
    assert_eq!(value["definitions"][0]["part_of_speech"], "verb");
    assert_eq!(value["definitions"][0]["labels"][0], "informal");
    assert_eq!(value["definitions"][0]["text"], "a \"small\" animal");
    assert_eq!(value["definitions"][0]["markup"], "a [[small]] animal");
}

#[test]
fn jsonl_sink_writes_each_word_on_its_own_line() {
    let mut sink = JsonlSink::new(Vec::new());
    for name in ["a", "b"] {
        sink.write_word(&RenderedWord {
            word: name.to_string(),
            definitions: Vec::new(),
        })
        .unwrap_or_default();
    }

    assert_eq!(String::from_utf8_lossy(&sink.writer).lines().count(), 2);
}

#[test]
fn jsonl_sink_flush_succeeds_after_writing() {
    let mut sink = JsonlSink::new(Vec::new());
    sink.write_word(&RenderedWord {
        word: "a".to_string(),
        definitions: Vec::new(),
    })
    .unwrap_or_default();

    assert!(sink.flush().is_ok());
}

#[tokio::test]
async fn a_flagged_definition_is_rendered_alone_to_confirm_the_failure() {
    let (renderer, calls) = echo_renderer();

    let outcome = simple(vec![entry("w", &["a", "FAIL b", "c"])], renderer).await;

    assert_eq!(
        recorded(&calls),
        vec![
            ("wm.batch".to_string(), vec!["FAIL b".to_string()]),
            (
                "wm.batch".to_string(),
                vec!["a".to_string(), "FAIL b".to_string(), "c".to_string()]
            ),
        ]
    );
    assert_eq!(outcome.summary.requests, 2);
    assert_eq!(outcome.failures.len(), 1);
}

#[tokio::test]
async fn a_definition_that_renders_alone_is_recovered_from_a_page_level_error() {
    let (renderer, _) = renderer(Box::new(|_, definitions| {
        if definitions.len() > 1 {
            let mut outcomes = echo(definitions);
            let last = outcomes.len() - 1;
            outcomes[last] = DefinitionOutcome::Failed("footer error".to_string());
            Ok(outcomes)
        } else {
            Ok(vec![DefinitionOutcome::Rendered(format!(
                "alone:{}",
                definitions[0]
            ))])
        }
    }));

    let outcome = simple(vec![entry("w", &["a", "b", "c"])], renderer).await;

    assert!(outcome.failures.is_empty());
    assert_eq!(outcome.summary.failed, 0);
    assert_eq!(outcome.summary.requests, 2);
    assert_eq!(
        outcome.words[0]
            .definitions
            .iter()
            .map(|d| d.text.clone())
            .collect::<Vec<_>>(),
        vec!["r:a", "r:b", "alone:c"]
    );
}

#[tokio::test]
async fn a_confirmed_failure_keeps_the_error_from_the_solo_render() {
    let (renderer, _) = renderer(Box::new(|_, definitions| {
        if definitions.len() > 1 {
            Ok(vec![
                DefinitionOutcome::Failed("definitions message".to_string()),
                DefinitionOutcome::Rendered("r:b".to_string()),
            ])
        } else {
            Ok(vec![DefinitionOutcome::Failed("solo message".to_string())])
        }
    }));

    let outcome = simple(vec![entry("w", &["a", "b"])], renderer).await;

    assert_eq!(outcome.failures.len(), 1);
    assert_eq!(outcome.failures[0].failure.error, "solo message");
}

#[tokio::test]
async fn a_failed_confirmation_request_keeps_the_original_failure() {
    let (renderer, _) = renderer(Box::new(|_, definitions| {
        if definitions.len() > 1 {
            Ok(vec![
                DefinitionOutcome::Failed("original".to_string()),
                DefinitionOutcome::Rendered("r:b".to_string()),
            ])
        } else {
            Err(Error::RenderingError("down".to_string()))
        }
    }));

    let outcome = simple(vec![entry("w", &["a", "b"])], renderer).await;

    assert_eq!(outcome.failures[0].failure.error, "original");
    assert_eq!(outcome.failures[0].failure.kind, FailureKind::Error);
}

#[tokio::test]
async fn a_failed_definition_in_a_request_of_one_is_not_confirmed_again() {
    let (renderer, calls) = echo_renderer();

    let outcome = simple(vec![entry("w", &["FAIL only"])], renderer).await;

    assert_eq!(recorded(&calls).len(), 1);
    assert_eq!(outcome.summary.requests, 1);
    assert_eq!(outcome.failures.len(), 1);
}

fn leaking_renderer() -> (Arc<dyn DefinitionRenderer>, Calls) {
    renderer(Box::new(|title, definitions| {
        Ok(definitions
            .iter()
            .map(|markup| {
                if markup.contains("LEAK") && title == "wm.batch" {
                    DefinitionOutcome::Rendered(format!("r:{markup} from wm.batch"))
                } else {
                    DefinitionOutcome::Rendered(format!("r:{markup}"))
                }
            })
            .collect())
    }))
}

#[test]
fn leaks_batch_title_detects_the_shared_title_in_any_case() {
    assert!(leaks_batch_title(Some(&Resolved::Text(
        "x wm.batch y".to_string()
    ))));
    assert!(leaks_batch_title(Some(&Resolved::Text(
        "x WM.Batch y".to_string()
    ))));
}

#[test]
fn leaks_batch_title_ignores_clean_text_and_failures() {
    assert!(!leaks_batch_title(Some(&Resolved::Text(
        "clean".to_string()
    ))));
    assert!(!leaks_batch_title(Some(&Resolved::Failed(
        FailureKind::Error,
        "wm.batch".to_string()
    ))));
    assert!(!leaks_batch_title(None));
}

#[tokio::test]
async fn a_definition_that_leaks_the_batch_title_is_rendered_again_under_its_own_word() {
    let (renderer, calls) = leaking_renderer();

    let outcome = simple(vec![entry("after", &["fine", "LEAK here"])], renderer).await;

    assert_eq!(
        outcome.words[0]
            .definitions
            .iter()
            .map(|d| d.text.clone())
            .collect::<Vec<_>>(),
        vec!["r:fine", "r:LEAK here"]
    );
    assert!(recorded(&calls).contains(&("after".to_string(), vec!["LEAK here".to_string()])));
    assert_eq!(outcome.summary.title_rerenders, 1);
    assert_eq!(outcome.summary.title_leak_words, vec!["after".to_string()]);
    assert_eq!(outcome.summary.requests, 2);
}

#[tokio::test]
async fn clean_definitions_are_not_rendered_again() {
    let (renderer, calls) = leaking_renderer();

    let outcome = simple(vec![entry("w", &["a", "b"])], renderer).await;

    assert_eq!(recorded(&calls).len(), 1);
    assert_eq!(outcome.summary.title_rerenders, 0);
    assert!(outcome.summary.title_leak_words.is_empty());
}

#[tokio::test]
async fn a_word_rendered_alone_is_not_checked_for_the_batch_title() {
    let (renderer, calls) = renderer(Box::new(|_, definitions| {
        Ok(definitions
            .iter()
            .map(|markup| DefinitionOutcome::Rendered(format!("r:{markup} wm.batch")))
            .collect())
    }));

    let outcome = simple(vec![entry("w", &["TITLE a"])], renderer).await;

    assert_eq!(recorded(&calls).len(), 1);
    assert_eq!(outcome.summary.title_rerenders, 0);
}

#[tokio::test]
async fn a_leak_that_cannot_be_rendered_again_is_logged_as_a_transient_failure() {
    let (renderer, _) = renderer(Box::new(|title, definitions| {
        if title == "wm.batch" {
            Ok(vec![
                DefinitionOutcome::Rendered(
                    "leaks wm.batch".to_string()
                );
                definitions.len()
            ])
        } else {
            Err(Error::RenderingError("down".to_string()))
        }
    }));

    let outcome = simple(vec![entry("w", &["a"])], renderer).await;

    assert!(outcome.words.is_empty());
    assert_eq!(outcome.failures.len(), 1);
    assert_eq!(outcome.failures[0].failure.kind, FailureKind::Transient);
}

#[tokio::test]
async fn the_same_word_is_listed_once_when_several_definitions_leak() {
    let (renderer, _) = leaking_renderer();

    let outcome = simple(vec![entry("w", &["LEAK a", "LEAK b"])], renderer).await;

    assert_eq!(outcome.summary.title_rerenders, 2);
    assert_eq!(outcome.summary.title_leak_words, vec!["w".to_string()]);
}

#[test]
fn is_affix_title_is_true_for_a_leading_or_trailing_hyphen() {
    assert!(is_affix_title("-ing"));
    assert!(is_affix_title("pre-"));
    assert!(is_affix_title("-"));
}

#[test]
fn is_affix_title_is_false_for_other_words() {
    assert!(!is_affix_title("ice-cream"));
    assert!(!is_affix_title("word"));
    assert!(!is_affix_title(""));
}

#[tokio::test]
async fn an_affix_word_is_rendered_alone_under_its_own_title() {
    let (renderer, calls) = echo_renderer();

    let outcome = simple(
        vec![
            entry("a", &["x"]),
            entry("-ferous", &["y", "z"]),
            entry("pre-", &["w"]),
        ],
        renderer,
    )
    .await;

    assert_eq!(
        recorded(&calls),
        vec![
            (
                "-ferous".to_string(),
                vec!["y".to_string(), "z".to_string()]
            ),
            ("pre-".to_string(), vec!["w".to_string()]),
            ("wm.batch".to_string(), vec!["x".to_string()]),
        ]
    );
    assert_eq!(outcome.summary.solo_words, 2);
}
