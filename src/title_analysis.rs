use crate::{
    dump_reader::{Error as DumpError, Page},
    title_dependency::{MAGIC_WORDS, balanced_end, normalize, strip_prefix_ignore_case},
};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const CURRENT_TITLE_CALL: &str = "getCurrentTitle";
const TEMPLATE_PREFIX: &str = "Template:";
const MODULE_PREFIX: &str = "Module:";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleDependents {
    pub templates: BTreeSet<String>,
    pub modules: BTreeSet<String>,
    pub templates_analysed: usize,
    pub modules_analysed: usize,
}

impl TitleDependents {
    pub fn lines(&self) -> String {
        let templates = self
            .templates
            .iter()
            .map(|name| format!("{TEMPLATE_PREFIX}{name}\n"));
        let modules = self
            .modules
            .iter()
            .map(|name| format!("{MODULE_PREFIX}{name}\n"));

        templates.chain(modules).collect()
    }

    pub fn without_templates(&self, removed: &BTreeSet<String>) -> Self {
        Self {
            templates: self.templates.difference(removed).cloned().collect(),
            ..self.clone()
        }
    }
}

pub struct TitleAnalysis {
    magic: Regex,
    comment: Regex,
    no_include: Regex,
    redirect: Regex,
    invoke: Regex,
    head: Regex,
    dynamic_head: Regex,
}

impl TitleAnalysis {
    pub fn new() -> Result<Self, regex::Error> {
        let magic = [
            r"\{\{\s*(?:safesubst:|subst:)?(?:",
            &MAGIC_WORDS.join("|"),
            r")\s*\}\}",
        ]
        .concat();

        Ok(Self {
            magic: Regex::new(&magic)?,
            comment: Regex::new(r"(?s)<!--.*?-->")?,
            no_include: Regex::new(r"(?is)<noinclude>.*?</noinclude>")?,
            redirect: Regex::new(r"(?i)^\s*#REDIRECT\s*:?\s*\[\[\s*([^\]|#]+)")?,
            invoke: Regex::new(r"(?i)\{\{\s*#invoke:\s*([^|}]+?)\s*[|}]")?,
            head: Regex::new(r"\{\{\s*([^{}|#<>\[\]]+?)\s*[|}]")?,
            dynamic_head: Regex::new(r"\{\{\s*\{\{")?,
        })
    }

    pub fn static_dependents(
        &self,
        pages: impl Iterator<Item = Result<Page, DumpError>>,
    ) -> TitleDependents {
        let mut templates = BTreeMap::new();
        let mut modules = BTreeMap::new();

        for page in pages {
            let page = match page {
                Ok(page) => page,
                Err(err) => {
                    eprintln!("Page error: {err}");
                    break;
                }
            };

            if let Some(name) = page.word.strip_prefix(TEMPLATE_PREFIX) {
                templates.insert(name.to_string(), page.markup);
            } else if let Some(name) = page.word.strip_prefix(MODULE_PREFIX) {
                modules.insert(name.to_string(), page.markup);
            }
        }

        let title_modules = self.title_reading_modules(&modules);

        TitleDependents {
            templates: self.title_dependent_templates(&templates, &title_modules),
            templates_analysed: templates.len(),
            modules_analysed: modules.len(),
            modules: title_modules,
        }
    }

    fn title_reading_modules(&self, modules: &BTreeMap<String, String>) -> BTreeSet<String> {
        modules
            .iter()
            .filter(|(_, text)| text.contains(CURRENT_TITLE_CALL) || self.magic.is_match(text))
            .map(|(name, _)| name.clone())
            .collect()
    }

    fn title_dependent_templates(
        &self,
        templates: &BTreeMap<String, String>,
        title_modules: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        let known = templates.keys().cloned().collect::<BTreeSet<_>>();
        let mut seeds = BTreeSet::new();
        let mut dependents: HashMap<String, BTreeSet<String>> = HashMap::new();

        for (name, text) in templates {
            if let Some(target) = self.redirect_target(text, TEMPLATE_PREFIX) {
                dependents.entry(target).or_default().insert(name.clone());
                continue;
            }

            if self.redirect.is_match(text) {
                continue;
            }

            let body = self.transcludable_text(text);

            if self.magic.is_match(&body) || !self.invoked_modules(&body).is_disjoint(title_modules)
            {
                seeds.insert(name.clone());
            }

            for referenced in self.referenced_templates(&body, &known) {
                dependents
                    .entry(referenced)
                    .or_default()
                    .insert(name.clone());
            }
        }

        let mut flagged = seeds.clone();
        let mut pending = seeds.into_iter().collect::<Vec<_>>();

        while let Some(current) = pending.pop() {
            for dependent in dependents.get(&current).into_iter().flatten() {
                if flagged.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }

        flagged
    }

    fn transcludable_text(&self, text: &str) -> String {
        let without_comments = self.comment.replace_all(text, "");

        self.no_include
            .replace_all(&without_comments, "")
            .into_owned()
    }

    fn referenced_templates(&self, text: &str, known: &BTreeSet<String>) -> BTreeSet<String> {
        let mut found = BTreeSet::new();

        for captures in self.head.captures_iter(text) {
            let raw = normalize(&captures[1]);
            let candidates = [strip_namespace(&raw, TEMPLATE_PREFIX), raw];

            if let Some(candidate) = candidates.into_iter().find(|name| known.contains(name)) {
                found.insert(candidate);
            }
        }

        for matched in self.dynamic_head.find_iter(text) {
            let start = matched.start() + 2;
            let end = balanced_end(text, start).unwrap_or(text.len());

            for segment in text[start..end].split(['|', '}', '{']) {
                let candidate = strip_namespace(&normalize(segment), TEMPLATE_PREFIX);

                if known.contains(&candidate) {
                    found.insert(candidate);
                }
            }
        }

        found
    }

    fn invoked_modules(&self, text: &str) -> BTreeSet<String> {
        self.invoke
            .captures_iter(text)
            .map(|captures| strip_namespace(&normalize(&captures[1]), MODULE_PREFIX))
            .collect()
    }

    fn redirect_target(&self, text: &str, prefix: &str) -> Option<String> {
        let captures = self.redirect.captures(text)?;
        let target = normalize(&captures[1]);

        strip_prefix_ignore_case(&target, prefix).map(|rest| rest.trim().to_string())
    }
}

fn strip_namespace(name: &str, prefix: &str) -> String {
    strip_prefix_ignore_case(name, prefix)
        .map_or_else(|| name.to_string(), |rest| rest.trim().to_string())
}

#[cfg(test)]
mod tests;
