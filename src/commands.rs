use crate::{
    dump_reader::{Error as DumpError, Page},
    entry_parser::{Definition, EntryParser},
    failure_log::LoggedFailure,
    markup_parser::{MarkupParser, PartOfSpeech},
    pipeline::{NumberedDefinition, WordEntry},
};
use clap::{Args, Parser, Subcommand};
use std::collections::BTreeMap;

#[derive(Debug, Parser)]
#[command(name = "whatchamacallit", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum Command {
    Render(RenderArgs),
    Retry(RetryArgs),
    Stats(StatsArgs),
}

#[derive(Debug, PartialEq, Eq, Args)]
pub struct RenderArgs {
    #[arg(long, default_value = "render-out")]
    pub out: String,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
    pub stride: u64,
    #[arg(long, default_value = "default")]
    pub setup_tag: String,
    #[arg(long, value_name = "FAILED.tsv")]
    pub skip_known: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Args)]
pub struct RetryArgs {
    #[arg(value_name = "FAILED.tsv")]
    pub failed: String,
    #[arg(long, default_value = "render-out")]
    pub out: String,
    #[arg(long, default_value = "default")]
    pub setup_tag: String,
}

#[derive(Debug, PartialEq, Eq, Args)]
pub struct StatsArgs {
    #[arg(default_value = "stat-runs/latest")]
    pub prefix: String,
}

fn build_entry(
    page: Page,
    markup_parser: &dyn MarkupParser,
    entry_parser: &dyn EntryParser,
) -> Option<WordEntry> {
    let mut parts = markup_parser
        .parse(&page.markup)
        .into_iter()
        .collect::<Vec<_>>();
    parts.sort_by_key(|(part_of_speech, _)| part_of_speech.to_string());

    let definitions = parts
        .into_iter()
        .flat_map(|(part_of_speech, entries)| entry_parser.parse(part_of_speech, &entries))
        .enumerate()
        .map(|(number, definition)| NumberedDefinition { number, definition })
        .collect::<Vec<_>>();

    if definitions.is_empty() {
        None
    } else {
        Some(WordEntry {
            word: page.word,
            definitions,
        })
    }
}

pub fn word_entries<'a>(
    pages: impl Iterator<Item = Result<Page, DumpError>> + 'a,
    stride: u64,
    markup_parser: &'a dyn MarkupParser,
    entry_parser: &'a dyn EntryParser,
) -> impl Iterator<Item = WordEntry> + 'a {
    let stride = stride.max(1);

    pages
        .map_while(|page| match page {
            Ok(page) => Some(page),
            Err(err) => {
                eprintln!("Page error: {err}");
                None
            }
        })
        .enumerate()
        .filter(move |(index, _)| (*index as u64).is_multiple_of(stride))
        .filter_map(move |(_, page)| build_entry(page, markup_parser, entry_parser))
}

pub fn retry_entries(selected: Vec<LoggedFailure>) -> Vec<WordEntry> {
    let mut grouped: BTreeMap<String, Vec<NumberedDefinition>> = BTreeMap::new();

    for entry in selected {
        grouped
            .entry(entry.failure.word)
            .or_default()
            .push(NumberedDefinition {
                number: entry.failure.definition_number,
                definition: Definition {
                    part_of_speech: PartOfSpeech::Unknown,
                    labels: Vec::new(),
                    markup: entry.failure.markup,
                    antonyms: Vec::new(),
                },
            });
    }

    grouped
        .into_iter()
        .map(|(word, mut definitions)| {
            definitions.sort_by_key(|item| item.number);
            WordEntry { word, definitions }
        })
        .collect()
}

#[cfg(test)]
mod tests;
