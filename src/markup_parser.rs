use crate::{markup_parser::Chunk::Indent, utils::trim_to_string};
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::LazyLock,
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PartOfSpeechParseError {
    #[error("Unknown part of speech: {0}")]
    UnknownPartOfSpeach(String),
}

#[derive(Debug, Clone)]
pub enum Chunk {
    PlainText(String),
    InternalLink(String),
    Template(Template),
    Indent,
}

#[derive(Debug, Clone, Copy)]
pub enum ListType {
    Ordered,
    Unordered,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: ListType,
    pub labels: Vec<String>,
    pub chunks: Vec<Chunk>,
}

impl std::fmt::Display for Chunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Chunk::PlainText(text) => f.write_str(text),
            Chunk::InternalLink(text) => f.write_str(text),
            Chunk::Template(template) => f.write_str(&template.to_string()),
            Indent => f.write_str(" "),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub contents: String,
}

impl std::fmt::Display for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.contents)
    }
}

#[derive(Debug)]
struct ParsedText {
    chunks: Vec<Chunk>,
    labels: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy, Hash)]
pub enum PartOfSpeech {
    #[default]
    Unknown,
    Abbreviation,
    Adjective,
    Adverb,
    Article,
    CombiningForm,
    Conjunction,
    Contraction,
    Determiner,
    DiacriticalMark,
    Idiom,
    Infix,
    Interfix,
    Interjection,
    IntransitiveVerb,
    Letter,
    Noun,
    Number,
    Numeral,
    Participle,
    Particle,
    Phrase,
    Postposition,
    Prefix,
    Preposition,
    PrepositionalPhrase,
    Pronoun,
    ProperNoun,
    Proverb,
    PunctuationMark,
    Suffix,
    Symbol,
    TransitiveVerb,
    Verb,
}

impl std::fmt::Display for PartOfSpeech {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PartOfSpeech::Unknown => "unknown",
            PartOfSpeech::Abbreviation => "abbreviation",
            PartOfSpeech::Adjective => "adjective",
            PartOfSpeech::Adverb => "adverb",
            PartOfSpeech::Article => "article",
            PartOfSpeech::CombiningForm => "combining form",
            PartOfSpeech::Conjunction => "conjunction",
            PartOfSpeech::Contraction => "contraction",
            PartOfSpeech::Determiner => "determiner",
            PartOfSpeech::DiacriticalMark => "diacritical mark",
            PartOfSpeech::Idiom => "idiom",
            PartOfSpeech::Infix => "infix",
            PartOfSpeech::Interfix => "interfix",
            PartOfSpeech::Interjection => "interjection",
            PartOfSpeech::IntransitiveVerb => "intransitive verb",
            PartOfSpeech::Letter => "letter",
            PartOfSpeech::Noun => "noun",
            PartOfSpeech::Number => "number",
            PartOfSpeech::Numeral => "numeral",
            PartOfSpeech::Participle => "participle",
            PartOfSpeech::Particle => "particle",
            PartOfSpeech::Phrase => "phrase",
            PartOfSpeech::Postposition => "postposition",
            PartOfSpeech::Prefix => "prefix",
            PartOfSpeech::Preposition => "preposition",
            PartOfSpeech::PrepositionalPhrase => "prepositional phrase",
            PartOfSpeech::Pronoun => "pronoun",
            PartOfSpeech::ProperNoun => "proper noun",
            PartOfSpeech::Proverb => "proverb",
            PartOfSpeech::PunctuationMark => "punctuation mark",
            PartOfSpeech::Suffix => "suffix",
            PartOfSpeech::Symbol => "symbol",
            PartOfSpeech::TransitiveVerb => "transitive verb",
            PartOfSpeech::Verb => "verb",
        })
    }
}

