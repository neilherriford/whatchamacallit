use super::*;
use quick_xml::events::{BytesEnd, BytesRef, BytesStart, BytesText};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[test]
fn title_and_ns_text_is_always_captured() {
    assert!(should_capture_text(State::Title, None));
    assert!(should_capture_text(State::Title, Some(10)));
    assert!(should_capture_text(State::Ns, None));
    assert!(should_capture_text(State::Ns, Some(0)));
}

#[test]
fn body_text_is_captured_only_in_the_main_namespace() {
    assert!(should_capture_text(State::Body, Some(0)));
    assert!(!should_capture_text(State::Body, Some(10)));
    assert!(!should_capture_text(State::Body, Some(14)));
    assert!(!should_capture_text(State::Body, None));
}

#[test]
fn text_in_other_states_is_not_captured() {
    assert!(!should_capture_text(State::Init, Some(0)));
    assert!(!should_capture_text(State::Page, Some(0)));
    assert!(!should_capture_text(State::Revision, Some(0)));
}

#[test]
fn only_namespace_zero_is_primary() {
    assert!(is_primary_namespace(Some(0)));
    assert!(!is_primary_namespace(Some(1)));
    assert!(!is_primary_namespace(Some(10)));
    assert!(!is_primary_namespace(Some(-1)));
    assert!(!is_primary_namespace(None));
}

#[test]
fn append_to_buffer_adds_the_text() {
    let mut buffer = String::new();
    append_to_buffer(&mut buffer, &BytesText::new("héllo"));

    assert_eq!(buffer, "héllo");
}

#[test]
fn append_to_buffer_keeps_existing_text() {
    let mut buffer = String::from("one ");
    append_to_buffer(&mut buffer, &BytesText::new("two"));

    assert_eq!(buffer, "one two");
}

enum Step {
    Event(Event<'static>),
    Failure,
}

struct MockDumpFile {
    steps: VecDeque<Step>,
    buffer_lengths_seen: Arc<Mutex<Vec<usize>>>,
}

impl DumpFile for MockDumpFile {
    fn read_event_into<'b>(&mut self, buf: &'b mut Vec<u8>) -> Result<Event<'b>, quick_xml::Error> {
        match self.buffer_lengths_seen.lock() {
            Ok(mut guard) => guard.push(buf.len()),
            Err(poisoned) => poisoned.into_inner().push(buf.len()),
        }
        buf.extend_from_slice(b"leftover");

        match self.steps.pop_front() {
            Some(Step::Event(event)) => Ok(event),
            Some(Step::Failure) => {
                Err(quick_xml::Error::from(std::io::Error::other("read failed")))
            }
            None => Ok(Event::Eof),
        }
    }
}

fn start(name: &'static str) -> Step {
    Step::Event(Event::Start(BytesStart::new(name)))
}

fn end(name: &'static str) -> Step {
    Step::Event(Event::End(BytesEnd::new(name)))
}

fn empty(name: &'static str) -> Step {
    Step::Event(Event::Empty(BytesStart::new(name)))
}

fn comment(content: &str) -> Step {
    Step::Event(Event::Comment(BytesText::from_escaped(content.to_string())))
}

fn text(content: &str) -> Step {
    Step::Event(Event::Text(BytesText::from_escaped(content.to_string())))
}

fn reference(name: &str) -> Step {
    Step::Event(Event::GeneralRef(BytesRef::new(name.to_string())))
}

fn page(title: &str, ns: &str, body: &str) -> Vec<Step> {
    vec![
        start("page"),
        start("title"),
        text(title),
        end("title"),
        start("ns"),
        text(ns),
        end("ns"),
        start("revision"),
        start("text"),
        text(body),
        end("text"),
        end("revision"),
        end("page"),
    ]
}

