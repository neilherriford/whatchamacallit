mod commands;
mod database;
mod dump_reader;
mod entry_parser;
mod failure_log;
mod html_entity_renderer;
mod markup_parser;
mod markup_renderer;
mod model;
mod pipeline;
mod stats;
mod title_dependency;
mod utils;

use crate::{
    commands::{Cli, Command, RenderArgs, RetryArgs},
    dump_reader::{DumpReader, LocalDumpFile},
    entry_parser::{EntryParser, SimpleEntryParser},
    failure_log::{FailureLog, known_failures, read_tsv, split_for_retry},
    html_entity_renderer::SimpleHtmlEntityRenderer,
    markup_parser::{MarkupParser, SimpleMarkupParser},
    markup_renderer::{
        DefinitionRenderer, LocalMediaWikiClient, MarkupRenderer, SimpleMarkupRenderer,
    },
    pipeline::{JsonlSink, PipelineConfig, PipelineSummary, WordEntry},
    title_dependency::SimpleTitleDependency,
};
use clap::Parser;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    process::ExitCode,
    sync::Arc,
};
use tokio;

#[tokio::main]
async fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Render(args) => render_command(args).await,
        Command::Retry(args) => retry_command(args).await,
        Command::Stats(args) => generate_samples(args.prefix).await,
    }
}

fn open_pages() -> Result<DumpReader, String> {
    let dump_file = LocalDumpFile::create_with_default_path().map_err(|err| err.to_string())?;

    DumpReader::new(Box::new(dump_file)).map_err(|err| err.to_string())
}

fn load_title_dependency() -> Result<SimpleTitleDependency, String> {
    let path = [
        env!("CARGO_MANIFEST_DIR"),
        "mediawiki",
        "title-dependent.txt",
    ]
    .iter()
    .collect::<std::path::PathBuf>();

    std::fs::read_to_string(&path)
        .map(|text| SimpleTitleDependency::from_lines(&text))
        .map_err(|err| format!("{}: {err}", path.display()))
}

fn write_failure_files(log: &FailureLog, out_dir: &Path, setup_tag: &str) -> std::io::Result<()> {
    let mut tsv = BufWriter::new(File::create(out_dir.join("failed-definitions.tsv"))?);
    log.write_tsv(&mut tsv, setup_tag)?;
    tsv.flush()?;

    let mut markdown = BufWriter::new(File::create(out_dir.join("failed-definitions.markdown"))?);
    log.write_markdown(&mut markdown, setup_tag)?;
    markdown.flush()
}

fn print_summary(summary: &PipelineSummary, failures: &FailureLog, out_dir: &Path) {
    println!("words: {}", summary.words);
    println!("definitions: {}", summary.definitions);
    println!("rendered: {}", summary.rendered);
    println!("failed: {}", summary.failed);
    println!("transient failures: {}", summary.transient);
    println!("requests: {}", summary.requests);
    println!("words rendered alone: {}", summary.solo_words);
    println!(
        "definitions rendered again under their own title: {}",
        summary.title_rerenders
    );
    for word in summary.title_leak_words.iter().take(20) {
        println!("  title leaked into: {word}");
    }
    if failures.is_empty() {
        println!("failure log: empty");
    } else {
        println!(
            "failure log: {} entries in {}",
            failures.len(),
            out_dir.join("failed-definitions.markdown").display()
        );
    }
}

async fn run_pipeline(
    words: impl Iterator<Item = WordEntry>,
    known: &HashMap<String, String>,
    out_dir: &Path,
    rendered_name: &str,
    kept: Vec<failure_log::FailedDefinition>,
    setup_tag: &str,
) -> Result<(), String> {
    let title_dependency = load_title_dependency()?;
    std::fs::create_dir_all(out_dir).map_err(|err| format!("{}: {err}", out_dir.display()))?;
    let rendered_path = out_dir.join(rendered_name);
    let file = File::create(&rendered_path)
        .map_err(|err| format!("{}: {err}", rendered_path.display()))?;
    let mut sink = JsonlSink::new(BufWriter::new(file));
    let mut failures = FailureLog::default();
    for failure in kept {
        failures.record(failure);
    }

    let renderer: Arc<dyn DefinitionRenderer> = Arc::new(SimpleMarkupRenderer::new(
        Box::new(SimpleHtmlEntityRenderer::default()),
        Box::new(LocalMediaWikiClient::default()),
    ));
    let summary = pipeline::run(
        words,
        renderer,
        &title_dependency,
        PipelineConfig::default(),
        known,
        &mut sink,
        &mut failures,
    )
    .await
    .map_err(|err| err.to_string())?;

    sink.flush().map_err(|err| err.to_string())?;
    write_failure_files(&failures, out_dir, setup_tag).map_err(|err| err.to_string())?;
    print_summary(&summary, &failures, out_dir);

    Ok(())
}

