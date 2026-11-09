use super::*;
use crate::{
    failure_log::{FailedDefinition, FailureKind},
    markup_parser::{Chunk, Entry, ListType},
};
use std::collections::HashMap;

struct ScriptedMarkupParser;

impl MarkupParser for ScriptedMarkupParser {
    fn parse(&self, text: &str) -> HashMap<PartOfSpeech, Vec<Entry>> {
        let mut result = HashMap::new();

        for part in text.split(';').filter(|part| !part.is_empty()) {
            let Some((name, definitions)) = part.split_once(':') else {
                continue;
            };
            let part_of_speech = match name {
                "noun" => PartOfSpeech::Noun,
                "verb" => PartOfSpeech::Verb,
                _ => PartOfSpeech::Adjective,
            };
            let entries = definitions
                .split('|')
                .filter(|definition| !definition.is_empty())
                .map(|definition| Entry {
                    kind: ListType::Ordered,
                    labels: Vec::new(),
                    chunks: vec![Chunk::PlainText(definition.to_string())],
                })
                .collect();

            result.insert(part_of_speech, entries);
        }

        result
    }
}

struct EchoEntryParser;

impl EntryParser for EchoEntryParser {
    fn parse(&self, part_of_speech: PartOfSpeech, entries: &[Entry]) -> Vec<Definition> {
        entries
            .iter()
            .map(|entry| Definition {
                part_of_speech,
                labels: Vec::new(),
                markup: entry.chunks.iter().map(ToString::to_string).collect(),
                antonyms: Vec::new(),
            })
            .collect()
    }
}

#[allow(clippy::unnecessary_wraps)]
fn page(word: &str, markup: &str) -> Result<Page, DumpError> {
    Ok(Page {
        word: word.to_string(),
        markup: markup.to_string(),
    })
}

fn entries(pages: Vec<Result<Page, DumpError>>, stride: u64) -> Vec<WordEntry> {
    word_entries(
        pages.into_iter(),
        stride,
        &ScriptedMarkupParser,
        &EchoEntryParser,
    )
    .collect()
}

fn markups(entry: &WordEntry) -> Vec<(usize, String)> {
    entry
        .definitions
        .iter()
        .map(|item| (item.number, item.definition.markup.clone()))
        .collect()
}

fn parse(values: &[&str]) -> Option<Command> {
    Cli::try_parse_from(std::iter::once("whatchamacallit").chain(values.iter().copied()))
        .ok()
        .map(|cli| cli.command)
}

#[test]
fn render_uses_the_defaults() {
    assert_eq!(
        parse(&["render"]),
        Some(Command::Render(RenderArgs {
            out: "render-out".to_string(),
            stride: 1,
            setup_tag: "default".to_string(),
            skip_known: None,
        }))
    );
}

#[test]
fn render_reads_every_option() {
    assert_eq!(
        parse(&[
            "render",
            "--out",
            "here",
            "--stride",
            "10",
            "--setup-tag",
            "luasandbox-1",
            "--skip-known",
            "log.tsv",
        ]),
        Some(Command::Render(RenderArgs {
            out: "here".to_string(),
            stride: 10,
            setup_tag: "luasandbox-1".to_string(),
            skip_known: Some("log.tsv".to_string()),
        }))
    );
}

#[test]
fn render_rejects_an_unknown_option() {
    assert_eq!(parse(&["render", "--bogus"]), None);
}

#[test]
fn render_rejects_an_option_without_a_value() {
    assert_eq!(parse(&["render", "--out"]), None);
    assert_eq!(parse(&["render", "--setup-tag"]), None);
    assert_eq!(parse(&["render", "--skip-known"]), None);
    assert_eq!(parse(&["render", "--stride"]), None);
}

#[test]
fn render_rejects_a_stride_of_zero() {
    assert_eq!(parse(&["render", "--stride", "0"]), None);
}

