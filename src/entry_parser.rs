use crate::{
    markup_parser::{
        Chunk, Entry, ListType, PartOfSpeech, Template, extract_arguments_from_template, is_english,
    },
    utils::trim_to_string,
};

pub struct Definition {
    pub part_of_speech: PartOfSpeech,
    pub labels: Vec<String>,
    pub markup: String,
    pub antonyms: Vec<String>,
}

pub trait EntryParser {
    fn parse(&self, part_of_speech: PartOfSpeech, entries: &[Entry]) -> Vec<Definition>;
}

#[derive(Debug, Default)]
pub struct SimpleEntryParser {}

impl EntryParser for SimpleEntryParser {
    fn parse(&self, part_of_speech: PartOfSpeech, entries: &[Entry]) -> Vec<Definition> {
        let mut result = Vec::new();

        if part_of_speech == PartOfSpeech::Unknown {
            return result;
        }

        let mut markup = String::new();
        let mut labels = Vec::new();
        let mut antonyms = Vec::new();

        for entry in entries {
            if let Some(terms) = extract_antonyms(entry) {
                for term in terms {
                    if !antonyms.contains(&term) {
                        antonyms.push(term);
                    }
                }

                continue;
            }

            if is_skipped_sub_line(entry) || is_empty_sub_line(entry) {
                continue;
            }

            if let Some(kind) = extract_example_type(entry) {
                let prefix = match kind {
                    ExampleType::Example => " Example: ",
                    ExampleType::Quotation => " Example quotation: ",
                };
                markup.push_str(prefix);
                append_chunks(&mut markup, &entry.chunks);
                labels.append(&mut entry.labels.clone());

                continue;
            }

            if !markup.is_empty() {
                result.push(Definition {
                    part_of_speech,
                    labels: labels.clone(),
                    antonyms: antonyms.clone(),
                    markup: markup.clone(),
                });

                labels.clear();
                antonyms.clear();
                markup.clear();
            }

            append_chunks(&mut markup, &entry.chunks);
            labels.append(&mut entry.labels.clone());
        }

        if !markup.is_empty() {
            result.push(Definition {
                part_of_speech,
                labels: labels.clone(),
                antonyms: antonyms.clone(),
                markup: markup.clone(),
            });
        }

        result
    }
}

fn append_chunks(buffer: &mut String, chunks: &[Chunk]) {
    for chunk in chunks {
        if matches!(chunk, Chunk::Indent) {
            continue;
        }
        buffer.push_str(&chunk.to_string());
    }
}

enum ExampleType {
    Example,
    Quotation,
}

fn extract_example_type(item: &Entry) -> Option<ExampleType> {
    if !is_sub_line(item) {
        return None;
    }

    let mut is_first_chunk = true;
    let mut first_chunk_is_indent = false;

    for chunk in &item.chunks {
        if is_first_chunk {
            first_chunk_is_indent = matches!(chunk, Chunk::Indent);
            is_first_chunk = false;
        }

        if let Chunk::Template(template) = chunk {
            if is_example_template(&template) {
                return Some(ExampleType::Example);
            }
            if is_quotation_template(&template) {
                return Some(ExampleType::Quotation);
            }
        }
    }

    if matches!(item.kind, ListType::Unordered) {
        Some(ExampleType::Quotation)
    } else if first_chunk_is_indent {
        Some(ExampleType::Example)
    } else {
        None
    }
}

fn is_sub_line(entry: &Entry) -> bool {
    matches!(entry.kind, ListType::Unordered) || matches!(entry.chunks.first(), Some(Chunk::Indent))
}

fn is_empty_sub_line(entry: &Entry) -> bool {
    is_sub_line(entry)
        && entry.chunks.iter().all(|chunk| match chunk {
            Chunk::Indent => true,
            Chunk::PlainText(text) => text.trim().is_empty(),
            Chunk::Template(template) => is_housekeeping_template(template),
            Chunk::InternalLink(_) => false,
        })
}

fn is_housekeeping_template(template: &Template) -> bool {
    matches!(
        template.name.as_ref(),
        "see citations" | "see more citations" | "seecites" | "seemorecites" | "rfdate" | "rfdatek"
    )
}

fn is_skipped_sub_line(entry: &Entry) -> bool {
    if !is_sub_line(entry) {
        return false;
    }

    let mut has_skipped_template = false;

    for chunk in &entry.chunks {
        if let Chunk::Template(template) = chunk {
            if is_example_template(template) || is_quotation_template(template) {
                return false;
            }
            if is_skipped_template(template) {
                has_skipped_template = true;
            }
        }
    }

    has_skipped_template
}

fn is_skipped_template(template: &Template) -> bool {
    match template.name.as_ref() {
        "syn" | "synonyms" | "nearsyn" | "near-syn" | "near-synonyms" | "nsyn" | "cot" | "co"
        | "coi" | "coord" | "coordinate terms" | "coa" | "comeronyms" | "hyper" | "hypernyms"
        | "hypo" | "hyponyms" | "hol" | "holo" | "holonyms" | "mer" | "mero" | "meronyms"
        | "alti" | "collocation" | "collocations" | "color panel" | "rfquote-sense"
        | "rfquotek" => true,
        name if name.starts_with("list:") => true,
        _ => false,
    }
}

fn extract_antonyms(entry: &Entry) -> Option<Vec<String>> {
    let template = entry.chunks.iter().find_map(|chunk| match chunk {
        Chunk::Template(template) => Some(template),
        _ => None,
    })?;

    if !is_antonym_template(template) {
        return None;
    }

    let arguments = extract_arguments_from_template(template);
    if !is_english(&arguments) {
        return Some(Vec::new());
    }

    Some(
        arguments
            .ordinals()
            .iter()
            .filter(|ordinal| **ordinal != 1)
            .filter_map(|ordinal| arguments.try_get_value_by_ordinal(*ordinal))
            .map(|term| strip_term_modifiers(&term))
            .filter(|term| !term.is_empty())
            .collect(),
    )
}

fn strip_term_modifiers(term: &str) -> String {
    trim_to_string(term.split('<').next().unwrap_or_default())
}

fn is_antonym_template(template: &Template) -> bool {
    matches!(template.name.as_ref(), "ant" | "antonym" | "antonyms")
}

fn is_example_template(template: &Template) -> bool {
    match trim_to_string(&template.name).to_ascii_lowercase().as_ref() {
        "ux" | "uxi" | "usex" | "uxa" | "suffixusex" | "prefixusex" | "afex" | "rfex"
        | "affixusex" | "sufex" | "prefex" => true,
        _ => false,
    }
}

fn is_quotation_template(template: &Template) -> bool {
    match trim_to_string(&template.name).to_ascii_lowercase().as_ref() {
        "c." | "see more citations" | "see citations" | "seemorecites" | "circa" | "ante"
        | "rfdatek" | "rfdate" => true,
        name if name.starts_with("quote") => true,
        name if name.starts_with("cite") => true,
        name if name.starts_with("rq:") => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