async fn render_command(args: RenderArgs) -> ExitCode {
    let known = match &args.skip_known {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => known_failures(&read_tsv(&text), &args.setup_tag),
            Err(err) => {
                println!("oops {path}: {err}");
                return ExitCode::FAILURE;
            }
        },
        None => HashMap::new(),
    };

    let pages = match open_pages() {
        Ok(pages) => pages,
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };
    let markup_parser = SimpleMarkupParser::default();
    let entry_parser = SimpleEntryParser::default();
    let words = commands::word_entries(pages, args.stride, &markup_parser, &entry_parser);

    match run_pipeline(
        words,
        &known,
        Path::new(&args.out),
        "rendered.jsonl",
        Vec::new(),
        &args.setup_tag,
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            println!("oops {err}");
            ExitCode::FAILURE
        }
    }
}

async fn retry_command(args: RetryArgs) -> ExitCode {
    let log_path = &args.failed;
    let entries = match std::fs::read_to_string(log_path) {
        Ok(text) => read_tsv(&text),
        Err(err) => {
            println!("oops {log_path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let (to_retry, kept) = split_for_retry(entries, &args.setup_tag);

    if to_retry.is_empty() {
        println!("nothing to retry for setup tag {}", args.setup_tag);
        return ExitCode::SUCCESS;
    }

    let kept = kept.into_iter().map(|entry| entry.failure).collect();
    let words = commands::retry_entries(to_retry).into_iter();

    match run_pipeline(
        words,
        &HashMap::new(),
        Path::new(&args.out),
        "retried.jsonl",
        kept,
        &args.setup_tag,
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            println!("oops {err}");
            ExitCode::FAILURE
        }
    }
}

async fn generate_samples(prefix: String) -> ExitCode {
    let dump_file = match LocalDumpFile::create_with_default_path() {
        Ok(dump_file) => Box::new(dump_file),
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut pages = match DumpReader::new(dump_file) {
        Ok(dump_reader) => Box::new(dump_reader),
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };

    let html_entity_renderer = SimpleHtmlEntityRenderer::default();
    let markup_renderer = SimpleMarkupRenderer::new(
        Box::new(html_entity_renderer),
        Box::new(LocalMediaWikiClient::default()),
    );
    let markup_parser = SimpleMarkupParser::default();
    let entry_parser = SimpleEntryParser::default();

    let mut templates_out = match File::create(format!("{prefix}-templates.csv")) {
        Ok(file) => BufWriter::new(file),
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut meta_out = match File::create(format!("{prefix}-meta.csv")) {
        Ok(file) => BufWriter::new(file),
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };

    let result = stats::run(
        &mut pages,
        Box::new(markup_parser),
        Box::new(entry_parser),
        Box::new(markup_renderer),
        &mut templates_out,
        &mut meta_out,
    )
    .await;

    if let Err(err) = result.and_then(|()| templates_out.flush()).and_then(|()| meta_out.flush()) {
        println!("oops {err}");
        return ExitCode::FAILURE;
    }
    return ExitCode::SUCCESS;
}

async fn rendering_test() -> ExitCode {
    let dump_file = match LocalDumpFile::create_with_default_path() {
        Ok(dump_file) => Box::new(dump_file),
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };

    let dump_reader = match DumpReader::new(dump_file) {
        Ok(dump_reader) => Box::new(dump_reader),
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };

    let html_entity_renderer = SimpleHtmlEntityRenderer::default();
    let markup_renderer = SimpleMarkupRenderer::new(
        Box::new(html_entity_renderer),
        Box::new(LocalMediaWikiClient::default()),
    );
    let markup_parser = SimpleMarkupParser::default();
    let entry_parser = SimpleEntryParser::default();

    let mut count = 0;

    for page in dump_reader {
        let page = match page {
            Ok(page) => page,
            Err(err) => {
                println!("oops {err}");
                return ExitCode::FAILURE;
            }
        };

        println!("{}", page.word);
        for _idx in 0..page.word.len() {
            print!("=");
        }
        print!("\n");

        for (part_of_speech, entries) in markup_parser.parse(&page.markup) {
            for definition in entry_parser.parse(part_of_speech, &entries) {
                println!("part of speech: {}", part_of_speech);
                print!("labels:         ");
                if definition.labels.is_empty() {
                    println!("none")
                } else {
                    println!("{}", definition.labels.join(", "));
                }
                print!("antonyms:       ");
                if definition.antonyms.is_empty() {
                    println!("none")
                } else {
                    println!("{}", definition.antonyms.join(", "));
                }

                match markup_renderer.render(&page.word, &definition.markup).await {
                    Ok(rendered) => println!("{rendered}\n- - - - -\n\n"),
                    Err(err) => eprintln!("oops: {err}"),
                }
            }

            print!("\n");
        }

        if count >= 2 {
            break;
        }

        count += 1;
    }

    return ExitCode::SUCCESS;
}