fn reader_over(steps: Vec<Step>) -> (DumpReader, Arc<Mutex<Vec<usize>>>) {
    let buffer_lengths_seen = Arc::new(Mutex::new(Vec::new()));
    let dump_file = MockDumpFile {
        steps: steps.into(),
        buffer_lengths_seen: buffer_lengths_seen.clone(),
    };

    match DumpReader::new(Box::new(dump_file)) {
        Ok(reader) => (reader, buffer_lengths_seen),
        Err(err) => panic!("unexpected error: {err}"),
    }
}

fn pages_of(steps: Vec<Step>) -> Vec<(String, String)> {
    let (reader, _) = reader_over(steps);

    reader
        .filter_map(Result::ok)
        .map(|page| (page.word, page.markup))
        .collect()
}

fn entry(word: &str, markup: &str) -> (String, String) {
    (word.to_string(), markup.to_string())
}

#[test]
fn building_a_reader_over_a_dump_file_succeeds() {
    let dump_file = MockDumpFile {
        steps: VecDeque::new(),
        buffer_lengths_seen: Arc::new(Mutex::new(Vec::new())),
    };

    assert!(DumpReader::new(Box::new(dump_file)).is_ok());
}

#[test]
fn an_empty_dump_yields_no_pages() {
    assert!(pages_of(vec![]).is_empty());
}

#[test]
fn a_main_namespace_page_yields_its_title_and_text() {
    assert_eq!(
        pages_of(page("dictionary", "0", "==English==\n# A book.")),
        vec![entry("dictionary", "==English==\n# A book.")]
    );
}

#[test]
fn pages_are_yielded_in_dump_order() {
    let mut steps = page("one", "0", "first");
    steps.extend(page("two", "0", "second"));
    steps.extend(page("three", "0", "third"));

    assert_eq!(
        pages_of(steps),
        vec![
            entry("one", "first"),
            entry("two", "second"),
            entry("three", "third")
        ]
    );
}

#[test]
fn pages_in_other_namespaces_are_skipped() {
    let mut steps = page("Template:lb", "10", "template body");
    steps.extend(page("Category:English", "14", "category body"));
    steps.extend(page("Wiktionary:About", "4", "project body"));
    steps.extend(page("word", "0", "entry body"));

    assert_eq!(pages_of(steps), vec![entry("word", "entry body")]);
}

#[test]
fn a_page_with_a_non_numeric_namespace_is_skipped() {
    let mut steps = page("odd", "abc", "body");
    steps.extend(page("word", "0", "entry body"));

    assert_eq!(pages_of(steps), vec![entry("word", "entry body")]);
}

#[test]
fn a_page_without_a_namespace_is_skipped() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        start("revision"),
        start("text"),
        text("body"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert!(pages_of(steps).is_empty());
}

#[test]
fn a_page_without_a_title_is_skipped() {
    let steps = vec![
        start("page"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        text("body"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert!(pages_of(steps).is_empty());
}

#[test]
fn namespace_state_does_not_leak_into_the_next_page() {
    let mut steps = page("first", "0", "kept");
    steps.extend(vec![
        start("page"),
        start("title"),
        text("second"),
        end("title"),
        start("revision"),
        start("text"),
        text("dropped"),
        end("text"),
        end("revision"),
        end("page"),
    ]);

    assert_eq!(pages_of(steps), vec![entry("first", "kept")]);
}

#[test]
fn a_page_with_an_empty_body_yields_empty_text() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "")]);
}

#[test]
fn body_text_split_across_events_is_joined() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        text("one "),
        text("two "),
        text("three"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "one two three")]);
}

#[test]
fn predefined_entity_references_in_the_body_are_decoded() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        text("a "),
        reference("lt"),
        text("b"),
        reference("gt"),
        text(" "),
        reference("amp"),
        text(" "),
        reference("quot"),
        reference("apos"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "a <b> & \"'")]);
}

#[test]
fn numeric_character_references_in_the_body_are_decoded() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        reference("#65"),
        reference("#x42"),
        reference("#x1F600"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "AB😀")]);
}

#[test]
fn unknown_entity_references_in_the_body_are_kept_as_written() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        text("x"),
        reference("nbsp"),
        text("y"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "x&nbsp;y")]);
}

