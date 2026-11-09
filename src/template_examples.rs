use crate::{
    dump_reader::DumpReader, entry_parser::EntryParser, markup_parser::MarkupParser,
    title_dependency::SimpleTitleDependency,
};
use std::collections::BTreeMap;

const EXAMPLES_PER_SHAPE: usize = 2;
const EXAMPLES_PER_TEMPLATE: usize = 12;

pub struct TemplateExamples {
    per_shape: usize,
    per_template: usize,
    examples: BTreeMap<String, Vec<(String, String, String)>>,
}

impl Default for TemplateExamples {
    fn default() -> Self {
        Self::new(EXAMPLES_PER_SHAPE, EXAMPLES_PER_TEMPLATE)
    }
}

impl TemplateExamples {
    pub fn new(per_shape: usize, per_template: usize) -> Self {
        Self {
            per_shape,
            per_template,
            examples: BTreeMap::new(),
        }
    }

    pub fn add_markup(&mut self, dependency: &SimpleTitleDependency, word: &str, markup: &str) {
        for invocation in dependency.flagged_invocations(markup) {
            let shape = argument_shape(&invocation.text);
            let entries = self.examples.entry(invocation.template).or_default();
            let known = entries.iter().any(|(seen_word, seen_text, _)| {
                seen_word == word || *seen_text == invocation.text
            });
            let in_shape = entries
                .iter()
                .filter(|(_, _, seen_shape)| *seen_shape == shape)
                .count();

            if entries.len() < self.per_template && in_shape < self.per_shape && !known {
                entries.push((word.to_string(), invocation.text, shape));
            }
        }
    }

    pub fn template_count(&self) -> usize {
        self.examples.len()
    }

    pub fn jobs(&self) -> Vec<(String, String, String)> {
        self.examples
            .iter()
            .flat_map(|(template, entries)| {
                entries
                    .iter()
                    .map(|(word, text, _)| (template.clone(), word.clone(), text.clone()))
            })
            .collect()
    }
}

pub fn argument_shape(invocation: &str) -> String {
    let body = invocation
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(invocation);
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let bytes = body.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let pair = bytes.get(index..index + 2);
        if matches!(pair, Some(b"{{") | Some(b"[[")) {
            depth += 1;
            current.push_str(&body[index..index + 2]);
            index += 2;
        } else if matches!(pair, Some(b"}}") | Some(b"]]")) {
            depth = depth.saturating_sub(1);
            current.push_str(&body[index..index + 2]);
            index += 2;
        } else if bytes[index] == b'|' && depth == 0 {
            arguments.push(std::mem::take(&mut current));
            index += 1;
        } else {
            let ch = body[index..].chars().next().unwrap_or(' ');
            current.push(ch);
            index += ch.len_utf8();
        }
    }
    arguments.push(current);

    let mut positional = 0;
    let mut named = Vec::new();
    for argument in arguments.iter().skip(1) {
        match argument.split_once('=') {
            Some((key, _)) if is_parameter_name(key) => named.push(key.trim().to_string()),
            _ => positional += 1,
        }
    }
    named.sort();
    named.dedup();

    format!("{positional}|{}", named.join(","))
}

fn is_parameter_name(key: &str) -> bool {
    let key = key.trim();
    !key.is_empty() && !key.contains(['{', '}', '[', ']', '<', '>'])
}

pub fn collect(
    pages: &mut DumpReader,
    markup_parser: &dyn MarkupParser,
    entry_parser: &dyn EntryParser,
    dependency: &SimpleTitleDependency,
    examples: &mut TemplateExamples,
) {
    for page in pages {
        let page = match page {
            Ok(page) => page,
            Err(err) => {
                eprintln!("Page error: {err}");
                break;
            }
        };

        for (part_of_speech, entries) in markup_parser.parse(&page.markup) {
            for definition in entry_parser.parse(part_of_speech, &entries) {
                examples.add_markup(dependency, &page.word, &definition.markup);
            }
        }
    }
}

#[cfg(test)]
mod tests;