#[test]
fn render_rejects_a_stride_that_is_not_a_number() {
    assert_eq!(parse(&["render", "--stride", "many"]), None);
    assert_eq!(parse(&["render", "--stride", "-3"]), None);
}

#[test]
fn render_rejects_a_positional_argument() {
    assert_eq!(parse(&["render", "log.tsv"]), None);
}

#[test]
fn retry_takes_the_failure_log_and_the_defaults() {
    assert_eq!(
        parse(&["retry", "log.tsv"]),
        Some(Command::Retry(RetryArgs {
            failed: "log.tsv".to_string(),
            out: "render-out".to_string(),
            setup_tag: "default".to_string(),
        }))
    );
}

#[test]
fn retry_reads_every_option() {
    assert_eq!(
        parse(&["retry", "log.tsv", "--out", "x", "--setup-tag", "new"]),
        Some(Command::Retry(RetryArgs {
            failed: "log.tsv".to_string(),
            out: "x".to_string(),
            setup_tag: "new".to_string(),
        }))
    );
}

#[test]
fn retry_requires_the_failure_log() {
    assert_eq!(parse(&["retry"]), None);
}

#[test]
fn retry_rejects_skip_known() {
    assert_eq!(
        parse(&["retry", "log.tsv", "--skip-known", "other.tsv"]),
        None
    );
}

#[test]
fn retry_rejects_a_second_positional_argument() {
    assert_eq!(parse(&["retry", "a.tsv", "b.tsv"]), None);
}

#[test]
fn generate_title_dependency_defaults_the_output_path() {
    assert_eq!(
        parse(&["generate-title-dependency"]),
        Some(Command::GenerateTitleDependency(GenerateArgs {
            out: DEFAULT_TITLE_DEPENDENCY_FILE.to_string(),
        }))
    );
}

#[test]
fn generate_title_dependency_takes_an_output_path() {
    assert_eq!(
        parse(&["generate-title-dependency", "--out", "list.txt"]),
        Some(Command::GenerateTitleDependency(GenerateArgs {
            out: "list.txt".to_string(),
        }))
    );
}

#[test]
fn generate_title_dependency_rejects_a_positional_argument() {
    assert_eq!(parse(&["generate-title-dependency", "list.txt"]), None);
}

#[test]
fn the_default_title_dependency_file_is_in_the_mediawiki_folder() {
    assert!(DEFAULT_TITLE_DEPENDENCY_FILE.ends_with("/mediawiki/title-dependent.txt"));
}

#[test]
fn stats_defaults_the_prefix() {
    assert_eq!(
        parse(&["stats"]),
        Some(Command::Stats(StatsArgs {
            prefix: "stat-runs/latest".to_string(),
        }))
    );
}

#[test]
fn stats_takes_a_prefix() {
    assert_eq!(
        parse(&["stats", "stat-runs/run-0001"]),
        Some(Command::Stats(StatsArgs {
            prefix: "stat-runs/run-0001".to_string(),
        }))
    );
}

#[test]
fn a_missing_subcommand_is_rejected() {
    assert_eq!(parse(&[]), None);
}

#[test]
fn an_unknown_subcommand_is_rejected() {
    assert_eq!(parse(&["bogus"]), None);
}

#[test]
fn word_entries_numbers_definitions_across_parts_of_speech() {
    let found = entries(vec![page("run", "verb:to move|to flow;noun:an act")], 1);

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].word, "run");
    assert_eq!(
        markups(&found[0]),
        vec![
            (0, "an act".to_string()),
            (1, "to move".to_string()),
            (2, "to flow".to_string()),
        ]
    );
}

#[test]
fn word_entries_orders_parts_of_speech_by_name_every_time() {
    let first = entries(vec![page("w", "verb:v;noun:n;adj:a")], 1);
    let second = entries(vec![page("w", "adj:a;noun:n;verb:v")], 1);

    assert_eq!(markups(&first[0]), markups(&second[0]));
}

