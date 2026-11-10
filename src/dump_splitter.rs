use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
};

const FOOTER: &[u8] = b"</mediawiki>\n";
const PAGE_OPEN: &[u8] = b"<page>";
const PAGE_CLOSE: &[u8] = b"</page>";
const NAMESPACE_OPEN: &[u8] = b"<ns>";
const TEXT_OPEN: &[u8] = b"<text ";
const BYTES_ATTRIBUTE: &[u8] = b"bytes=\"";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PartStats {
    pub kept: bool,
    pub offset: u64,
    pub length: u64,
    pub pages: BTreeMap<i32, u64>,
    pub text_bytes: BTreeMap<i32, u64>,
}

pub type PartOpener<'a> = dyn FnMut(usize) -> io::Result<Box<dyn Write>> + 'a;

fn namespace_of(line: &[u8]) -> Option<i32> {
    let rest = line.trim_ascii_start().strip_prefix(NAMESPACE_OPEN)?;
    let end = rest.iter().position(|byte| *byte == b'<')?;

    std::str::from_utf8(&rest[..end]).ok()?.trim().parse().ok()
}

fn text_bytes_of(line: &[u8]) -> Option<u64> {
    let rest = line.trim_ascii_start().strip_prefix(TEXT_OPEN)?;
    let tag_end = rest
        .iter()
        .position(|byte| *byte == b'>')
        .unwrap_or(rest.len());
    let tag = &rest[..tag_end];
    let start = tag
        .windows(BYTES_ATTRIBUTE.len())
        .position(|window| window == BYTES_ATTRIBUTE)?
        + BYTES_ATTRIBUTE.len();
    let length = tag[start..].iter().position(|byte| *byte == b'"')?;

    std::str::from_utf8(&tag[start..start + length])
        .ok()?
        .parse()
        .ok()
}

fn page_figures(page: &[u8]) -> (Option<i32>, u64) {
    let mut namespace = None;
    let mut text_bytes = 0;

    for line in page.split_inclusive(|byte| *byte == b'\n') {
        if namespace.is_none() {
            namespace = namespace_of(line);
        }
        text_bytes += text_bytes_of(line).unwrap_or(0);
    }

    (namespace, text_bytes)
}

fn finish_part(writer: &mut Option<Box<dyn Write>>) -> io::Result<()> {
    if let Some(mut open) = writer.take() {
        open.write_all(FOOTER)?;
        open.flush()?;
    }

    Ok(())
}

fn unexpected_end() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "the dump ended in the middle of a page",
    )
}

fn read_line<R: BufRead>(
    input: &mut R,
    line: &mut Vec<u8>,
    position: &mut u64,
) -> io::Result<bool> {
    line.clear();
    let read = input.read_until(b'\n', line)?;
    *position += read as u64;

    Ok(read > 0)
}

fn read_page<R: BufRead>(
    input: &mut R,
    line: &mut Vec<u8>,
    page: &mut Vec<u8>,
    position: &mut u64,
) -> io::Result<()> {
    page.clear();
    page.extend_from_slice(line);

    loop {
        if !read_line(input, line, position)? {
            return Err(unexpected_end());
        }
        page.extend_from_slice(line);
        if line.trim_ascii() == PAGE_CLOSE {
            return Ok(());
        }
    }
}

fn seek_page_open<R: BufRead>(
    input: &mut R,
    line: &mut Vec<u8>,
    position: &mut u64,
) -> io::Result<bool> {
    while read_line(input, line, position)? {
        if line.trim_ascii() == PAGE_OPEN {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn split_dump<R: BufRead>(
    mut input: R,
    total_bytes: u64,
    parts: usize,
    keep_every: usize,
    open: &mut PartOpener<'_>,
) -> io::Result<Vec<PartStats>> {
    let parts = parts.max(1);
    let keep_every = keep_every.max(1);
    let mut line = Vec::new();
    let mut header = Vec::new();
    let mut position = 0u64;

    loop {
        line.clear();
        let read = input.read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(Vec::new());
        }
        position += read as u64;
        if line.trim_ascii() == PAGE_OPEN {
            break;
        }
        header.extend_from_slice(&line);
    }

    let mut page_start = position - line.len() as u64;

    let mut stats: Vec<PartStats> = Vec::new();
    let mut writer: Option<Box<dyn Write>> = None;
    let mut part_bytes = 0u64;
    let mut consumed = 0u64;
    let mut target = 0u64;
    let mut page = Vec::new();

    loop {
        read_page(&mut input, &mut line, &mut page, &mut position)?;

        if stats.is_empty() || (part_bytes >= target && stats.len() < parts) {
            finish_part(&mut writer)?;
            let index = stats.len();
            let kept = index % keep_every == 0;
            stats.push(PartStats {
                kept,
                offset: page_start,
                ..PartStats::default()
            });
            consumed += part_bytes;
            part_bytes = 0;
            target = (total_bytes.saturating_sub(consumed) / (parts - index) as u64).max(1);

            if kept {
                let mut created = open(index)?;
                created.write_all(&header)?;
                writer = Some(created);
            }
        }

        if let Some(open_part) = writer.as_mut() {
            open_part.write_all(&page)?;
        }

        let (namespace, text_bytes) = page_figures(&page);
        if let Some(current) = stats.last_mut() {
            current.length = position - current.offset;
            if let Some(namespace) = namespace {
                *current.pages.entry(namespace).or_default() += 1;
                *current.text_bytes.entry(namespace).or_default() += text_bytes;
            }
        }
        part_bytes += page.len() as u64;

        if !seek_page_open(&mut input, &mut line, &mut position)? {
            break;
        }
        page_start = position - line.len() as u64;
    }

    finish_part(&mut writer)?;

    Ok(stats)
}

pub fn write_manifest(stats: &[PartStats], out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "part\tkept\tnamespace\tpages\ttext_bytes")?;

    for (index, part) in stats.iter().enumerate() {
        for (namespace, pages) in &part.pages {
            writeln!(
                out,
                "{index}\t{kept}\t{namespace}\t{pages}\t{text_bytes}",
                kept = if part.kept { "yes" } else { "no" },
                text_bytes = part.text_bytes.get(namespace).copied().unwrap_or(0),
            )?;
        }
    }

    Ok(())
}

pub fn write_ranges(stats: &[PartStats], out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "part\toffset\tlength\tpages")?;

    for (index, part) in stats.iter().enumerate().filter(|(_, part)| part.kept) {
        writeln!(
            out,
            "{index}\t{}\t{}\t{}",
            part.offset,
            part.length,
            part.pages.values().sum::<u64>()
        )?;
    }

    Ok(())
}

pub fn totals(stats: &[PartStats]) -> BTreeMap<i32, (u64, u64)> {
    let mut totals: BTreeMap<i32, (u64, u64)> = BTreeMap::new();

    for part in stats {
        for (namespace, pages) in &part.pages {
            let entry = totals.entry(*namespace).or_default();
            entry.0 += pages;
            entry.1 += part.text_bytes.get(namespace).copied().unwrap_or(0);
        }
    }

    totals
}

#[cfg(test)]
mod tests;
