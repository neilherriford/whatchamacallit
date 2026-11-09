mod database;
mod dump_reader;
mod entry_parser;
mod failure_log;
mod html_entity_renderer;
mod markup_parser;
mod markup_renderer;
mod model;
mod stats;
mod title_dependency;
mod utils;

use crate::{
    dump_reader::{DumpReader, LocalDumpFile},
    entry_parser::{EntryParser, SimpleEntryParser},
    html_entity_renderer::SimpleHtmlEntityRenderer,
    markup_parser::{MarkupParser, SimpleMarkupParser},
    markup_renderer::{LocalMediaWikiClient, MarkupRenderer, SimpleMarkupRenderer},
};
use std::{
    fs::File,
    io::{BufWriter, Write},
    process::ExitCode,
};
use tokio;

#[tokio::main]
async fn main() -> ExitCode {
    generate_samples().await
}

async fn generate_samples() -> ExitCode {
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

    let prefix = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "stat-runs/latest".to_string());
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
