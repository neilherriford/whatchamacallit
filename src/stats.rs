use crate::{
    dump_reader::DumpReader,
    entry_parser::EntryParser,
    markup_parser::MarkupParser,
    markup_renderer::{Error, MarkupRenderer, RenderReport},
    title_dependency::TitleDependency,
};
use std::{
    io::Write,
    process::Command,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::task::JoinSet;

const SAMPLE_STRIDE: u64 = 10;
const CONCURRENCY: usize = 20;
const BATCH_SIZE: usize = 200;
const BATCH_MAX_BYTES: usize = 120_000;
const BATCH_TITLE: &str = "wm.batch";
const DEFINITION_SEPARATOR: &str = "wm.\u{241c}";
const WORD_SEPARATOR: &str = "wm.\u{241e}";

#[derive(Debug, Clone)]
pub struct Timing {
    pub millis: f64,
    pub error: Option<String>,
    pub templates: usize,
    pub quotations: usize,
}

pub struct Stopwatch {
    instant: Instant,
    elapsed: Option<f64>,
}

impl Stopwatch {
    pub fn start() -> Self {
        Self {
            instant: Instant::now(),
            elapsed: None,
        }
    }

    pub fn stop(&mut self) -> f64 {
        if let Some(elapsed) = self.elapsed {
            elapsed
        } else {
            let elapsed = self.instant.elapsed().as_secs_f64() * 1000.0;
            self.elapsed = Some(elapsed);
            elapsed
        }
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

fn git_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn first_template_name(markup: &str) -> Option<&str> {
    let body = markup.trim_start().strip_prefix("{{")?;
    let end = body.find(['|', '}']).unwrap_or(body.len());
    Some(body[..end].trim())
}

fn is_form_of(markup: &str) -> bool {
    first_template_name(markup)
        .map(|name| name.to_ascii_lowercase().ends_with(" of"))
        .unwrap_or(false)
}

#[derive(Default)]
struct WordRecord {
    word: String,
    dump_position: u64,
    elapsed_page_load: f64,
    elapsed_parse_markup: f64,
    elapsed_parse_entry: f64,
    definition_count: usize,
    definition_bytes: usize,
    definition_templates: usize,
    definition_quotes: usize,
    form_of_count: usize,
    markups: Vec<String>,
    elapsed_render_time: f64,
    render_errors: u64,
    parser_cpu_seconds: f64,
    parser_wall_seconds: f64,
    lua_seconds: f64,
    lua_peak_memory_bytes: u64,
}

#[derive(Debug, Default)]
struct Observed {
    error_count: u64,
    requests: u64,
    solo_words: u64,
    lua_memory_limit_bytes: u64,
    lua_cpu_limit_seconds: f64,
}

fn batch_is_full(word_count: usize, markup_bytes: usize) -> bool {
    word_count >= BATCH_SIZE || markup_bytes >= BATCH_MAX_BYTES
}

fn join_definitions(markups: &[String]) -> String {
    markups.join(&format!("\n\n{DEFINITION_SEPARATOR}\n\n"))
}

fn join_words(chunks: &[String]) -> String {
    chunks.join(&format!("\n\n{WORD_SEPARATOR}\n\n"))
}

fn group_markup(records: &[WordRecord]) -> String {
    join_words(
        &records
            .iter()
            .map(|record| join_definitions(&record.markups))
            .collect::<Vec<_>>(),
    )
}

fn format_row(record: &WordRecord, finished_at_ms: u128) -> String {
    format!(
        "\"{word}\",{elapsed_page_load},{elapsed_parse_markup},{elapsed_parse_entry},{elapsed_render_time},{definition_count},{definition_bytes},{definition_templates},{definition_quotes},{dump_position},{finished_at_ms},{form_of_count},0,{render_errors},{parser_cpu_seconds},{parser_wall_seconds},{lua_seconds},{lua_peak_memory_bytes}",
        word = record.word,
        elapsed_page_load = record.elapsed_page_load,
        elapsed_parse_markup = record.elapsed_parse_markup,
        elapsed_parse_entry = record.elapsed_parse_entry,
        elapsed_render_time = record.elapsed_render_time,
        definition_count = record.definition_count,
        definition_bytes = record.definition_bytes,
        definition_templates = record.definition_templates,
        definition_quotes = record.definition_quotes,
        dump_position = record.dump_position,
        form_of_count = record.form_of_count,
        render_errors = record.render_errors,
        parser_cpu_seconds = record.parser_cpu_seconds,
        parser_wall_seconds = record.parser_wall_seconds,
        lua_seconds = record.lua_seconds,
        lua_peak_memory_bytes = record.lua_peak_memory_bytes,
    )
}

struct Completed {
    records: Vec<WordRecord>,
    rendered: Result<RenderReport, Error>,
    elapsed: f64,
}

async fn request_group(
    markup_renderer: &dyn MarkupRenderer,
    title: &str,
    records: &[WordRecord],
) -> (Result<RenderReport, Error>, f64) {
    let markup = group_markup(records);
    let mut timer = Stopwatch::start();
    let rendered = markup_renderer.render_report(title, &markup).await;
    (rendered, timer.stop())
}

fn attribute(
    records: &mut [WordRecord],
    rendered: Result<RenderReport, Error>,
    elapsed: f64,
    observed: &mut Observed,
    templates_out: &mut dyn Write,
) -> std::io::Result<()> {
    let total_definitions = records
        .iter()
        .map(|record| record.definition_count)
        .sum::<usize>();
    if total_definitions == 0 {
        return Ok(());
    }

    observed.requests += 1;

    for record in records.iter_mut() {
        let share = record.definition_count as f64 / total_definitions as f64;
        record.elapsed_render_time = elapsed * share;

        if let Ok(report) = &rendered {
            record.parser_cpu_seconds = report.parser_cpu_seconds * share;
            record.parser_wall_seconds = report.parser_wall_seconds * share;
            record.lua_seconds = report.lua_seconds * share;
            record.lua_peak_memory_bytes = report.lua_memory_bytes;
        }
    }

    match rendered {
        Ok(report) => {
            observed.lua_memory_limit_bytes = observed
                .lua_memory_limit_bytes
                .max(report.lua_memory_limit_bytes);
            observed.lua_cpu_limit_seconds =
                f64::max(observed.lua_cpu_limit_seconds, report.lua_cpu_limit_seconds);

            if let Some(first) = records.first() {
                for template in report.templates {
                    writeln!(
                        templates_out,
                        "{word},{name},{calls},{millis}",
                        word = csv_field(&first.word),
                        name = csv_field(&template.name),
                        calls = template.calls,
                        millis = template.millis
                    )?;
                }
            }
        }
        Err(err) => {
            let words = records
                .iter()
                .map(|record| record.word.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!("❗️{words}: Markup render error: {err}");
            observed.error_count += 1;
            if let Some(first) = records.first_mut() {
                first.render_errors += 1;
            }
        }
    }

    Ok(())
}

fn dispatch(
    in_flight: &mut JoinSet<Completed>,
    markup_renderer: &Arc<dyn MarkupRenderer>,
    title: String,
    records: Vec<WordRecord>,
) {
    let markup_renderer = Arc::clone(markup_renderer);
    in_flight.spawn(async move {
        let (rendered, elapsed) = request_group(markup_renderer.as_ref(), &title, &records).await;
        Completed {
            records,
            rendered,
            elapsed,
        }
    });
}

async fn drain_to(
    in_flight: &mut JoinSet<Completed>,
    limit: usize,
    observed: &mut Observed,
    templates_out: &mut dyn Write,
    total_definition_count: &mut usize,
) -> std::io::Result<()> {
    while in_flight.len() > limit {
        let Some(joined) = in_flight.join_next().await else {
            break;
        };
        let completed = joined.map_err(std::io::Error::other)?;
        let Completed {
            mut records,
            rendered,
            elapsed,
        } = completed;
        attribute(&mut records, rendered, elapsed, observed, templates_out)?;
        for record in &records {
            emit_row(record, total_definition_count);
        }
    }

    Ok(())
}

fn emit_row(record: &WordRecord, total_definition_count: &mut usize) {
    *total_definition_count += record.definition_count;
    println!("{}", format_row(record, now_millis()));
}

pub async fn run(
    pages: &mut DumpReader,
    markup_parser: Box<dyn MarkupParser>,
    entry_parser: Box<dyn EntryParser>,
    markup_renderer: Arc<dyn MarkupRenderer>,
    title_dependency: Box<dyn TitleDependency>,
    templates_out: &mut dyn Write,
    meta_out: &mut dyn Write,
) -> std::io::Result<()> {
    let started_at_ms = now_millis();
    let mut observed = Observed::default();
    let mut timed_page_count = 0;
    let mut sampled_page_count = 0;
    let mut total_definition_count = 0;
    let mut page_index = 0;
    let mut pending: Vec<WordRecord> = Vec::new();
    let mut pending_bytes = 0usize;
    let mut in_flight: JoinSet<Completed> = JoinSet::new();

    println!(
        "word,elapsed_page_load,elapsed_parse_markup,elapsed_parse_entry,elapsed_render_time,definition_count,definition_bytes,definition_templates,definition_quotes,dump_position,finished_at_ms,form_of_count,form_of_render_ms,render_errors,parser_cpu_seconds,parser_wall_seconds,lua_seconds,lua_peak_memory_bytes"
    );
    writeln!(templates_out, "word,template,calls,millis")?;
    let mut total = Stopwatch::start();

    loop {
        let mut timer = Stopwatch::start();
        let page = match pages.next() {
            Some(Ok(page)) => page,
            Some(Err(err)) => {
                eprintln!("Page error: {err}");
                break;
            }
            None => break,
        };

        if page_index % SAMPLE_STRIDE != 0 {
            page_index += 1;
            continue;
        }

        let elapsed_page_load = timer.stop();

        let dump_position = page_index;
        page_index += 1;
        sampled_page_count += 1;

        let mut timer = Stopwatch::start();
        let parsed_markup = markup_parser.parse(&page.markup);
        let elapsed_parse_markup = timer.stop();

        if parsed_markup.is_empty() {
            continue;
        }
        timed_page_count += 1;

        let mut record = WordRecord {
            word: page.word.clone(),
            dump_position,
            elapsed_page_load,
            elapsed_parse_markup,
            elapsed_parse_entry: 0.0,
            definition_count: 0,
            definition_bytes: 0,
            definition_templates: 0,
            definition_quotes: 0,
            form_of_count: 0,
            markups: Vec::new(),
            elapsed_render_time: 0.0,
            render_errors: 0,
            parser_cpu_seconds: 0.0,
            parser_wall_seconds: 0.0,
            lua_seconds: 0.0,
            lua_peak_memory_bytes: 0,
        };

        for (part_of_speech, entries) in parsed_markup {
            let mut timer = Stopwatch::start();
            let definitions = entry_parser.parse(part_of_speech, &entries);
            record.elapsed_parse_entry += timer.stop();

            for definition in definitions {
                record.definition_count += 1;
                record.definition_bytes += definition.markup.len();
                record.definition_templates += definition.markup.matches("{{").count();
                record.definition_quotes +=
                    definition.markup.matches(" Example quotation: ").count();

                if is_form_of(&definition.markup) {
                    record.form_of_count += 1;
                }

                record.markups.push(definition.markup);
            }
        }

        if record.markups.is_empty() {
            emit_row(&record, &mut total_definition_count);
            continue;
        }

        if title_dependency.requires_title(&record.markups.join("\n")) {
            observed.solo_words += 1;
            let title = record.word.clone();
            drain_to(
                &mut in_flight,
                CONCURRENCY - 1,
                &mut observed,
                templates_out,
                &mut total_definition_count,
            )
            .await?;
            dispatch(&mut in_flight, &markup_renderer, title, vec![record]);
            continue;
        }

        pending_bytes += record.markups.iter().map(String::len).sum::<usize>();
        pending.push(record);
        if batch_is_full(pending.len(), pending_bytes) {
            pending_bytes = 0;
            drain_to(
                &mut in_flight,
                CONCURRENCY - 1,
                &mut observed,
                templates_out,
                &mut total_definition_count,
            )
            .await?;
            dispatch(
                &mut in_flight,
                &markup_renderer,
                BATCH_TITLE.to_string(),
                std::mem::take(&mut pending),
            );
        }
    }

    if !pending.is_empty() {
        drain_to(
            &mut in_flight,
            CONCURRENCY - 1,
            &mut observed,
            templates_out,
            &mut total_definition_count,
        )
        .await?;
        dispatch(
            &mut in_flight,
            &markup_renderer,
            BATCH_TITLE.to_string(),
            std::mem::take(&mut pending),
        );
    }

    drain_to(
        &mut in_flight,
        0,
        &mut observed,
        templates_out,
        &mut total_definition_count,
    )
    .await?;

    let elapsed_ms = total.stop();

    writeln!(
        meta_out,
        "started_at_ms,finished_at_ms,elapsed_ms,sample_stride,concurrency,git_commit,pages_scanned,pages_sampled,pages_with_definitions,definitions,render_errors,lua_memory_limit_bytes,lua_cpu_limit_seconds,requests,solo_words"
    )?;
    writeln!(
        meta_out,
        "{started_at_ms},{finished_at_ms},{elapsed_ms},{SAMPLE_STRIDE},{CONCURRENCY},{commit},{page_index},{sampled_page_count},{timed_page_count},{total_definition_count},{error_count},{lua_memory_limit_bytes},{lua_cpu_limit_seconds},{requests},{solo_words}",
        finished_at_ms = now_millis(),
        commit = git_commit(),
        error_count = observed.error_count,
        lua_memory_limit_bytes = observed.lua_memory_limit_bytes,
        lua_cpu_limit_seconds = observed.lua_cpu_limit_seconds,
        requests = observed.requests,
        solo_words = observed.solo_words
    )?;

    println!(
        "\n\npage_count:   {timed_page_count}\nerror_count:  {error_count}\ntotal elapsed: {elapsed_ms}\n\n",
        error_count = observed.error_count
    );

    Ok(())
}

#[cfg(test)]
mod tests;
