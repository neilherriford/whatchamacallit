use crate::{
    markup_renderer::{DefinitionOutcome, DefinitionRenderer, Error},
    pipeline::{BATCH_TITLE, PipelineSummary},
    stats::csv_field,
};
use async_trait::async_trait;
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Instant,
};

const REQUESTS_HEADER: &str = "started_ms,elapsed_ms,definitions,batch,ok";
const META_HEADER: &str = "started_at_ms,finished_at_ms,elapsed_ms,concurrency,batch_size,batch_max_bytes,stride,setup_tag,git_commit,words,definitions,rendered,failed,transient,requests,failed_requests,request_seconds,solo_words,title_rerenders";

#[derive(Debug, Clone, PartialEq)]
pub struct RequestTiming {
    pub started_ms: f64,
    pub elapsed_ms: f64,
    pub definitions: usize,
    pub batch: bool,
    pub ok: bool,
}

pub struct TimedRenderer {
    inner: Arc<dyn DefinitionRenderer>,
    origin: Instant,
    timings: Mutex<Vec<RequestTiming>>,
}

impl TimedRenderer {
    pub fn new(inner: Arc<dyn DefinitionRenderer>) -> Self {
        Self {
            inner,
            origin: Instant::now(),
            timings: Mutex::new(Vec::new()),
        }
    }

    fn push(&self, timing: RequestTiming) {
        match self.timings.lock() {
            Ok(mut guard) => guard.push(timing),
            Err(poisoned) => poisoned.into_inner().push(timing),
        }
    }

    pub fn timings(&self) -> Vec<RequestTiming> {
        let mut found = match self.timings.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        found.sort_by(|left, right| left.started_ms.total_cmp(&right.started_ms));
        found
    }
}

#[async_trait]
impl DefinitionRenderer for TimedRenderer {
    async fn render_definitions(
        &self,
        page_title: &str,
        definitions: &[String],
    ) -> Result<Vec<DefinitionOutcome>, Error> {
        let started_ms = self.origin.elapsed().as_secs_f64() * 1000.0;
        let begun = Instant::now();
        let result = self.inner.render_definitions(page_title, definitions).await;

        self.push(RequestTiming {
            started_ms,
            elapsed_ms: begun.elapsed().as_secs_f64() * 1000.0,
            definitions: definitions.len(),
            batch: page_title == BATCH_TITLE,
            ok: result.is_ok(),
        });

        result
    }
}

pub struct RunFacts<'a> {
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub elapsed_ms: f64,
    pub concurrency: usize,
    pub batch_size: usize,
    pub batch_max_bytes: usize,
    pub stride: u64,
    pub setup_tag: &'a str,
    pub git_commit: &'a str,
}

pub fn write_requests(timings: &[RequestTiming], out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "{REQUESTS_HEADER}")?;

    for timing in timings {
        writeln!(
            out,
            "{:.3},{:.3},{},{},{}",
            timing.started_ms, timing.elapsed_ms, timing.definitions, timing.batch, timing.ok
        )?;
    }

    Ok(())
}

pub fn write_meta(
    facts: &RunFacts,
    summary: &PipelineSummary,
    timings: &[RequestTiming],
    out: &mut dyn Write,
) -> io::Result<()> {
    let failed_requests = timings.iter().filter(|timing| !timing.ok).count();
    let request_seconds = timings
        .iter()
        .fold(0.0, |total, timing| total + timing.elapsed_ms)
        / 1000.0;

    writeln!(out, "{META_HEADER}")?;
    writeln!(
        out,
        "{},{},{:.3},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{},{}",
        facts.started_at_ms,
        facts.finished_at_ms,
        facts.elapsed_ms,
        facts.concurrency,
        facts.batch_size,
        facts.batch_max_bytes,
        facts.stride,
        csv_field(facts.setup_tag),
        csv_field(facts.git_commit),
        summary.words,
        summary.definitions,
        summary.rendered,
        summary.failed,
        summary.transient,
        summary.requests,
        failed_requests,
        request_seconds,
        summary.solo_words,
        summary.title_rerenders
    )
}

#[cfg(test)]
mod tests;