#[test]
fn word_entries_keeps_the_part_of_speech_on_each_definition() {
    let found = entries(vec![page("w", "noun:n;verb:v")], 1);

    let parts = found[0]
        .definitions
        .iter()
        .map(|item| item.definition.part_of_speech)
        .collect::<Vec<_>>();
    assert_eq!(parts, vec![PartOfSpeech::Noun, PartOfSpeech::Verb]);
}

#[test]
fn word_entries_skips_pages_without_definitions() {
    let found = entries(
        vec![page("a", ""), page("b", "noun:x"), page("c", "noun:")],
        1,
    );

    assert_eq!(
        found.iter().map(|e| e.word.clone()).collect::<Vec<_>>(),
        vec!["b"]
    );
}

#[test]
fn word_entries_takes_every_nth_page() {
    let pages = (0..7).map(|n| page(&format!("w{n}"), "noun:x")).collect();

    let found = entries(pages, 3);

    assert_eq!(
        found.iter().map(|e| e.word.clone()).collect::<Vec<_>>(),
        vec!["w0", "w3", "w6"]
    );
}

#[test]
fn word_entries_counts_skipped_pages_without_definitions_toward_the_stride() {
    let found = entries(
        vec![page("a", ""), page("b", "noun:x"), page("c", "noun:y")],
        2,
    );

    assert_eq!(
        found.iter().map(|e| e.word.clone()).collect::<Vec<_>>(),
        vec!["c"]
    );
}

#[test]
fn word_entries_treats_a_stride_of_zero_as_one() {
    let found = entries(vec![page("a", "noun:x"), page("b", "noun:y")], 0);

    assert_eq!(found.len(), 2);
}

#[test]
fn word_entries_stops_at_the_first_page_error() {
    let found = entries(
        vec![
            page("a", "noun:x"),
            Err(DumpError::IoError(std::io::Error::other("broken"))),
            page("b", "noun:y"),
        ],
        1,
    );

    assert_eq!(
        found.iter().map(|e| e.word.clone()).collect::<Vec<_>>(),
        vec!["a"]
    );
}

#[test]
fn word_entries_of_no_pages_is_empty() {
    assert!(entries(Vec::new(), 1).is_empty());
}

fn logged(word: &str, number: usize, markup: &str) -> LoggedFailure {
    LoggedFailure {
        failure: FailedDefinition {
            word: word.to_string(),
            definition_number: number,
            kind: FailureKind::Error,
            error: "e".to_string(),
            markup: markup.to_string(),
        },
        hash: String::new(),
        setup_tag: "t".to_string(),
    }
}

#[test]
fn retry_entries_groups_failures_by_word() {
    let found = retry_entries(vec![
        logged("b", 1, "b1"),
        logged("a", 0, "a0"),
        logged("b", 4, "b4"),
    ]);

    assert_eq!(
        found.iter().map(|e| e.word.clone()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(
        markups(&found[1]),
        vec![(1, "b1".to_string()), (4, "b4".to_string())]
    );
}

#[test]
fn retry_entries_keeps_the_original_definition_numbers() {
    let found = retry_entries(vec![logged("w", 7, "m")]);

    assert_eq!(markups(&found[0]), vec![(7, "m".to_string())]);
}

#[test]
fn retry_entries_sorts_each_words_definitions_by_number() {
    let found = retry_entries(vec![logged("w", 9, "x"), logged("w", 2, "y")]);

    assert_eq!(
        markups(&found[0]),
        vec![(2, "y".to_string()), (9, "x".to_string())]
    );
}

#[test]
fn retry_entries_uses_an_unknown_part_of_speech() {
    let found = retry_entries(vec![logged("w", 0, "m")]);

    assert_eq!(
        found[0].definitions[0].definition.part_of_speech,
        PartOfSpeech::Unknown
    );
}

#[test]
fn retry_entries_of_nothing_is_empty() {
    assert!(retry_entries(Vec::new()).is_empty());
}
