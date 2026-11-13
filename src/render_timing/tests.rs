use super::*;
use std::time::Duration;

struct SlowRenderer {
    delay: Duration,
    fail: bool,
}

#[async_trait]
impl DefinitionRenderer for SlowRenderer {
    async fn render_definitions(
        &self,
        _page_title: &str,
        definitions: &[String],
    ) -> Result<Vec<DefinitionOutcome>, Error> {
        tokio::time::sleep(self.delay).await;

        if self.fail {
            return Err(Error::RenderingError("boom".to_string()));
        }

        Ok(definitions
            .iter()
            .map(|markup| DefinitionOutcome::Rendered(format!("r:{markup}")))
            .collect())
    }
}

fn timed(delay_ms: u64, fail: bool) -> TimedRenderer {
    TimedRenderer::new(Arc::new(SlowRenderer {
        delay: Duration::from_millis(delay_ms),
        fail,
    }))
}

fn definitions(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("d{index}")).collect()
}

fn timing(started_ms: f64, elapsed_ms: f64, ok: bool) -> RequestTiming {
    RequestTiming {
        started_ms,
        elapsed_ms,
        definitions: 2,
        batch: true,
        ok,
    }
}

fn facts() -> RunFacts<'static> {
    RunFacts {
        started_at_ms: 1000,
        finished_at_ms: 3500,
        elapsed_ms: 2500.0,
        concurrency: 20,
        batch_size: 200,
        batch_max_bytes: 120_000,
        stride: 10,
        setup_tag: "tuned",
        git_commit: "abc123",
    }
}

fn summary() -> PipelineSummary {
    PipelineSummary {
        words: 5,
        definitions: 9,
        rendered: 7,
        failed: 1,
        transient: 1,
        requests: 4,
        solo_words: 2,
        title_rerenders: 1,
        title_leak_words: Vec::new(),
    }
}

fn written(write: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
    let mut out = Vec::new();
    assert!(write(&mut out).is_ok());
    String::from_utf8(out).unwrap_or_default()
}

#[tokio::test]
async fn the_rendered_outcomes_pass_through_unchanged() {
    let renderer = timed(0, false);

    let found = renderer
        .render_definitions("wm.batch", &definitions(2))
        .await;

    assert_eq!(
        found.ok(),
        Some(vec![
            DefinitionOutcome::Rendered("r:d0".to_string()),
            DefinitionOutcome::Rendered("r:d1".to_string()),
        ])
    );
}

#[tokio::test]
async fn an_error_from_the_inner_renderer_passes_through() {
    let renderer = timed(0, true);

    let found = renderer
        .render_definitions("wm.batch", &definitions(1))
        .await;

    assert!(matches!(found, Err(Error::RenderingError(message)) if message == "boom"));
}

#[tokio::test]
async fn every_call_records_one_timing() {
    let renderer = timed(0, false);

    let _ = renderer
        .render_definitions("wm.batch", &definitions(3))
        .await;
    let _ = renderer.render_definitions("word", &definitions(1)).await;

    assert_eq!(renderer.timings().len(), 2);
}

#[tokio::test]
async fn a_timing_records_the_definition_count_and_whether_it_was_a_batch() {
    let renderer = timed(0, false);

    let _ = renderer
        .render_definitions("wm.batch", &definitions(3))
        .await;
    let _ = renderer.render_definitions("word", &definitions(1)).await;

    let timings = renderer.timings();
    assert_eq!(
        timings
            .iter()
            .map(|found| (found.definitions, found.batch))
            .collect::<Vec<_>>(),
        vec![(3, true), (1, false)]
    );
}

#[tokio::test]
async fn a_timing_covers_the_time_spent_inside_the_inner_renderer() {
    let renderer = timed(30, false);

    let _ = renderer
        .render_definitions("wm.batch", &definitions(1))
        .await;

    let timings = renderer.timings();
    assert!(timings[0].elapsed_ms >= 30.0);
}