#[test]
fn entity_references_in_the_title_are_decoded() {
    let steps = vec![
        start("page"),
        start("title"),
        text("AT"),
        reference("amp"),
        text("T"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        text("body"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("AT&T", "body")]);
}

#[test]
fn an_invalid_character_reference_is_reported_as_an_xml_error() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        reference("#0"),
        end("text"),
        end("revision"),
        end("page"),
    ];
    let (mut reader, _) = reader_over(steps);

    assert!(matches!(reader.next(), Some(Err(Error::XmlError(_)))));
    assert!(reader.next().is_none());
}

#[test]
fn references_outside_captured_text_are_ignored() {
    let steps = vec![
        start("page"),
        reference("amp"),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        reference("lt"),
        start("text"),
        text("body"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "body")]);
}

#[test]
fn text_outside_the_title_namespace_and_body_is_ignored() {
    let steps = vec![
        start("page"),
        text("\n  "),
        start("title"),
        text("word"),
        end("title"),
        start("ns"),
        text("0"),
        end("ns"),
        start("id"),
        text("12345"),
        end("id"),
        start("revision"),
        start("timestamp"),
        text("2026-01-01T00:00:00Z"),
        end("timestamp"),
        start("text"),
        text("body"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "body")]);
}

#[test]
fn empty_elements_and_comments_are_ignored() {
    let steps = vec![
        start("page"),
        start("title"),
        text("word"),
        end("title"),
        empty("redirect"),
        comment("a note"),
        start("ns"),
        text("0"),
        end("ns"),
        start("revision"),
        start("text"),
        text("body"),
        end("text"),
        end("revision"),
        end("page"),
    ];

    assert_eq!(pages_of(steps), vec![entry("word", "body")]);
}

#[test]
fn the_iterator_stays_finished_after_the_dump_ends() {
    let (mut reader, _) = reader_over(page("word", "0", "body"));

    assert!(matches!(reader.next(), Some(Ok(_))));
    assert!(reader.next().is_none());
    assert!(reader.next().is_none());
}

#[test]
fn a_read_error_is_reported_once_and_ends_the_iterator() {
    let (mut reader, _) = reader_over(vec![Step::Failure]);

    assert!(matches!(reader.next(), Some(Err(Error::XmlError(_)))));
    assert!(reader.next().is_none());
}

#[test]
fn pages_before_a_read_error_are_still_yielded() {
    let mut steps = page("word", "0", "body");
    steps.push(Step::Failure);
    steps.extend(page("lost", "0", "never read"));
    let (mut reader, _) = reader_over(steps);

    assert!(matches!(
        reader.next(),
        Some(Ok(found)) if found.word == "word" && found.markup == "body"
    ));
    assert!(matches!(reader.next(), Some(Err(Error::XmlError(_)))));
    assert!(reader.next().is_none());
}

#[test]
fn a_read_error_in_the_middle_of_a_page_drops_that_page() {
    let steps = vec![start("page"), start("title"), text("word"), Step::Failure];
    let (mut reader, _) = reader_over(steps);

    assert!(matches!(reader.next(), Some(Err(_))));
    assert!(reader.next().is_none());
}

#[test]
fn the_read_buffer_is_cleared_before_every_read() {
    let (reader, buffer_lengths_seen) = reader_over(page("word", "0", "body"));
    let _ = reader.count();

    let seen = match buffer_lengths_seen.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    assert!(!seen.is_empty());
    assert!(seen.iter().all(|length| *length == 0));
}

#[test]
fn mock_dump_file_is_usable_as_a_trait_object() {
    let mut dump_file: Box<dyn DumpFile> = Box::new(MockDumpFile {
        steps: VecDeque::from(vec![start("page")]),
        buffer_lengths_seen: Arc::new(Mutex::new(Vec::new())),
    });
    let mut buf = Vec::new();

    assert!(matches!(
        dump_file.read_event_into(&mut buf),
        Ok(Event::Start(_))
    ));
    assert!(matches!(
        dump_file.read_event_into(&mut buf),
        Ok(Event::Eof)
    ));
}
