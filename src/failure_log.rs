use std::{
    collections::{BTreeMap, HashMap},
    io::{self, Write},
};

const TSV_HEADER: &str = "word\tdefinition_number\tkind\thash\tsetup_tag\terror\tmarkup";
const MARKDOWN_MARKUP_LIMIT: usize = 140;
const MARKDOWN_ERROR_LIMIT: usize = 110;
const SIGNATURE_LIMIT: usize = 110;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Error,
    Transient,
}

impl FailureKind {
    fn label(self) -> &'static str {
        match self {
            FailureKind::Error => "error",
            FailureKind::Transient => "transient",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "error" => Some(FailureKind::Error),
            "transient" => Some(FailureKind::Transient),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedDefinition {
    pub word: String,
    pub definition_number: usize,
    pub kind: FailureKind,
    pub error: String,
    pub markup: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedFailure {
    pub failure: FailedDefinition,
    pub hash: String,
    pub setup_tag: String,
}

pub fn markup_hash(markup: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for byte in markup.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }

    format!("{hash:016x}")
}

fn escape_field(text: &str) -> String {
    let mut out = String::new();

    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }

    out
}

fn unescape_field(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }

    out
}

pub fn signature(error: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;

    for ch in error.chars() {
        match quote {
            Some(open) if ch == open => {
                quote = None;
                out.push(ch);
            }
            Some(_) => {}
            None => {
                out.push(ch);
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                    out.push_str("...");
                }
            }
        }
    }

    out.chars().take(SIGNATURE_LIMIT).collect()
}

fn markdown_cell(text: &str, limit: usize) -> String {
    let flat = text.replace(['\n', '\r', '\t'], " ");
    let shortened = if flat.chars().count() > limit {
        format!("{}...", flat.chars().take(limit).collect::<String>())
    } else {
        flat
    };

    shortened
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
}

#[derive(Debug, Default)]
pub struct FailureLog {
    entries: Vec<FailedDefinition>,
}

impl FailureLog {
    pub fn record(&mut self, failure: FailedDefinition) {
        self.entries.push(failure);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn sorted(&self) -> Vec<&FailedDefinition> {
        let mut sorted = self.entries.iter().collect::<Vec<_>>();
        sorted.sort_by(|left, right| {
            (&left.word, left.definition_number).cmp(&(&right.word, right.definition_number))
        });
        sorted
    }

    pub fn write_tsv(&self, out: &mut dyn Write, setup_tag: &str) -> io::Result<()> {
        writeln!(out, "{TSV_HEADER}")?;

        for failure in self.sorted() {
            writeln!(
                out,
                "{word}\t{number}\t{kind}\t{hash}\t{tag}\t{error}\t{markup}",
                word = escape_field(&failure.word),
                number = failure.definition_number,
                kind = failure.kind.label(),
                hash = markup_hash(&failure.markup),
                tag = escape_field(setup_tag),
                error = escape_field(&failure.error),
                markup = escape_field(&failure.markup),
            )?;
        }

        Ok(())
    }

    pub fn write_markdown(&self, out: &mut dyn Write, setup_tag: &str) -> io::Result<()> {
        writeln!(out, "# Definitions that failed to render")?;
        writeln!(out)?;
        writeln!(
            out,
            "Server setup tag: `{tag}`. Each row is one definition. The definition number counts from 0 within the word's definitions. A transient failure is a request that kept failing, not a problem with the definition.",
            tag = setup_tag.replace('`', "'")
        )?;
        writeln!(out)?;
        writeln!(out, "Total: {}", self.entries.len())?;
        writeln!(out)?;
        writeln!(out, "## Counts by error")?;
        writeln!(out)?;
        writeln!(out, "| Error | Definitions |")?;
        writeln!(out, "|---|---:|")?;

        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for failure in &self.entries {
            *counts.entry(signature(&failure.error)).or_default() += 1;
        }
        let mut counted = counts.into_iter().collect::<Vec<_>>();
        counted.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        for (error, count) in counted {
            writeln!(
                out,
                "| {error} | {count} |",
                error = markdown_cell(&error, MARKDOWN_ERROR_LIMIT)
            )?;
        }

        writeln!(out)?;
        writeln!(out, "## Definitions")?;
        writeln!(out)?;
        writeln!(
            out,
            "| Word | Definition number | Kind | Error | Markup (first {MARKDOWN_MARKUP_LIMIT} characters) |"
        )?;
        writeln!(out, "|---|---:|---|---|---|")?;

        for failure in self.sorted() {
            writeln!(
                out,
                "| {word} | {number} | {kind} | {error} | {markup} |",
                word = markdown_cell(&failure.word, 60),
                number = failure.definition_number,
                kind = failure.kind.label(),
                error = markdown_cell(&failure.error, MARKDOWN_ERROR_LIMIT),
                markup = markdown_cell(&failure.markup, MARKDOWN_MARKUP_LIMIT),
            )?;
        }

        Ok(())
    }
}

pub fn read_tsv(text: &str) -> Vec<LoggedFailure> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.splitn(7, '\t');
            let word = fields.next()?;
            let number = fields.next()?.parse::<usize>().ok()?;
            let kind = FailureKind::parse(fields.next()?)?;
            let hash = fields.next()?;
            let setup_tag = fields.next()?;
            let error = fields.next()?;
            let markup = fields.next()?;

            Some(LoggedFailure {
                failure: FailedDefinition {
                    word: unescape_field(word),
                    definition_number: number,
                    kind,
                    error: unescape_field(error),
                    markup: unescape_field(markup),
                },
                hash: hash.to_string(),
                setup_tag: unescape_field(setup_tag),
            })
        })
        .collect()
}

pub fn split_for_retry(
    entries: Vec<LoggedFailure>,
    setup_tag: &str,
) -> (Vec<LoggedFailure>, Vec<LoggedFailure>) {
    entries.into_iter().partition(|entry| {
        entry.failure.kind == FailureKind::Transient || entry.setup_tag != setup_tag
    })
}

pub fn known_failures(entries: &[LoggedFailure], setup_tag: &str) -> HashMap<String, String> {
    entries
        .iter()
        .filter(|entry| entry.failure.kind == FailureKind::Error && entry.setup_tag == setup_tag)
        .map(|entry| (entry.hash.clone(), entry.failure.error.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
