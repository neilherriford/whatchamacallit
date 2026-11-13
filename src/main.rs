mod commands;
mod database;
mod dump_reader;
mod dump_splitter;
mod entry_parser;
mod failure_log;
mod html_entity_renderer;
mod markup_parser;
mod markup_renderer;
mod model;
mod pipeline;
mod render_timing;
mod stats;
mod template_examples;
mod title_analysis;
mod title_dependency;
mod title_differential;
mod utils;

use crate::{
    commands::{Cli, Command, GenerateArgs, RenderArgs, RetryArgs, SplitArgs},
    dump_reader::{DumpReader, LocalDumpFile},
    dump_splitter::{split_dump, totals, write_manifest, write_ranges},
    entry_parser::{EntryParser, SimpleEntryParser},
    failure_log::{FailureLog, known_failures, read_tsv, split_for_retry},
    html_entity_renderer::SimpleHtmlEntityRenderer,
    markup_parser::{MarkupParser, SimpleMarkupParser},
    markup_renderer::{
        DefinitionRenderer, LocalMediaWikiClient, MarkupRenderer, SimpleMarkupRenderer,
    },
    pipeline::{JsonlSink, PipelineConfig, PipelineSummary, WordEntry},
    render_timing::{RequestTiming, RunFacts, TimedRenderer, write_meta, write_requests},
    template_examples::TemplateExamples,
    title_analysis::TitleAnalysis,
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
        Command::GenerateTitleDependency(args) => generate_title_dependency(args).await,
        Command::Stats(args) => generate_samples(args.prefix).await,
        Command::SplitDump(args) => split_dump_command(&args),
    }
}

const TITLE_COMPARISON_CONCURRENCY: usize = 10;

fn open_pages() -> Result<DumpReader, String> {
    open_pages_in(&[0])
}

