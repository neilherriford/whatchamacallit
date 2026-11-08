use crate::{
    dump_reader::DumpReader, entry_parser::EntryParser, markup_parser::MarkupParser,
    markup_renderer::MarkupRenderer,
};
use std::{
    collections::BTreeMap,
    io::Write,
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const SAMPLE_STRIDE: u64 = 1000;
const CONCURRENCY: u64 = 1;
const DEFINITION_SEPARATOR: &str = "wm.\u{241c}";

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
struct TemplateTotals {
    calls: u64,
    millis: f64,
}

pub async fn run(
    pages: &mut DumpReader,
    markup_parser: Box<dyn MarkupParser>,
    entry_parser: Box<dyn EntryParser>,
    markup_renderer: Box<dyn MarkupRenderer>,
    templates_out: &mut dyn Write,
    meta_out: &mut dyn Write,
) -> std::io::Result<()> {
    let started_at_ms = now_millis();
    let mut error_count = 0;
    let mut timed_page_count = 0;
    let mut sampled_page_count = 0;
    let mut total_definition_count = 0;
    let mut page_index = 0;
    let mut lua_memory_limit_bytes = 0;
    let mut lua_cpu_limit_seconds = 0.0;

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

        let mut definition_count = 0;
        let mut definition_bytes = 0;
        let mut definition_templates = 0;
        let mut definition_quotes = 0;
        let mut elapsed_render_time = 0.0;
        let mut elapsed_parse_entry = 0.0;
        let mut form_of_count = 0;
        let form_of_render_ms = 0.0;
        let mut render_errors = 0;
        let mut parser_cpu_seconds = 0.0;
        let mut parser_wall_seconds = 0.0;
        let mut lua_seconds = 0.0;
        let mut lua_peak_memory_bytes = 0;
        let mut template_totals: BTreeMap<String, TemplateTotals> = BTreeMap::new();

        let mut markups: Vec<String> = Vec::new();

        for (part_of_speech, entries) in parsed_markup {
            let mut timer = Stopwatch::start();
            let definitions = entry_parser.parse(part_of_speech, &entries);
            elapsed_parse_entry += timer.stop();

            for definition in definitions {
                definition_count += 1;
                definition_bytes += definition.markup.len();
                definition_templates += definition.markup.matches("{{").count();
                definition_quotes += definition.markup.matches(" Example quotation: ").count();

                if is_form_of(&definition.markup) {
                    form_of_count += 1;
                }

                markups.push(definition.markup);
            }
        }

        if !markups.is_empty() {
            let mut timer = Stopwatch::start();
            let rendered = markup_renderer
                .render_report(&page.word, &join_definitions(&markups))
                .await;
            elapsed_render_time += timer.stop();

            match rendered {
                Ok(report) => {
                    parser_cpu_seconds += report.parser_cpu_seconds;
                    parser_wall_seconds += report.parser_wall_seconds;
                    lua_seconds += report.lua_seconds;
                    lua_peak_memory_bytes = lua_peak_memory_bytes.max(report.lua_memory_bytes);
                    lua_memory_limit_bytes = lua_memory_limit_bytes.max(report.lua_memory_limit_bytes);
                    lua_cpu_limit_seconds = f64::max(lua_cpu_limit_seconds, report.lua_cpu_limit_seconds);

                    for template in report.templates {
                        let totals = template_totals.entry(template.name).or_default();
                        totals.calls += template.calls;
                        totals.millis += template.millis;
                    }
                }
                Err(err) => {
                    eprintln!("❗️{word}: Markup render error: {err}", word = page.word);
                    error_count += 1;
                    render_errors += 1;
                }
            }
        }

        for (name, totals) in &template_totals {
            writeln!(
                templates_out,
                "{word},{name},{calls},{millis}",
                word = csv_field(&page.word),
                name = csv_field(name),
                calls = totals.calls,
                millis = totals.millis
            )?;
        }

        total_definition_count += definition_count;

        println!(
            "\"{word}\",{elapsed_page_load},{elapsed_parse_markup},{elapsed_parse_entry},{elapsed_render_time},{definition_count},{definition_bytes},{definition_templates},{definition_quotes},{dump_position},{finished_at_ms},{form_of_count},{form_of_render_ms},{render_errors},{parser_cpu_seconds},{parser_wall_seconds},{lua_seconds},{lua_peak_memory_bytes}",
            word = page.word,
            finished_at_ms = now_millis()
        );
    }

    let elapsed_ms = total.stop();

    writeln!(
        meta_out,
        "started_at_ms,finished_at_ms,elapsed_ms,sample_stride,concurrency,git_commit,pages_scanned,pages_sampled,pages_with_definitions,definitions,render_errors,lua_memory_limit_bytes,lua_cpu_limit_seconds"
    )?;
    writeln!(
        meta_out,
        "{started_at_ms},{finished_at_ms},{elapsed_ms},{SAMPLE_STRIDE},{CONCURRENCY},{commit},{page_index},{sampled_page_count},{timed_page_count},{total_definition_count},{error_count},{lua_memory_limit_bytes},{lua_cpu_limit_seconds}",
        finished_at_ms = now_millis(),
        commit = git_commit()
    )?;

    println!(
        "\n\npage_count:   {timed_page_count}\nerror_count:  {error_count}\ntotal elapsed: {elapsed_ms}\n\n"
    );

    Ok(())
}

fn join_definitions(markups: &[String]) -> String {
    markups.join(&format!("\n\n{DEFINITION_SEPARATOR}\n\n"))
}

#[cfg(test)]
mod tests;
