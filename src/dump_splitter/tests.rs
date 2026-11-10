use super::*;
use std::{cell::RefCell, io::Cursor, rc::Rc};

type Written = Rc<RefCell<Vec<(usize, Vec<u8>)>>>;

struct SharedWriter {
    written: Written,
    slot: usize,
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written.borrow_mut()[self.slot]
            .1
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

const HEADER: &str =
    "<mediawiki version=\"0.11\">\n  <siteinfo>\n    <sitename>Test</sitename>\n  </siteinfo>\n";

fn page(title: &str, namespace: &str, text: &str) -> String {
    format!(
        "  <page>\n    <title>{title}</title>\n    <ns>{namespace}</ns>\n    <revision>\n      <text bytes=\"{bytes}\" xml:space=\"preserve\">{text}</text>\n    </revision>\n  </page>\n",
        bytes = text.len()
    )
}

fn dump(pages: &[String]) -> String {
    format!("{HEADER}{}</mediawiki>\n", pages.concat())
}

fn split(input: &str, parts: usize, keep_every: usize) -> (Vec<PartStats>, Vec<(usize, String)>) {
    let written: Written = Rc::new(RefCell::new(Vec::new()));
    let sink = written.clone();
    let mut open = move |index: usize| -> io::Result<Box<dyn Write>> {
        sink.borrow_mut().push((index, Vec::new()));
        let slot = sink.borrow().len() - 1;
        Ok(Box::new(SharedWriter {
            written: sink.clone(),
            slot,
        }))
    };

    let stats = match split_dump(
        Cursor::new(input.as_bytes().to_vec()),
        input.len() as u64,
        parts,
        keep_every,
        &mut open,
    ) {
        Ok(stats) => stats,
        Err(err) => panic!("unexpected error: {err}"),
    };
    let parts = written
        .borrow()
        .iter()
        .map(|(index, bytes)| (*index, String::from_utf8_lossy(bytes).to_string()))
        .collect();

    (stats, parts)
}

fn pages_in(part: &str) -> Vec<String> {
    part.split("  <page>\n")
        .skip(1)
        .map(|page| format!("  <page>\n{}", page.trim_end_matches("</mediawiki>\n")))
        .collect()
}

fn sample_pages(count: usize) -> Vec<String> {
    (0..count)
        .map(|n| {
            page(
                &format!("w{n}"),
                if n % 5 == 0 { "10" } else { "0" },
                &"x".repeat(100),
            )
        })
        .collect()
}

#[test]
fn every_part_starts_with_the_header_and_ends_with_the_footer() {
    let (_, parts) = split(&dump(&sample_pages(10)), 3, 1);

    assert!(!parts.is_empty());
    for (_, text) in &parts {
        assert!(text.starts_with(HEADER));
        assert!(text.ends_with("</page>\n</mediawiki>\n"));
    }
}

#[test]
fn no_page_is_lost_or_duplicated_across_the_parts() {
    let pages = sample_pages(25);
    let (_, parts) = split(&dump(&pages), 4, 1);

    let rejoined = parts
        .iter()
        .flat_map(|(_, text)| pages_in(text))
        .collect::<Vec<_>>();

    assert_eq!(rejoined, pages);
}

#[test]
fn pages_are_spread_over_roughly_the_requested_number_of_parts() {
    let (stats, parts) = split(&dump(&sample_pages(40)), 4, 1);

    assert_eq!(stats.len(), 4);
    assert_eq!(parts.len(), 4);
    for part in &stats {
        let pages = part.pages.values().sum::<u64>();
        assert!((8..=12).contains(&pages));
    }
}

#[test]
fn a_single_part_holds_every_page() {
    let (stats, parts) = split(&dump(&sample_pages(7)), 1, 1);

    assert_eq!(stats.len(), 1);
    assert_eq!(pages_in(&parts[0].1).len(), 7);
}

#[test]
fn asking_for_zero_parts_behaves_like_one() {
    let (stats, _) = split(&dump(&sample_pages(7)), 0, 1);

    assert_eq!(stats.len(), 1);
}

#[test]
fn more_parts_than_pages_makes_one_part_per_page_at_most() {
    let (stats, parts) = split(&dump(&sample_pages(3)), 50, 1);

    assert!(stats.len() <= 3);
    assert_eq!(parts.len(), stats.len());
}

#[test]
fn pages_are_counted_per_namespace() {
    let pages = vec![
        page("a", "0", "one"),
        page("Template:b", "10", "two"),
        page("c", "0", "three"),
    ];
    let (stats, _) = split(&dump(&pages), 1, 1);

    assert_eq!(stats[0].pages.get(&0), Some(&2));
    assert_eq!(stats[0].pages.get(&10), Some(&1));
}

#[test]
fn text_bytes_are_summed_per_namespace() {
    let pages = vec![
        page("a", "0", "12345"),
        page("Template:b", "10", "123"),
        page("c", "0", "1234567"),
    ];
    let (stats, _) = split(&dump(&pages), 1, 1);

    assert_eq!(stats[0].text_bytes.get(&0), Some(&12));
    assert_eq!(stats[0].text_bytes.get(&10), Some(&3));
}

#[test]
fn a_page_without_a_text_size_adds_no_bytes() {
    let missing = "  <page>\n    <title>a</title>\n    <ns>0</ns>\n    <revision>\n      <text xml:space=\"preserve\">abc</text>\n    </revision>\n  </page>\n";
    let (stats, _) = split(&dump(&[missing.to_string()]), 1, 1);

    assert_eq!(stats[0].pages.get(&0), Some(&1));
    assert_eq!(stats[0].text_bytes.get(&0), Some(&0));
}

#[test]
fn a_page_without_a_namespace_is_written_but_not_counted() {
    let missing = "  <page>\n    <title>a</title>\n    <revision>\n      <text bytes=\"3\">abc</text>\n    </revision>\n  </page>\n";
    let (stats, parts) = split(&dump(&[missing.to_string()]), 1, 1);

    assert!(stats[0].pages.is_empty());
    assert_eq!(pages_in(&parts[0].1).len(), 1);
}

#[test]
fn only_every_kth_part_is_written_but_all_are_counted() {
    let (stats, parts) = split(&dump(&sample_pages(40)), 8, 3);

    assert_eq!(stats.len(), 8);
    assert_eq!(
        stats.iter().map(|part| part.kept).collect::<Vec<_>>(),
        vec![true, false, false, true, false, false, true, false]
    );
    assert_eq!(
        parts.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        vec![0, 3, 6]
    );
    assert_eq!(
        totals(&stats).values().map(|(pages, _)| pages).sum::<u64>(),
        40
    );
}

#[test]
fn asking_to_keep_every_zeroth_part_keeps_every_part() {
    let (stats, parts) = split(&dump(&sample_pages(12)), 3, 0);

    assert!(stats.iter().all(|part| part.kept));
    assert_eq!(parts.len(), 3);
}

#[test]
fn an_input_with_no_pages_makes_no_parts() {
    let (stats, parts) = split(&dump(&[]), 4, 1);

    assert!(stats.is_empty());
    assert!(parts.is_empty());
}

#[test]
fn an_empty_input_makes_no_parts() {
    let (stats, parts) = split("", 4, 1);

    assert!(stats.is_empty());
    assert!(parts.is_empty());
}

#[test]
fn a_dump_cut_in_the_middle_of_a_page_is_an_error() {
    let truncated = format!("{HEADER}  <page>\n    <title>a</title>\n");
    let mut open = |_: usize| -> io::Result<Box<dyn Write>> { Ok(Box::new(io::sink())) };

    let result = split_dump(
        Cursor::new(truncated.as_bytes().to_vec()),
        truncated.len() as u64,
        2,
        1,
        &mut open,
    );

    assert_eq!(
        result.map_err(|err| err.kind()),
        Err(io::ErrorKind::UnexpectedEof)
    );
}

#[test]
fn a_failing_part_opener_stops_the_split() {
    let input = dump(&sample_pages(3));
    let mut open = |_: usize| -> io::Result<Box<dyn Write>> { Err(io::Error::other("disk full")) };

    let result = split_dump(
        Cursor::new(input.as_bytes().to_vec()),
        input.len() as u64,
        2,
        1,
        &mut open,
    );

    assert!(result.is_err());
}

#[test]
fn text_containing_page_like_text_does_not_end_a_page() {
    let tricky = page("a", "0", "&lt;page&gt; and &lt;/page&gt;");
    let (stats, parts) = split(&dump(&[tricky.clone(), page("b", "0", "x")]), 1, 1);

    assert_eq!(stats[0].pages.get(&0), Some(&2));
    assert_eq!(pages_in(&parts[0].1)[0], tricky);
}

#[test]
fn a_page_with_a_multi_line_text_is_kept_whole() {
    let multi = page("a", "0", "line one\nline two\n  indented");
    let (stats, parts) = split(&dump(&[multi.clone()]), 1, 1);

    assert_eq!(stats[0].pages.get(&0), Some(&1));
    assert_eq!(pages_in(&parts[0].1), vec![multi]);
}

#[test]
fn the_manifest_lists_one_row_per_part_and_namespace() {
    let pages = vec![page("a", "0", "12345"), page("Template:b", "10", "123")];
    let (stats, _) = split(&dump(&pages), 1, 1);
    let mut out = Vec::new();

    assert!(write_manifest(&stats, &mut out).is_ok());

    assert_eq!(
        String::from_utf8_lossy(&out),
        "part\tkept\tnamespace\tpages\ttext_bytes\n0\tyes\t0\t1\t5\n0\tyes\t10\t1\t3\n"
    );
}

#[test]
fn the_manifest_marks_parts_that_were_not_written() {
    let (stats, _) = split(&dump(&sample_pages(20)), 4, 2);
    let mut out = Vec::new();

    assert!(write_manifest(&stats, &mut out).is_ok());

    let text = String::from_utf8_lossy(&out).to_string();
    assert!(text.lines().any(|line| line.starts_with("1\tno\t")));
    assert!(text.lines().any(|line| line.starts_with("2\tyes\t")));
}

#[test]
fn totals_add_up_every_part() {
    let (stats, _) = split(&dump(&sample_pages(20)), 4, 1);

    assert_eq!(totals(&stats).get(&10), Some(&(4, 400)));
    assert_eq!(totals(&stats).get(&0), Some(&(16, 1600)));
}

#[test]
fn namespace_of_reads_the_namespace_line() {
    assert_eq!(namespace_of(b"    <ns>828</ns>\n"), Some(828));
    assert_eq!(namespace_of(b"    <ns>-1</ns>\n"), Some(-1));
    assert_eq!(namespace_of(b"    <title>x</title>\n"), None);
    assert_eq!(namespace_of(b"    <ns>abc</ns>\n"), None);
}

#[test]
fn text_bytes_of_reads_the_bytes_attribute_of_the_text_line() {
    assert_eq!(
        text_bytes_of(b"      <text bytes=\"5473\" sha1=\"x\" xml:space=\"preserve\">hello"),
        Some(5473)
    );
    assert_eq!(
        text_bytes_of(b"      <text xml:space=\"preserve\">hello"),
        None
    );
    assert_eq!(text_bytes_of(b"      <comment>bytes=\"4\"</comment>"), None);
}

#[test]
fn every_part_range_is_the_exact_page_bytes_of_that_part() {
    let input = dump(&sample_pages(25));
    let (stats, parts) = split(&input, 4, 1);

    for (index, part) in stats.iter().enumerate() {
        let range = &input[part.offset as usize..(part.offset + part.length) as usize];
        let written = parts
            .iter()
            .find(|(written_index, _)| *written_index == index)
            .map(|(_, text)| text.clone())
            .unwrap_or_default();

        assert_eq!(format!("{HEADER}{range}</mediawiki>\n"), written);
    }
}

#[test]
fn the_first_range_starts_right_after_the_header() {
    let input = dump(&sample_pages(10));
    let (stats, _) = split(&input, 3, 1);

    assert_eq!(stats[0].offset, HEADER.len() as u64);
}

#[test]
fn the_ranges_are_contiguous_and_cover_every_page() {
    let input = dump(&sample_pages(30));
    let (stats, _) = split(&input, 5, 1);

    for pair in stats.windows(2) {
        assert_eq!(pair[0].offset + pair[0].length, pair[1].offset);
    }
    let last = &stats[stats.len() - 1];
    assert_eq!(
        last.offset + last.length,
        (input.len() - "</mediawiki>\n".len()) as u64
    );
}

#[test]
fn ranges_are_recorded_for_parts_that_are_not_written() {
    let input = dump(&sample_pages(40));
    let (stats, _) = split(&input, 8, 3);

    assert!(stats.iter().all(|part| part.length > 0));
    assert!(stats.windows(2).all(|pair| pair[0].offset < pair[1].offset));
}

#[test]
fn the_ranges_file_lists_only_the_kept_parts() {
    let (stats, _) = split(&dump(&sample_pages(40)), 8, 3);
    let mut out = Vec::new();

    assert!(write_ranges(&stats, &mut out).is_ok());

    let text = String::from_utf8_lossy(&out).to_string();
    let parts = text
        .lines()
        .skip(1)
        .filter_map(|line| line.split('\t').next())
        .collect::<Vec<_>>();
    assert_eq!(text.lines().next(), Some("part\toffset\tlength\tpages"));
    assert_eq!(parts, vec!["0", "3", "6"]);
}

#[test]
fn the_ranges_file_gives_each_part_its_offset_length_and_page_count() {
    let input = dump(&sample_pages(6));
    let (stats, _) = split(&input, 1, 1);
    let mut out = Vec::new();

    assert!(write_ranges(&stats, &mut out).is_ok());

    assert_eq!(
        String::from_utf8_lossy(&out),
        format!(
            "part\toffset\tlength\tpages\n0\t{}\t{}\t6\n",
            HEADER.len(),
            stats[0].length
        )
    );
}