fn open_pages_in(namespaces: &[i32]) -> Result<DumpReader, String> {
    let dump_file = LocalDumpFile::create_with_default_path().map_err(|err| err.to_string())?;

    DumpReader::with_namespaces(Box::new(dump_file), namespaces).map_err(|err| err.to_string())
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
        .map_err(|err| {
            format!(
                "{}: {err} (run `whatchamacallit generate-title-dependency` to create it)",
                path.display()
            )
        })
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

fn write_run_stats(
    out_dir: &Path,
    rendered_name: &str,
    facts: &RunFacts,
    summary: &PipelineSummary,
    timings: &[RequestTiming],
) -> std::io::Result<()> {
    let stem = Path::new(rendered_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(rendered_name);

    let mut meta = BufWriter::new(File::create(out_dir.join(format!("{stem}-meta.csv")))?);
    write_meta(facts, summary, timings, &mut meta)?;
    meta.flush()?;

    let mut requests = BufWriter::new(File::create(out_dir.join(format!("{stem}-requests.csv")))?);
    write_requests(timings, &mut requests)?;
    requests.flush()
}

async fn run_pipeline(
    words: impl Iterator<Item = WordEntry>,
    known: &HashMap<String, String>,
    out_dir: &Path,
    rendered_name: &str,
    kept: Vec<failure_log::FailedDefinition>,
    setup_tag: &str,
    stride: u64,
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

    let timed = Arc::new(TimedRenderer::new(Arc::new(SimpleMarkupRenderer::new(
        Box::new(SimpleHtmlEntityRenderer::default()),
        Box::new(LocalMediaWikiClient::default()),
    ))));
    let renderer: Arc<dyn DefinitionRenderer> = timed.clone();
    let config = PipelineConfig::default();
    let started_at_ms = stats::now_millis();
    let started = std::time::Instant::now();
    let summary = pipeline::run(
        words,
        renderer,
        &title_dependency,
        config,
        known,
        &mut sink,
        &mut failures,
    )
    .await
    .map_err(|err| err.to_string())?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let finished_at_ms = stats::now_millis();

    sink.flush().map_err(|err| err.to_string())?;
    write_failure_files(&failures, out_dir, setup_tag).map_err(|err| err.to_string())?;
    write_run_stats(
        out_dir,
        rendered_name,
        &RunFacts {
            started_at_ms,
            finished_at_ms,
            elapsed_ms,
            concurrency: config.concurrency,
            batch_size: config.batch_size,
            batch_max_bytes: config.batch_max_bytes,
            stride,
            setup_tag,
            git_commit: &stats::git_commit(),
        },
        &summary,
        &timed.timings(),
    )
    .map_err(|err| err.to_string())?;
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
        args.stride,
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
        1,
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

fn split_dump_command(args: &SplitArgs) -> ExitCode {
    match run_split(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            println!("oops {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_split(args: &SplitArgs) -> Result<(), String> {
    let input_path = args
        .input
        .as_ref()
        .map_or_else(LocalDumpFile::default_path, std::path::PathBuf::from);
    let out_dir = Path::new(&args.out);
    let file = File::open(&input_path).map_err(|err| format!("{}: {err}", input_path.display()))?;
    let total_bytes = file
        .metadata()
        .map_err(|err| format!("{}: {err}", input_path.display()))?
        .len();
    std::fs::create_dir_all(out_dir).map_err(|err| format!("{}: {err}", out_dir.display()))?;

    let started = std::time::Instant::now();
    let ranges_only = args.ranges_only;
    let mut open = |index: usize| -> std::io::Result<Box<dyn Write>> {
        if ranges_only {
            return Ok(Box::new(std::io::sink()));
        }
        let part = File::create(out_dir.join(format!("part-{index:04}.xml")))?;
        Ok(Box::new(BufWriter::new(part)))
    };
    let stats = split_dump(
        std::io::BufReader::with_capacity(1 << 20, file),
        total_bytes,
        args.parts,
        args.keep_every,
        &mut open,
    )
    .map_err(|err| err.to_string())?;

    let manifest_path = out_dir.join("manifest.tsv");
    let mut manifest = BufWriter::new(
        File::create(&manifest_path).map_err(|err| format!("{}: {err}", manifest_path.display()))?,
    );
    write_manifest(&stats, &mut manifest)
        .and_then(|()| manifest.flush())
        .map_err(|err| format!("{}: {err}", manifest_path.display()))?;

    let ranges_path = out_dir.join("ranges.tsv");
    let mut ranges = BufWriter::new(
        File::create(&ranges_path).map_err(|err| format!("{}: {err}", ranges_path.display()))?,
    );
    write_ranges(&stats, &mut ranges)
        .and_then(|()| ranges.flush())
        .map_err(|err| format!("{}: {err}", ranges_path.display()))?;

    println!(
        "parts: {}\nparts written: {}\nseconds: {:.1}",
        stats.len(),
        stats.iter().filter(|part| part.kept).count(),
        started.elapsed().as_secs_f64()
    );
    for (namespace, (pages, text_bytes)) in totals(&stats) {
        println!("namespace {namespace}: {pages} pages, {text_bytes} text bytes");
    }
    println!("manifest: {}\nranges: {}", manifest_path.display(), ranges_path.display());

    Ok(())
}

async fn generate_title_dependency(args: GenerateArgs) -> ExitCode {
    match build_title_dependency(&args.out).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            println!("oops {err}");
            ExitCode::FAILURE
        }
    }
}

async fn build_title_dependency(out: &str) -> Result<(), String> {
    let analysis = TitleAnalysis::new().map_err(|err| err.to_string())?;

    println!("reading templates and modules from the dump");
    let static_dependents = analysis.static_dependents(open_pages_in(&[10, 828])?);
    println!(
        "templates analysed: {}\nmodules analysed: {}\nmodules reading the page title: {}\ntitle-dependent templates: {}",
        static_dependents.templates_analysed,
        static_dependents.modules_analysed,
        static_dependents.modules.len(),
        static_dependents.templates.len()
    );

    println!("collecting example definitions from the dump");
    let dependency = SimpleTitleDependency::from_lines(&static_dependents.lines());
    let mut examples = TemplateExamples::default();
    template_examples::collect(
        &mut open_pages()?,
        &SimpleMarkupParser::default(),
        &SimpleEntryParser::default(),
        &dependency,
        &mut examples,
    );
    println!("templates with examples: {}", examples.template_count());

    println!("rendering each example under its own title and a shared title");
    let renderer: Arc<dyn DefinitionRenderer> = Arc::new(SimpleMarkupRenderer::new(
        Box::new(SimpleHtmlEntityRenderer::default()),
        Box::new(LocalMediaWikiClient::default()),
    ));
    let verdicts =
        title_differential::compare_titles(renderer, &examples, TITLE_COMPARISON_CONCURRENCY)
            .await?;
    println!(
        "examples rendered: {}\ncleared (identical under both titles): {}\ntitle-dependent (an example differs): {}\nkept flagged (an example failed to render): {}\nkept flagged (no examples in the dump): {}",
        verdicts.examples,
        verdicts.cleared.len(),
        verdicts.differs.len(),
        verdicts.failed.len(),
        static_dependents
            .templates
            .len()
            .saturating_sub(examples.template_count())
    );

    std::fs::write(out, static_dependents.without_templates(&verdicts.cleared).lines())
        .map_err(|err| format!("{out}: {err}"))?;
    println!("written to {out}");

    Ok(())
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

    let title_dependency = match load_title_dependency() {
        Ok(title_dependency) => title_dependency,
        Err(err) => {
            println!("oops {err}");
            return ExitCode::FAILURE;
        }
    };

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
        Arc::new(markup_renderer),
        Box::new(title_dependency),
        &mut templates_out,
        &mut meta_out,
    )
    .await;

    if let Err(err) = result
        .and_then(|()| templates_out.flush())
        .and_then(|()| meta_out.flush())
    {
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
