use crate::{
    dump_reader::DumpReader, entry_parser::EntryParser, markup_parser::MarkupParser,
    markup_renderer::MarkupRenderer,
};
use std::time::Instant;

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

pub async fn run(
    pages: &mut DumpReader,
    markup_parser: Box<dyn MarkupParser>,
    entry_parser: Box<dyn EntryParser>,
    markup_renderer: Box<dyn MarkupRenderer>,
) {
    let mut error_count = 0;
    let mut timed_page_count = 0;
    let mut page_index = 0;

    println!(
        "word,elapsed_page_load,elapsed_parse_markup,elapsed_parse_entry,elapsed_render_time,definition_count,definition_bytes,definition_templates,definition_quotes"
    );
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

        if page_index % 1000 != 0 {
            page_index += 1;
            continue;
        }

        let elapsed_page_load = timer.stop();

        page_index += 1;

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

        for (part_of_speech, entries) in parsed_markup {
            let mut timer = Stopwatch::start();
            let definitions = entry_parser.parse(part_of_speech, &entries);
            elapsed_parse_entry += timer.stop();

            for definition in definitions {
                definition_count += 1;
                definition_bytes += definition.markup.len();
                definition_templates += definition.markup.matches("{{").count();
                definition_quotes += definition.markup.matches(" Example quotation: ").count();

                let mut timer = Stopwatch::start();
                if let Err(err) = markup_renderer.render(&page.word, &definition.markup).await {
                    eprintln!("❗️{word}: Markup render error: {err}", word = page.word);
                    error_count += 1;
                }
                elapsed_render_time += timer.stop();
            }
        }

        println!(
            "\"{word}\",{elapsed_page_load},{elapsed_parse_markup},{elapsed_parse_entry},{elapsed_render_time},{definition_count},{definition_bytes},{definition_templates},{definition_quotes}",
            word = page.word
        );
    }

    println!(
        "\n\npage_count:   {timed_page_count}\nerror_count:  {error_count}\ntotal elapsed: {}\n\n",
        total.stop()
    );
}