impl FromStr for PartOfSpeech {
    type Err = PartOfSpeechParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_ref() {
            "abbreviation" => Ok(PartOfSpeech::Abbreviation),
            "adjective" => Ok(PartOfSpeech::Adjective),
            "adverb" => Ok(PartOfSpeech::Adverb),
            "article" => Ok(PartOfSpeech::Article),
            "combining form" => Ok(PartOfSpeech::CombiningForm),
            "conjunction" => Ok(PartOfSpeech::Conjunction),
            "contraction" => Ok(PartOfSpeech::Contraction),
            "determiner" => Ok(PartOfSpeech::Determiner),
            "diacritical mark" => Ok(PartOfSpeech::DiacriticalMark),
            "idiom" => Ok(PartOfSpeech::Idiom),
            "infix" => Ok(PartOfSpeech::Infix),
            "interfix" => Ok(PartOfSpeech::Interfix),
            "interjection" => Ok(PartOfSpeech::Interjection),
            "intransitive verb" => Ok(PartOfSpeech::IntransitiveVerb),
            "letter" => Ok(PartOfSpeech::Letter),
            "noun" => Ok(PartOfSpeech::Noun),
            "number" => Ok(PartOfSpeech::Number),
            "numeral" => Ok(PartOfSpeech::Numeral),
            "participle" => Ok(PartOfSpeech::Participle),
            "particle" => Ok(PartOfSpeech::Particle),
            "phrase" => Ok(PartOfSpeech::Phrase),
            "postposition" => Ok(PartOfSpeech::Postposition),
            "prefix" => Ok(PartOfSpeech::Prefix),
            "preposition" => Ok(PartOfSpeech::Preposition),
            "prepositional phrase" => Ok(PartOfSpeech::PrepositionalPhrase),
            "pronoun" => Ok(PartOfSpeech::Pronoun),
            "proper noun" => Ok(PartOfSpeech::ProperNoun),
            "proverb" => Ok(PartOfSpeech::Proverb),
            "punctuation mark" => Ok(PartOfSpeech::PunctuationMark),
            "suffix" => Ok(PartOfSpeech::Suffix),
            "symbol" => Ok(PartOfSpeech::Symbol),
            "transitive verb" => Ok(PartOfSpeech::TransitiveVerb),
            "verb" => Ok(PartOfSpeech::Verb),

            unknown => Err(PartOfSpeechParseError::UnknownPartOfSpeach(
                unknown.to_string(),
            )),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum Language {
    Ignore,
    English,
}

const OPEN_TEMPLATE: &str = "{{";
const CLOSE_TEMPLATE: &str = "}}";
const OPEN_COMMENT: &str = "<!--";
const CLOSE_COMMENT: &str = "-->";
const OPEN_INTERNAL_LINK: &str = "[[";
const CLOSE_INTERNAL_LINK: &str = "]]";
const ARGUMENT_SEPERATOR: &str = "|";
const HEADER: char = '=';

fn logical_lines(text: &[char]) -> impl Iterator<Item = Vec<char>> {
    let mut index = 0;

    std::iter::from_fn(move || {
        if index >= text.len() {
            return None;
        }

        let mut line = Vec::new();
        let mut depth = 0;
        let mut in_comment = false;

        while index < text.len() {
            if in_comment {
                if matches_sequence_at_index(text, index, CLOSE_COMMENT) {
                    in_comment = false;
                    index += CLOSE_COMMENT.len();
                } else {
                    index += 1;
                }
                continue;
            }
            if matches_sequence_at_index(text, index, OPEN_COMMENT) {
                in_comment = true;
                index += OPEN_COMMENT.len();
                continue;
            }
            if matches_sequence_at_index(text, index, OPEN_TEMPLATE) {
                depth += 1;
                line.extend(&text[index..index + OPEN_TEMPLATE.len()]);
                index += OPEN_TEMPLATE.len();
                continue;
            }
            if matches_sequence_at_index(text, index, CLOSE_TEMPLATE) {
                if depth > 0 {
                    depth -= 1;
                }
                line.extend(&text[index..index + CLOSE_TEMPLATE.len()]);
                index += CLOSE_TEMPLATE.len();
                continue;
            }
            if text[index] == '\n' {
                let will_enter_header = text.get(index + 1) == Some(&HEADER);

                if depth == 0 || will_enter_header {
                    break;
                }
            }
            line.push(text[index]);
            index += 1;
        }

        index += 1;
        if line.last() == Some(&'\r') {
            line.pop();
        }
        Some(line)
    })
}

pub trait MarkupParser {
    fn parse(&self, text: &str) -> HashMap<PartOfSpeech, Vec<Entry>>;
}

#[derive(Debug, Default)]
pub struct SimpleMarkupParser {}

impl MarkupParser for SimpleMarkupParser {
    fn parse(&self, text: &str) -> HashMap<PartOfSpeech, Vec<Entry>> {
        let mut result = HashMap::new();
        let mut language = Language::Ignore;
        let mut seen_english = false;
        let mut part_of_speech = PartOfSpeech::Unknown;

        for line in logical_lines(&text.chars().collect::<Vec<_>>()) {
            if entering_header(&line) {
                if let Some((level, title)) = parse_header(&line) {
                    if level == 2 {
                        if &title == "English" {
                            seen_english = true;
                            language = Language::English
                        } else {
                            if seen_english {
                                break;
                            }
                            language = Language::Ignore;
                        }
                        continue;
                    }

                    if language == Language::English && level >= 3 {
                        part_of_speech = title.parse().unwrap_or_default();
                        continue;
                    }
                }
            }

            if language == Language::English && part_of_speech != PartOfSpeech::Unknown {
                let buffer: &mut Vec<Entry> = result.entry(part_of_speech).or_default();

                if let Some(kind) = is_entering_list(&line) {
                    if let Some(parsed_text) = parse_list(&line) {
                        if parsed_text.chunks.iter().all(
                            |chunk| matches!(chunk, Chunk::PlainText(text) if text.trim().is_empty()),
                        ) {
                            continue;
                        }

                        let mut seen = HashSet::new();
                        let mut labels = parsed_text.labels;
                        labels.retain(|label| seen.insert(label.clone()));

                        buffer.push(Entry {
                            kind,
                            labels,
                            chunks: parsed_text.chunks,
                        });
                    }
                }
            }
        }
        result
    }
}

fn entering_header(line: &[char]) -> bool {
    line.starts_with(&[HEADER])
}

fn is_entering_list(line: &[char]) -> Option<ListType> {
    let mut last_ch = '\0';

    for ch in line {
        match ch {
            '*' | '#' => last_ch = *ch,
            _ => break,
        }
    }

    match last_ch {
        '#' => Some(ListType::Ordered),
        '*' => Some(ListType::Unordered),
        _ => None,
    }
}

fn is_entering_template(text: &[char], index: usize) -> bool {
    matches_sequence_at_index(text, index, OPEN_TEMPLATE)
}

fn is_exiting_template(text: &[char], index: usize) -> bool {
    matches_sequence_at_index(text, index, CLOSE_TEMPLATE)
}

fn is_entering_internal_link(text: &[char], index: usize) -> bool {
    matches_sequence_at_index(text, index, OPEN_INTERNAL_LINK)
}

fn is_exiting_internal_link(text: &[char], index: usize) -> bool {
    matches_sequence_at_index(text, index, CLOSE_INTERNAL_LINK)
}

fn is_template_argument_separator(text: &[char], index: usize) -> bool {
    matches_sequence_at_index(text, index, ARGUMENT_SEPERATOR)
}

fn is_label_template(template: &Template) -> bool {
    match template.name.as_ref() {
        "label" | "lbl" | "lb" => true,
        _ => false,
    }
}

fn matches_sequence_at_index(text: &[char], index: usize, seq: &str) -> bool {
    for (offset, expected) in seq.chars().enumerate() {
        if text.get(index + offset) != Some(&expected) {
            return false;
        }
    }

    true
}

fn parse_header(line: &[char]) -> Option<(usize, String)> {
    let mut leading = 1;
    let mut trailing = 0;

    for index in 1..line.len() {
        if line[index] == HEADER {
            leading += 1;
        } else {
            break;
        }
    }

    let mut suffix = 0;
    for index in (1..line.len()).rev() {
        match line[index] {
            HEADER => {
                trailing += 1;
                suffix += 1;
            }
            '\n' | '\r' | ' ' | '\t' => suffix += 1,
            _ => break,
        }
    }

    if trailing == 0 {
        return None;
    }

    let level = leading.min(trailing);
    let end = line.len() - suffix;
    if end <= level {
        return None;
    }
    let title = trim_to_string(&line[level..end].iter().collect::<String>());
    if title.is_empty() {
        None
    } else {
        Some((level, title))
    }
}

fn parse_list(text: &[char]) -> Option<ParsedText> {
    let mut index = 0;

    loop {
        if index >= text.len() {
            break;
        }

        match text[index] {
            '#' | '*' => index += 1,
            _ => break,
        }
    }

    if index == 0 || index >= text.len() {
        return None;
    }

    let text = &text[index..];
    if text.iter().all(|ch| ch.is_whitespace()) {
        return None;
    }

    Some(parse_text(text))
}

fn parse_text(text: &[char]) -> ParsedText {
    let mut chunks = Vec::new();
    let mut labels = Vec::new();
    let mut buffer = String::new();
    let mut index = 0;
    let mut at_line_start = true;

    loop {
        if index >= text.len() {
            break;
        }

        if at_line_start && is_indent(text, index) {
            chunks.push(Chunk::Indent);
            index += 1;
            continue;
        }

        if is_entering_template(text, index) {
            at_line_start = false;
            let (new_index, template) = extract_template(text, index);

            let Some(template) = template else {
                buffer.push_str(OPEN_TEMPLATE);
                index += OPEN_TEMPLATE.len();
                continue;
            };

            if buffer.len() > 0 {
                chunks.push(Chunk::PlainText(buffer.clone()));
                buffer.clear();
            }
            index = new_index;

            if is_label_template(&template) {
                let arguments = extract_arguments_from_template(&template);
                if is_english(&arguments) {
                    labels.append(
                        &mut arguments
                            .ordinals()
                            .iter()
                            .filter(|ordinal| **ordinal != 1)
                            .filter_map(|ordinal| arguments.try_get_value_by_ordinal(*ordinal))
                            .collect::<Vec<_>>(),
                    );
                }
            }

            if include_template(&template) {
                chunks.push(Chunk::Template(template));
            }

            continue;
        }

        if is_entering_internal_link(text, index) {
            at_line_start = false;
            let (new_index, extracted) = extract_internal_link(text, index);

            let Some(parsed) = extracted else {
                buffer.push_str(OPEN_INTERNAL_LINK);
                index += OPEN_INTERNAL_LINK.len();
                continue;
            };

            if buffer.len() > 0 {
                chunks.push(Chunk::PlainText(buffer.clone()));
                buffer.clear();
            }
            index = new_index;
            chunks.push(Chunk::InternalLink(parsed));
            continue;
        }

        if !text[index].is_whitespace() {
            at_line_start = false;
        }
        buffer.push(text[index]);
        index += 1
    }

    if buffer.len() > 0 {
        chunks.push(Chunk::PlainText(buffer));
    }

    ParsedText { chunks, labels }
}

fn is_indent(text: &[char], index: usize) -> bool {
    text[index] == ':'
}

fn extract_template(text: &[char], index: usize) -> (usize, Option<Template>) {
    let mut new_index = index + OPEN_TEMPLATE.len();
    let mut buffer = String::new();
    let mut name = String::new();
    let mut is_collecting_name = true;
    let mut depth = 0;
    let mut valid = false;

    loop {
        if new_index >= text.len() {
            break;
        }

        let is_separator = is_template_argument_separator(text, new_index);
        let is_close = is_exiting_template(text, new_index);

        if is_entering_template(text, new_index) {
            buffer.push_str(OPEN_TEMPLATE);
            new_index += OPEN_TEMPLATE.len();
            depth += 1;
            continue;
        }

        if is_close {
            new_index += CLOSE_TEMPLATE.len();

            if depth == 0 {
                if name.is_empty() {
                    name = trim_to_string(&buffer);
                    buffer = name.clone();
                }

                valid = !name.is_empty();
                buffer.insert_str(0, OPEN_TEMPLATE);
                buffer.push_str(CLOSE_TEMPLATE);

                break;
            }
            buffer.push_str(CLOSE_TEMPLATE);
            depth -= 1;
            continue;
        }

        if is_collecting_name && is_separator && depth == 0 {
            name = trim_to_string(&buffer);
            buffer = name.clone();
            is_collecting_name = false;
        }

        buffer.push(text[new_index]);
        new_index += 1
    }

    (
        new_index,
        if valid {
            Some(Template {
                name: trim_to_string(&name).to_ascii_lowercase(),
                contents: buffer,
            })
        } else {
            None
        },
    )
}

pub(crate) struct TemplateArguments {
    arguments: HashMap<String, String>,
}

impl TemplateArguments {
    pub(crate) fn try_get_value_by_ordinal(&self, ordinal: usize) -> Option<String> {
        if let Some(value) = self.arguments.get(&ordinal.to_string()) {
            let result = trim_to_string(value);
            if result.is_empty() {
                None
            } else {
                Some(result)
            }
        } else {
            None
        }
    }

    pub(crate) fn ordinals(&self) -> Vec<usize> {
        let mut ordinals = self
            .arguments
            .keys()
            .filter_map(|key| {
                if let Some(ordinal) = key.parse::<usize>().ok() {
                    Some(ordinal)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        ordinals.sort();
        ordinals
    }
}

pub(crate) fn extract_arguments_from_template(template: &Template) -> TemplateArguments {
    let text = template
        .contents
        .chars()
        .skip(OPEN_TEMPLATE.len() + template.name.len() + ARGUMENT_SEPERATOR.len())
        .collect::<Vec<_>>();
    let mut index = 0;
    let mut next_ordinal = 1;
    let mut arguments = HashMap::new();
    let mut template_depth = 0;
    let mut link_depth = 0;
    let mut buffer = String::new();
    let mut used_ordinals = HashSet::new();
    let mut equals_at: Option<usize> = None;

    loop {
        if index >= text.len() {
            break;
        }

        let is_separator = is_template_argument_separator(&text, index);
        let is_close = is_exiting_template(&text, index);
        if is_entering_template(&text, index) {
            buffer.push_str(OPEN_TEMPLATE);
            index += OPEN_TEMPLATE.len();
            template_depth += 1;
            continue;
        }

        if is_entering_internal_link(&text, index) {
            buffer.push_str(OPEN_INTERNAL_LINK);
            index += OPEN_INTERNAL_LINK.len();
            link_depth += 1;
            continue;
        }

        if link_depth > 0 && is_exiting_internal_link(&text, index) {
            buffer.push_str(CLOSE_INTERNAL_LINK);
            index += CLOSE_INTERNAL_LINK.len();
            link_depth -= 1;
            continue;
        }

        if template_depth == 0 && ((is_separator && link_depth == 0) || is_close) {
            if let Some(position) = equals_at {
                let (name, value) = buffer.split_at(position);
                arguments.insert(name.to_string(), value[1..].to_string());
            } else {
                let mut proposed_ordinal = next_ordinal;

                loop {
                    if used_ordinals.insert(proposed_ordinal) {
                        break;
                    } else {
                        proposed_ordinal += 1;
                    }
                }

                arguments.insert(proposed_ordinal.to_string(), buffer.clone());
                next_ordinal = proposed_ordinal + 1;
            }
            buffer.clear();
            equals_at = None;

            if is_close {
                break;
            }

            index += 1;
            continue;
        }

        if is_close {
            buffer.push_str(CLOSE_TEMPLATE);
            template_depth -= 1;
            index += CLOSE_TEMPLATE.len();
            continue;
        }

        if text[index] == HEADER && template_depth == 0 && link_depth == 0 && equals_at.is_none() {
            equals_at = Some(buffer.len());
        }

        buffer.push(text[index]);
        index += 1;
    }

    TemplateArguments { arguments }
}

pub(crate) fn is_english(arguments: &TemplateArguments) -> bool {
    arguments
        .try_get_value_by_ordinal(1)
        .map(|code| code.to_ascii_lowercase())
        .is_some_and(|code| code == "en" || code.starts_with("en-"))
}

fn extract_internal_link(text: &[char], index: usize) -> (usize, Option<String>) {
    let mut new_index = index;
    let mut buffer = OPEN_INTERNAL_LINK.to_string();

    #[allow(unused_assignments)]
    let mut valid = false;
    let mut depth = 0;

    new_index += OPEN_INTERNAL_LINK.len();
    loop {
        if new_index >= text.len() {
            return (new_index, None);
        }

        if is_entering_template(text, new_index) {
            buffer.push_str(OPEN_TEMPLATE);
            new_index += OPEN_TEMPLATE.len();
            depth += 1;
            continue;
        }

        if is_exiting_template(text, new_index) {
            buffer.push_str(CLOSE_TEMPLATE);
            new_index += CLOSE_TEMPLATE.len();
            if depth > 0 {
                depth -= 1;
            }
            continue;
        }

        if is_exiting_internal_link(text, new_index) && depth == 0 {
            buffer.push_str(CLOSE_INTERNAL_LINK);
            new_index += CLOSE_INTERNAL_LINK.len();
            valid = true;
            break;
        }

        buffer.push(text[new_index]);
        new_index += 1;
    }

    (new_index, if valid { Some(buffer) } else { None })
}

fn template_name_deny_list() -> &'static HashSet<String> {
    static DENY: LazyLock<HashSet<String>> = LazyLock::new(|| {
        [
            "attention",
            "attn",
            "c",
            "cln",
            "defdate",
            "rfc-sense",
            "rfd-sense",
            "rfdef",
            "rfquotek",
            "rfv-sense",
            "senseid",
            "sid",
            "topics",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect::<HashSet<_>>()
    });
    &DENY
}

fn include_template(template: &Template) -> bool {
    !template_name_deny_list().contains(&template.name)
}

#[cfg(test)]
mod tests;
