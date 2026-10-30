use quick_xml::{Reader, events::Event};
use std::path::PathBuf;
use std::{fs::File, io::BufReader};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("XML Error: {0}")]
    XmlError(#[from] quick_xml::Error),
}

fn dictionary_path() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "dictionary",
        "enwiktionary-2026-10-01-p6p12232071.xml",
    ]
    .iter()
    .collect()
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum State {
    Init,
    Page,
    Title,
    Ns,
    Revision,
    Body,
}

pub fn test() -> Result<(), Error> {
    let mut reader = Reader::from_reader(BufReader::new(File::open(dictionary_path())?));
    let mut buf = Vec::new();
    let mut current_state = State::Init;
    let mut current_ns: Option<i32> = None;
    let mut current_title: Option<String> = None;
    let mut text_buffer = String::new();

    println!("---------------------------------");
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let qname = e.name();
                let element_name: &str = qname.as_ref();

                current_state = match (element_name, current_state) {
                    ("page", _) => State::Page,
                    ("title", State::Page) => State::Title,
                    ("ns", State::Page) => State::Ns,
                    ("revision", State::Page) => State::Revision,
                    ("text", State::Revision) => State::Body,
                    _ => current_state,
                };
            }
            Event::Text(t) => {
                if should_capture_text(current_state, current_ns) {
                    append_to_buffer(&mut text_buffer, &t);
                }
            }
            Event::GeneralRef(r) => {
                if should_capture_text(current_state, current_ns) {
                    let decoded: String = match r.resolve_char_ref()? {
                        Some(c) => c.to_string(),
                        None => match &*r {
                            "lt" => "<".into(),
                            "gt" => ">".into(),
                            "amp" => "&".into(),
                            "quot" => "\"".into(),
                            "apos" => "'".into(),
                            other => format!("&{other};"),
                        },
                    };

                    text_buffer.push_str(&decoded);
                }
            }
            Event::End(e) => {
                let qname = e.name();
                let element_name: &str = qname.as_ref();

                current_state = match (element_name, current_state) {
                    ("page", State::Page) => {
                        if is_primary_namespace(current_ns)
                            && let Some(title) = &current_title
                            && title == "nonsense"
                        {
                            println!("{title}\n- - - - - -\n{text_buffer}");
                            break;
                        }

                        current_ns = None;
                        current_title = None;
                        text_buffer.clear();
                        State::Init
                    }
                    ("title", State::Title) => {
                        current_title = Some(std::mem::take(&mut text_buffer));
                        State::Page
                    }
                    ("ns", State::Ns) => {
                        current_ns = match text_buffer.parse::<i32>() {
                            Ok(ns) => Some(ns),
                            Err(err) => {
                                eprintln!("Failed to parse namespace '{text_buffer}': {err}");
                                None
                            }
                        };
                        text_buffer.clear();
                        State::Page
                    }
                    ("revision", State::Revision) => State::Page,
                    ("text", State::Body) => State::Revision,
                    _ => current_state,
                };
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    println!("---------------------------------");

    Ok(())
}

fn should_capture_text(current_state: State, current_ns: Option<i32>) -> bool {
    match current_state {
        State::Title | State::Ns => true,
        State::Body => is_primary_namespace(current_ns),
        _ => false,
    }
}

fn append_to_buffer(buffer: &mut String, byte_text: &quick_xml::events::BytesText<'_>) {
    buffer.push_str(&String::from_utf8_lossy(byte_text.as_bytes()))
}

fn is_primary_namespace(ns: Option<i32>) -> bool {
    matches!(ns, Some(0))
}
