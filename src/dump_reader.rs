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

pub trait DumpFile {
    fn read_event_into<'b>(&mut self, buf: &'b mut Vec<u8>) -> Result<Event<'b>, quick_xml::Error>;
}

pub struct LocalDumpFile {
    reader: Reader<BufReader<File>>,
}

impl LocalDumpFile {
    pub fn create(path: PathBuf) -> Result<Self, Error> {
        Ok(Self {
            reader: Reader::from_reader(BufReader::new(File::open(path)?)),
        })
    }

    pub fn default_path() -> PathBuf {
        [
            env!("CARGO_MANIFEST_DIR"),
            "dictionary",
            "enwiktionary-2026-10-01-p6p12232071.xml",
        ]
        .iter()
        .collect()
    }

    pub fn create_with_default_path() -> Result<Self, Error> {
        Self::create(Self::default_path())
    }
}

impl DumpFile for LocalDumpFile {
    fn read_event_into<'b>(&mut self, buf: &'b mut Vec<u8>) -> Result<Event<'b>, quick_xml::Error> {
        self.reader.read_event_into(buf)
    }
}

pub struct Page {
    pub word: String,
    pub markup: String,
}

pub struct DumpReader {
    dump_file: Box<dyn DumpFile>,
    buf: Vec<u8>,
    state: State,
    ns: Option<i32>,
    namespaces: Vec<i32>,
    title: Option<String>,
    text_buffer: String,
    finished: bool,
}

impl DumpReader {
    pub fn new(dump_file: Box<dyn DumpFile>) -> Result<Self, Error> {
        Self::with_namespaces(dump_file, &[0])
    }

    pub fn with_namespaces(
        dump_file: Box<dyn DumpFile>,
        namespaces: &[i32],
    ) -> Result<Self, Error> {
        Ok(Self {
            dump_file,
            buf: Vec::new(),
            state: State::Init,
            ns: None,
            namespaces: namespaces.to_vec(),
            title: None,
            text_buffer: String::new(),
            finished: false,
        })
    }

    fn next_page(&mut self) -> Result<Option<Page>, Error> {
        loop {
            self.buf.clear();

            match self.dump_file.read_event_into(&mut self.buf)? {
                Event::Start(e) => {
                    let qname = e.name();
                    let element_name: &str = qname.as_ref();

                    self.state = match (element_name, self.state) {
                        ("page", _) => State::Page,
                        ("title", State::Page) => State::Title,
                        ("ns", State::Page) => State::Ns,
                        ("revision", State::Page) => State::Revision,
                        ("text", State::Revision) => State::Body,
                        _ => self.state,
                    };
                }
                Event::Text(t) => {
                    if should_capture_text(self.state, self.ns, &self.namespaces) {
                        append_to_buffer(&mut self.text_buffer, &t);
                    }
                }
                Event::GeneralRef(r) => {
                    if should_capture_text(self.state, self.ns, &self.namespaces) {
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

                        self.text_buffer.push_str(&decoded);
                    }
                }

                Event::End(e) => {
                    let qname = e.name();
                    let element_name: &str = qname.as_ref();

                    match (element_name, self.state) {
                        ("page", State::Page) => {
                            let is_entry = is_wanted_namespace(self.ns, &self.namespaces);
                            let word = self.title.take();
                            let markup = std::mem::take(&mut self.text_buffer);

                            self.ns = None;
                            self.state = State::Init;

                            if is_entry && let Some(word) = word {
                                return Ok(Some(Page { word, markup }));
                            }
                        }
                        ("title", State::Title) => {
                            self.title = Some(std::mem::take(&mut self.text_buffer));
                            self.state = State::Page;
                        }
                        ("ns", State::Ns) => {
                            self.ns = match self.text_buffer.parse::<i32>() {
                                Ok(ns) => Some(ns),
                                Err(err) => {
                                    eprintln!(
                                        "Failed to parse namespace '{}': {err}",
                                        self.text_buffer
                                    );
                                    None
                                }
                            };
                            self.text_buffer.clear();
                            self.state = State::Page;
                        }
                        ("revision", State::Revision) => self.state = State::Page,
                        ("text", State::Body) => self.state = State::Revision,
                        _ => {}
                    }
                }
                Event::Eof => return Ok(None),
                _ => {}
            }
        }
    }
}

impl Iterator for DumpReader {
    type Item = Result<Page, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        match self.next_page() {
            Ok(Some(page)) => Some(Ok(page)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(err) => {
                self.finished = true;
                Some(Err(err))
            }
        }
    }
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

fn should_capture_text(current_state: State, current_ns: Option<i32>, namespaces: &[i32]) -> bool {
    match current_state {
        State::Title | State::Ns => true,
        State::Body => is_wanted_namespace(current_ns, namespaces),
        _ => false,
    }
}

fn append_to_buffer(buffer: &mut String, byte_text: &quick_xml::events::BytesText<'_>) {
    buffer.push_str(&String::from_utf8_lossy(byte_text.as_bytes()))
}

fn is_wanted_namespace(ns: Option<i32>, namespaces: &[i32]) -> bool {
    ns.is_some_and(|ns| namespaces.contains(&ns))
}

#[cfg(test)]
mod tests;
