use std::collections::HashSet;

pub(crate) const MAGIC_WORDS: [&str; 26] = [
    "PAGENAME",
    "PAGENAMEE",
    "FULLPAGENAME",
    "FULLPAGENAMEE",
    "BASEPAGENAME",
    "BASEPAGENAMEE",
    "ROOTPAGENAME",
    "ROOTPAGENAMEE",
    "SUBPAGENAME",
    "SUBPAGENAMEE",
    "NAMESPACE",
    "NAMESPACEE",
    "NAMESPACENUMBER",
    "TALKSPACE",
    "TALKSPACEE",
    "SUBJECTSPACE",
    "SUBJECTSPACEE",
    "ARTICLESPACE",
    "ARTICLESPACEE",
    "TALKPAGENAME",
    "TALKPAGENAMEE",
    "SUBJECTPAGENAME",
    "SUBJECTPAGENAMEE",
    "ARTICLEPAGENAME",
    "ARTICLEPAGENAMEE",
    "PAGEID",
];

const HEAD_TERMINATORS: [char; 8] = ['{', '}', '|', '#', '<', '>', '[', ']'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub template: String,
    pub text: String,
}

struct Head<'a> {
    text: &'a str,
    terminator_is_close: bool,
}

fn template_head(rest: &str) -> Option<Head<'_>> {
    let end = rest.find(HEAD_TERMINATORS).unwrap_or(rest.len());
    let after = &rest[end..];

    if !after.starts_with('|') && !after.starts_with('}') {
        return None;
    }

    Some(Head {
        text: rest[..end].trim(),
        terminator_is_close: after.starts_with("}}"),
    })
}

pub(crate) fn balanced_end(markup: &str, start: usize) -> Option<usize> {
    let bytes = markup.as_bytes();
    let mut depth = 0usize;
    let mut index = start;

    while index + 1 < bytes.len() {
        if bytes[index] == b'{' && bytes[index + 1] == b'{' {
            depth += 1;
            index += 2;
        } else if bytes[index] == b'}' && bytes[index + 1] == b'}' {
            depth = depth.checked_sub(1)?;
            index += 2;
            if depth == 0 {
                return Some(index);
            }
        } else {
            index += 1;
        }
    }

    None
}

pub trait TitleDependency: Send + Sync {
    fn requires_title(&self, markup: &str) -> bool;
}

#[derive(Debug, Default)]
pub struct SimpleTitleDependency {
    templates: HashSet<String>,
    modules: HashSet<String>,
}

impl SimpleTitleDependency {
    pub fn from_lines(text: &str) -> Self {
        let mut result = Self::default();

        for line in text.lines() {
            if let Some(name) = line.strip_prefix("Template:") {
                result.templates.insert(name.to_string());
            } else if let Some(name) = line.strip_prefix("Module:") {
                result.modules.insert(name.to_string());
            }
        }

        result
    }

    fn head_requires_title(&self, rest: &str) -> bool {
        let rest = rest.trim_start();

        if rest.starts_with('{') {
            return true;
        }

        if let Some(invoked) = strip_prefix_ignore_case(rest, "#invoke:") {
            return self.invoke_requires_title(invoked);
        }

        if let Some(head) = template_head(rest) {
            if head.terminator_is_close && is_magic_word(head.text) {
                return true;
            }
        }

        self.flagged_name(rest).is_some()
    }

    fn flagged_name(&self, rest: &str) -> Option<String> {
        let head = template_head(rest.trim_start())?;
        let name = normalize(head.text);
        let stripped = strip_prefix_ignore_case(&name, "Template:")
            .map(|stripped| stripped.trim().to_string())
            .unwrap_or_else(|| name.clone());

        if self.templates.contains(&stripped) {
            Some(stripped)
        } else if self.templates.contains(&name) {
            Some(name)
        } else {
            None
        }
    }

    pub fn flagged_invocations(&self, markup: &str) -> Vec<Invocation> {
        markup
            .match_indices("{{")
            .filter_map(|(index, _)| {
                let template = self.flagged_name(&markup[index + 2..])?;
                let end = balanced_end(markup, index)?;
                Some(Invocation {
                    template,
                    text: markup[index..end].to_string(),
                })
            })
            .collect()
    }

    fn invoke_requires_title(&self, invoked: &str) -> bool {
        let Some(end) = invoked.find(['|', '}']) else {
            return false;
        };
        let name = normalize(&invoked[..end]);
        let stripped = strip_prefix_ignore_case(&name, "Module:")
            .map(|stripped| stripped.trim().to_string())
            .unwrap_or(name);

        self.modules.contains(&stripped)
    }
}

impl TitleDependency for SimpleTitleDependency {
    fn requires_title(&self, markup: &str) -> bool {
        markup
            .match_indices("{{")
            .any(|(index, _)| self.head_requires_title(&markup[index + 2..]))
    }
}

pub(crate) fn normalize(name: &str) -> String {
    let name = name.replace('_', " ");
    let name = name.trim();
    name.strip_prefix(':').unwrap_or(name).trim().to_string()
}

pub(crate) fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn is_magic_word(head: &str) -> bool {
    let head = head
        .strip_prefix("safesubst:")
        .or_else(|| head.strip_prefix("subst:"))
        .unwrap_or(head)
        .trim();

    MAGIC_WORDS.contains(&head)
}

#[cfg(test)]
mod tests;