#[tokio::test]
async fn a_failed_call_is_recorded_as_not_ok() {
    let renderer = timed(0, true);

    let _ = renderer
        .render_definitions("wm.batch", &definitions(1))
        .await;

    assert!(!renderer.timings()[0].ok);
}

#[tokio::test]
async fn a_successful_call_is_recorded_as_ok() {
    let renderer = timed(0, false);

    let _ = renderer
        .render_definitions("wm.batch", &definitions(1))
        .await;

    assert!(renderer.timings()[0].ok);
}

#[tokio::test]
async fn timings_come_back_ordered_by_start() {
    let renderer = timed(5, false);

    for _ in 0..3 {
        let _ = renderer
            .render_definitions("wm.batch", &definitions(1))
            .await;
    }

    let starts = renderer
        .timings()
        .iter()
        .map(|found| found.started_ms)
        .collect::<Vec<_>>();
    let mut sorted = starts.clone();
    sorted.sort_by(f64::total_cmp);
    assert_eq!(starts, sorted);
}

#[tokio::test]
async fn overlapping_calls_start_before_the_earlier_ones_finish() {
    let renderer = Arc::new(timed(40, false));

    let first = Arc::clone(&renderer);
    let second = Arc::clone(&renderer);
    let (_, _) = tokio::join!(
        async move { first.render_definitions("wm.batch", &definitions(1)).await },
        async move { second.render_definitions("wm.batch", &definitions(1)).await },
    );

    let timings = renderer.timings();
    assert!(timings[1].started_ms < timings[0].started_ms + timings[0].elapsed_ms);
}

#[test]
fn the_requests_file_starts_with_its_header() {
    let text = written(|out| write_requests(&[], out));

    assert_eq!(text, "started_ms,elapsed_ms,definitions,batch,ok\n");
}

#[test]
fn the_requests_file_has_one_row_per_timing() {
    let text =
        written(|out| write_requests(&[timing(1.5, 20.25, true), timing(3.0, 4.0, false)], out));

    assert_eq!(
        text,
        "started_ms,elapsed_ms,definitions,batch,ok\n1.500,20.250,2,true,true\n3.000,4.000,2,true,false\n"
    );
}

#[test]
fn the_meta_file_has_a_header_and_one_row() {
    let text = written(|out| write_meta(&facts(), &summary(), &[timing(0.0, 1000.0, true)], out));

    assert_eq!(text.lines().count(), 2);
}

#[test]
fn the_meta_row_lists_the_run_facts_and_the_pipeline_counts() {
    let text = written(|out| write_meta(&facts(), &summary(), &[timing(0.0, 1000.0, true)], out));

    assert_eq!(
        text.lines().nth(1),
        Some("1000,3500,2500.000,20,200,120000,10,\"tuned\",\"abc123\",5,9,7,1,1,4,0,1.000,2,1")
    );
}

#[test]
fn the_meta_row_sums_the_request_time_in_seconds() {
    let timings = [timing(0.0, 1500.0, true), timing(10.0, 2500.0, true)];

    let text = written(|out| write_meta(&facts(), &summary(), &timings, out));

    let row = text.lines().nth(1).unwrap_or_default().to_string();
    assert!(row.contains(",4.000,"));
}

#[test]
fn the_meta_row_counts_the_failed_requests() {
    let timings = [
        timing(0.0, 1.0, false),
        timing(1.0, 1.0, true),
        timing(2.0, 1.0, false),
    ];

    let text = written(|out| write_meta(&facts(), &summary(), &timings, out));

    let row = text.lines().nth(1).unwrap_or_default().to_string();
    assert!(row.contains(",4,2,0.003,"));
}

#[test]
fn a_setup_tag_with_a_comma_stays_in_one_field() {
    let mut run = facts();
    run.setup_tag = "a,b";

    let text = written(|out| write_meta(&run, &summary(), &[], out));

    assert!(text.contains(",\"a,b\","));
}

#[test]
fn a_run_with_no_requests_reports_zero_request_time() {
    let text = written(|out| write_meta(&facts(), &summary(), &[], out));

    assert!(text.contains(",4,0,0.000,"));
}
