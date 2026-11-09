use crate::{
    entry_parser::Definition,
    failure_log::{FailedDefinition, FailureKind, FailureLog, markup_hash},
    markup_parser::PartOfSpeech,
    markup_renderer::{
        DefinitionOutcome, DefinitionRenderer, has_unterminated_template, prepare_markup,
    },
    title_dependency::TitleDependency,
};
use serde_json::json;
use std::{
    collections::HashMap,
    io::{self, Write},
    sync::Arc,
};
use tokio::task::JoinSet;

pub const BATCH_TITLE: &str = "wm.batch";
const UNTERMINATED_TEMPLATE: &str = "unterminated template: an opening braces pair is never closed";
const RANGE_ATTEMPTS: u32 = 2;
const SINGLE_DEFINITION_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineConfig {
    pub concurrency: usize,
    pub batch_size: usize,
    pub batch_max_bytes: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            concurrency: 20,
            batch_size: 200,
            batch_max_bytes: 120_000,
        }
    }
}

pub struct NumberedDefinition {
    pub number: usize,
    pub definition: Definition,
}

pub struct WordEntry {
    pub word: String,
    pub definitions: Vec<NumberedDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDefinition {
    pub number: usize,
    pub part_of_speech: PartOfSpeech,
    pub labels: Vec<String>,
    pub antonyms: Vec<String>,
    pub markup: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedWord {
    pub word: String,
    pub definitions: Vec<RenderedDefinition>,
}

pub trait RenderedSink {
    fn write_word(&mut self, word: &RenderedWord) -> io::Result<()>;
}

pub struct JsonlSink<W: Write> {
    writer: W,
}

impl<W: Write> JsonlSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<W: Write> RenderedSink for JsonlSink<W> {
    fn write_word(&mut self, word: &RenderedWord) -> io::Result<()> {
        let definitions = word
            .definitions
            .iter()
            .map(|definition| {
                json!({
                    "number": definition.number,
                    "part_of_speech": definition.part_of_speech.to_string(),
                    "labels": definition.labels,
                    "antonyms": definition.antonyms,
                    "markup": definition.markup,
                    "text": definition.text,
                })
            })
            .collect::<Vec<_>>();

        writeln!(
            self.writer,
            "{}",
            json!({"word": word.word, "definitions": definitions})
        )
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PipelineSummary {
    pub words: usize,
    pub definitions: usize,
    pub rendered: usize,
    pub failed: usize,
    pub transient: usize,
    pub requests: u64,
    pub solo_words: usize,
    pub title_rerenders: usize,
    pub title_leak_words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Resolved {
    Text(String),
    Failed(FailureKind, String),
}

struct WordWork {
    word: String,
    definitions: Vec<NumberedDefinition>,
    resolved: Vec<Option<Resolved>>,
}

struct Completed {
    title: String,
    words: Vec<WordWork>,
    owners: Vec<(usize, usize)>,
    outcomes: Vec<Resolved>,
    requests: u64,
}

fn is_affix_title(word: &str) -> bool {
    word.starts_with('-') || word.ends_with('-')
}

fn batch_is_full(config: &PipelineConfig, word_count: usize, markup_bytes: usize) -> bool {
    word_count >= config.batch_size || markup_bytes >= config.batch_max_bytes
}

fn prepare_work(entry: WordEntry, known: &HashMap<String, String>) -> WordWork {
    let resolved = entry
        .definitions
        .iter()
        .map(|item| {
            let markup = &item.definition.markup;

            if let Some(error) = known.get(&markup_hash(markup)) {
                return Some(Resolved::Failed(FailureKind::Error, error.clone()));
            }
            if has_unterminated_template(&prepare_markup(markup)) {
                return Some(Resolved::Failed(
                    FailureKind::Error,
                    UNTERMINATED_TEMPLATE.to_string(),
                ));
            }

            None
        })
        .collect();

    WordWork {
        word: entry.word,
        definitions: entry.definitions,
        resolved,
    }
}

fn unresolved_markups(work: &WordWork) -> Vec<&str> {
    work.definitions
        .iter()
        .zip(&work.resolved)
        .filter(|(_, resolved)| resolved.is_none())
        .map(|(item, _)| item.definition.markup.as_str())
        .collect()
}

fn collect_definitions(words: &[WordWork]) -> (Vec<String>, Vec<(usize, usize)>) {
    let mut definitions = Vec::new();
    let mut owners = Vec::new();

    for (word_index, work) in words.iter().enumerate() {
        for (definition_index, item) in work.definitions.iter().enumerate() {
            if work.resolved[definition_index].is_none() {
                definitions.push(item.definition.markup.clone());
                owners.push((word_index, definition_index));
            }
        }
    }

    (definitions, owners)
}

async fn render_resilient(
    renderer: &dyn DefinitionRenderer,
    title: &str,
    definitions: &[String],
) -> (Vec<Resolved>, u64) {
    let mut outcomes: Vec<Option<Resolved>> = vec![None; definitions.len()];
    let mut requests = 0u64;
    let mut stack = vec![(0usize, definitions.len(), 1u32)];
    let mut to_confirm: Vec<usize> = Vec::new();

    while let Some((start, end, attempt)) = stack.pop() {
        if start >= end {
            continue;
        }

        requests += 1;
        let result = renderer
            .render_definitions(title, &definitions[start..end])
            .await;

        match result {
            Ok(found) if found.len() == end - start => {
                for (offset, outcome) in found.into_iter().enumerate() {
                    outcomes[start + offset] = Some(match outcome {
                        DefinitionOutcome::Rendered(text) => Resolved::Text(text),
                        DefinitionOutcome::Failed(message) => {
                            if end - start > 1 {
                                to_confirm.push(start + offset);
                            }
                            Resolved::Failed(FailureKind::Error, message)
                        }
                    });
                }
            }
            other => {
                let message = match other {
                    Err(err) => err.to_string(),
                    Ok(found) => format!(
                        "expected {expected} outcomes, found {found}",
                        expected = end - start,
                        found = found.len()
                    ),
                };
                let limit = if end - start == 1 {
                    SINGLE_DEFINITION_ATTEMPTS
                } else {
                    RANGE_ATTEMPTS
                };

                if attempt < limit {
                    stack.push((start, end, attempt + 1));
                } else if end - start > 1 {
                    let middle = start + (end - start) / 2;
                    stack.push((middle, end, 1));
                    stack.push((start, middle, 1));
                } else {
                    outcomes[start] = Some(Resolved::Failed(FailureKind::Transient, message));
                }
            }
        }
    }

    for index in to_confirm {
        requests += 1;
        let alone = renderer
            .render_definitions(title, &definitions[index..=index])
            .await;

        if let Ok(found) = alone {
            match found.into_iter().next() {
                Some(DefinitionOutcome::Rendered(text)) => {
                    outcomes[index] = Some(Resolved::Text(text));
                }
                Some(DefinitionOutcome::Failed(message)) => {
                    outcomes[index] = Some(Resolved::Failed(FailureKind::Error, message));
                }
                _ => {}
            }
        }
    }

    let resolved = outcomes
        .into_iter()
        .map(|outcome| {
            outcome.unwrap_or_else(|| {
                Resolved::Failed(FailureKind::Transient, "not rendered".to_string())
            })
        })
        .collect();

    (resolved, requests)
}

fn finish_word(
    work: WordWork,
    sink: &mut dyn RenderedSink,
    failures: &mut FailureLog,
    summary: &mut PipelineSummary,
) -> io::Result<()> {
    let WordWork {
        word,
        definitions,
        resolved,
    } = work;
    let mut rendered = Vec::new();

    for (item, outcome) in definitions.into_iter().zip(resolved) {
        let (kind, error) = match outcome {
            Some(Resolved::Text(text)) => {
                summary.rendered += 1;
                rendered.push(RenderedDefinition {
                    number: item.number,
                    part_of_speech: item.definition.part_of_speech,
                    labels: item.definition.labels,
                    antonyms: item.definition.antonyms,
                    markup: item.definition.markup,
                    text,
                });
                continue;
            }
            Some(Resolved::Failed(kind, error)) => (kind, error),
            None => (FailureKind::Transient, "not rendered".to_string()),
        };

        match kind {
            FailureKind::Error => summary.failed += 1,
            FailureKind::Transient => summary.transient += 1,
        }
        failures.record(FailedDefinition {
            word: word.clone(),
            definition_number: item.number,
            kind,
            error,
            markup: item.definition.markup,
        });
    }

    if rendered.is_empty() {
        Ok(())
    } else {
        sink.write_word(&RenderedWord {
            word,
            definitions: rendered,
        })
    }
}

fn dispatch(
    in_flight: &mut JoinSet<Completed>,
    renderer: &Arc<dyn DefinitionRenderer>,
    title: String,
    words: Vec<WordWork>,
) {
    let (definitions, owners) = collect_definitions(&words);
    let renderer = Arc::clone(renderer);

    in_flight.spawn(async move {
        let (outcomes, requests) = render_resilient(renderer.as_ref(), &title, &definitions).await;
        Completed {
            title,
            words,
            owners,
            outcomes,
            requests,
        }
    });
}

fn leaks_batch_title(resolved: Option<&Resolved>) -> bool {
    matches!(resolved, Some(Resolved::Text(text)) if text.to_ascii_lowercase().contains(BATCH_TITLE))
}

async fn rerender_leaks(
    renderer: &Arc<dyn DefinitionRenderer>,
    words: &mut [WordWork],
    summary: &mut PipelineSummary,
) {
    for work in words.iter_mut() {
        for definition_index in 0..work.definitions.len() {
            if !leaks_batch_title(work.resolved[definition_index].as_ref()) {
                continue;
            }

            let markup = work.definitions[definition_index].definition.markup.clone();
            summary.requests += 1;
            summary.title_rerenders += 1;
            if !summary.title_leak_words.contains(&work.word) {
                summary.title_leak_words.push(work.word.clone());
            }

            let redone = match renderer.render_definitions(&work.word, &[markup]).await {
                Ok(found) => match found.into_iter().next() {
                    Some(DefinitionOutcome::Rendered(text)) => Resolved::Text(text),
                    Some(DefinitionOutcome::Failed(message)) => {
                        Resolved::Failed(FailureKind::Error, message)
                    }
                    None => Resolved::Failed(FailureKind::Transient, "not rendered".to_string()),
                },
                Err(err) => Resolved::Failed(FailureKind::Transient, err.to_string()),
            };
            work.resolved[definition_index] = Some(redone);
        }
    }
}

async fn drain_to(
    in_flight: &mut JoinSet<Completed>,
    limit: usize,
    renderer: &Arc<dyn DefinitionRenderer>,
    sink: &mut dyn RenderedSink,
    failures: &mut FailureLog,
    summary: &mut PipelineSummary,
) -> io::Result<()> {
    while in_flight.len() > limit {
        let Some(joined) = in_flight.join_next().await else {
            break;
        };
        let Completed {
            title,
            mut words,
            owners,
            outcomes,
            requests,
        } = joined.map_err(io::Error::other)?;

        summary.requests += requests;
        for ((word_index, definition_index), outcome) in owners.into_iter().zip(outcomes) {
            words[word_index].resolved[definition_index] = Some(outcome);
        }
        if title == BATCH_TITLE {
            rerender_leaks(renderer, &mut words, summary).await;
        }
        for work in words {
            finish_word(work, sink, failures, summary)?;
        }
    }

    Ok(())
}

pub async fn run(
    words: impl Iterator<Item = WordEntry>,
    renderer: Arc<dyn DefinitionRenderer>,
    title_dependency: &dyn TitleDependency,
    config: PipelineConfig,
    known: &HashMap<String, String>,
    sink: &mut dyn RenderedSink,
    failures: &mut FailureLog,
) -> io::Result<PipelineSummary> {
    let mut summary = PipelineSummary::default();
    let mut in_flight: JoinSet<Completed> = JoinSet::new();
    let mut pending: Vec<WordWork> = Vec::new();
    let mut pending_bytes = 0usize;
    let room = config.concurrency.saturating_sub(1);

    for entry in words {
        summary.words += 1;
        summary.definitions += entry.definitions.len();

        let work = prepare_work(entry, known);
        let markups = unresolved_markups(&work);

        if markups.is_empty() {
            finish_word(work, sink, failures, &mut summary)?;
            continue;
        }

        if is_affix_title(&work.word) || title_dependency.requires_title(&markups.join("\n")) {
            summary.solo_words += 1;
            let title = work.word.clone();
            drain_to(
                &mut in_flight,
                room,
                &renderer,
                sink,
                failures,
                &mut summary,
            )
            .await?;
            dispatch(&mut in_flight, &renderer, title, vec![work]);
            continue;
        }

        pending_bytes += markups.iter().map(|markup| markup.len()).sum::<usize>();
        pending.push(work);

        if batch_is_full(&config, pending.len(), pending_bytes) {
            pending_bytes = 0;
            drain_to(
                &mut in_flight,
                room,
                &renderer,
                sink,
                failures,
                &mut summary,
            )
            .await?;
            dispatch(
                &mut in_flight,
                &renderer,
                BATCH_TITLE.to_string(),
                std::mem::take(&mut pending),
            );
        }
    }

    if !pending.is_empty() {
        drain_to(
            &mut in_flight,
            room,
            &renderer,
            sink,
            failures,
            &mut summary,
        )
        .await?;
        dispatch(
            &mut in_flight,
            &renderer,
            BATCH_TITLE.to_string(),
            std::mem::take(&mut pending),
        );
    }

    drain_to(&mut in_flight, 0, &renderer, sink, failures, &mut summary).await?;

    Ok(summary)
}

#[cfg(test)]
mod tests;
