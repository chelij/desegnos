//! Conflict-free three-way merges of page and document source text.
//!
//! A merge is only used when neither side changed the same text; the caller turns any conflict
//! into its usual stale result. Sources are never returned with conflict markers.

use crate::{Error, Result, SiteContent};
use std::collections::HashMap;

const CONFLICT: &str = "You and the AI changed the same part of this source.";

/// Merge one text with a common base, keeping both sides' changes.
pub fn merge_text(base: &str, ours: &str, theirs: &str) -> Result<String> {
    if ours == theirs || base == theirs {
        return Ok(ours.to_string());
    }
    if base == ours {
        return Ok(theirs.to_string());
    }
    if let Ok(merged) = diffy::merge(base, ours, theirs) {
        return Ok(merged);
    }
    // One AI-written HTML line can run for thousands of characters, so any two changes in it
    // would otherwise conflict. Retry at tag, declaration and rule boundaries and rejoin the
    // segments exactly.
    merge_segments(base, ours, theirs)
}

/// Merge every text part of one page or document.
pub fn merge_content(
    base: &SiteContent,
    ours: &SiteContent,
    theirs: &SiteContent,
) -> Result<SiteContent> {
    Ok(SiteContent {
        html: merge_text(&base.html, &ours.html, &theirs.html)?,
        css: merge_text(&base.css, &ours.css, &theirs.css)?,
        javascript: merge_text(&base.javascript, &ours.javascript, &theirs.javascript)?,
    })
}

/// One side's replacement of the base segments in `start .. end`, empty segments meaning an
/// insertion.
struct Change {
    start: usize,
    end: usize,
    segments: Vec<String>,
}

fn segments(text: &str) -> Vec<&str> {
    text.split_inclusive(['\n', '>', ';', '}']).collect()
}

fn merge_segments(base: &str, ours: &str, theirs: &str) -> Result<String> {
    let base = segments(base);
    let ours = segments(ours);
    let theirs = segments(theirs);
    let our_changes = changes(&base, &ours);
    let their_changes = changes(&base, &theirs);
    let mut merged = Vec::with_capacity(base.len());
    let (mut our_next, mut their_next, mut position) = (0, 0, 0);
    while our_next < our_changes.len() || their_next < their_changes.len() {
        let next = [
            our_changes.get(our_next).map_or(usize::MAX, |c| c.start),
            their_changes
                .get(their_next)
                .map_or(usize::MAX, |c| c.start),
        ]
        .into_iter()
        .min()
        .expect("a change");
        // A change that starts inside a change already kept overlaps it.
        if next < position {
            return Err(Error(CONFLICT.into()));
        }
        merged.extend(
            base[position..next]
                .iter()
                .map(|segment| segment.to_string()),
        );
        position = next;
        match (
            our_changes.get(our_next).filter(|c| c.start == next),
            their_changes.get(their_next).filter(|c| c.start == next),
        ) {
            (Some(change), None) => {
                keep(&mut merged, change);
                position = change.end;
                our_next += 1;
            }
            (None, Some(change)) => {
                keep(&mut merged, change);
                position = change.end;
                their_next += 1;
            }
            (Some(our_change), Some(their_change)) => {
                if our_change.end == their_change.end {
                    if our_change.segments != their_change.segments {
                        return Err(Error(CONFLICT.into()));
                    }
                    keep(&mut merged, our_change);
                    position = our_change.end;
                } else if our_change.end == next {
                    keep(&mut merged, our_change);
                    keep(&mut merged, their_change);
                    position = their_change.end;
                } else if their_change.end == next {
                    keep(&mut merged, their_change);
                    keep(&mut merged, our_change);
                    position = our_change.end;
                } else {
                    return Err(Error(CONFLICT.into()));
                }
                our_next += 1;
                their_next += 1;
            }
            (None, None) => break,
        }
    }
    merged.extend(base[position..].iter().map(|segment| segment.to_string()));
    Ok(merged.concat())
}

fn keep(merged: &mut Vec<String>, change: &Change) {
    merged.extend(change.segments.iter().cloned());
}

/// Every range of `base` that `other` replaced, in order.
fn changes(base: &[&str], other: &[&str]) -> Vec<Change> {
    let matches = matches(base, other);
    let mut changes = Vec::new();
    let (mut base_index, mut other_index) = (0, 0);
    for (matched_base, matched_other) in matches
        .into_iter()
        .chain(std::iter::once((base.len(), other.len())))
    {
        if matched_base > base_index || matched_other > other_index {
            changes.push(Change {
                start: base_index,
                end: matched_base,
                segments: other[other_index..matched_other]
                    .iter()
                    .map(|segment| segment.to_string())
                    .collect(),
            });
        }
        base_index = matched_base + 1;
        other_index = matched_other + 1;
    }
    changes
}

/// Segments both sides kept, as increasing index pairs. Unique segments anchor the search first,
/// so repeated tags and whitespace never pull a change into a neighbour's text.
fn matches(base: &[&str], other: &[&str]) -> Vec<(usize, usize)> {
    enum Task {
        Segment(usize, usize),
        Gap(usize, usize, usize, usize),
    }
    let mut matches = Vec::new();
    let mut tasks = vec![Task::Gap(0, base.len(), 0, other.len())];
    while let Some(task) = tasks.pop() {
        let (mut start, gap_end, mut other_start, other_gap_end) = match task {
            Task::Segment(base_index, other_index) => {
                matches.push((base_index, other_index));
                continue;
            }
            Task::Gap(start, end, other_start, other_end) => (start, end, other_start, other_end),
        };
        while start < gap_end && other_start < other_gap_end && base[start] == other[other_start] {
            matches.push((start, other_start));
            start += 1;
            other_start += 1;
        }
        let (mut end, mut other_end) = (gap_end, other_gap_end);
        while end > start && other_end > other_start && base[end - 1] == other[other_end - 1] {
            end -= 1;
            other_end -= 1;
        }
        // Trailing equal segments follow every anchor this gap still contains.
        for offset in (0..gap_end - end).rev() {
            tasks.push(Task::Segment(end + offset, other_end + offset));
        }
        let anchors = anchors(base, start, end, other, other_start, other_end);
        if anchors.is_empty() {
            continue;
        }
        let mut sequence = Vec::new();
        let (mut base_index, mut other_index) = (start, other_start);
        for (anchor_base, anchor_other) in &anchors {
            sequence.push(Task::Gap(
                base_index,
                *anchor_base,
                other_index,
                *anchor_other,
            ));
            sequence.push(Task::Segment(*anchor_base, *anchor_other));
            base_index = *anchor_base + 1;
            other_index = *anchor_other + 1;
        }
        sequence.push(Task::Gap(base_index, end, other_index, other_end));
        for task in sequence.into_iter().rev() {
            tasks.push(task);
        }
    }
    matches
}

/// Segments appearing exactly once on each side, thinned to the longest increasing run.
fn anchors(
    base: &[&str],
    base_start: usize,
    base_end: usize,
    other: &[&str],
    other_start: usize,
    other_end: usize,
) -> Vec<(usize, usize)> {
    let mut seen: HashMap<&str, (usize, usize, usize, usize)> = HashMap::new();
    let base_range = base.iter().enumerate().take(base_end).skip(base_start);
    for (index, segment) in base_range {
        let entry = seen.entry(*segment).or_default();
        entry.0 += 1;
        entry.1 = index;
    }
    let other_range = other.iter().enumerate().take(other_end).skip(other_start);
    for (index, segment) in other_range {
        let entry = seen.entry(*segment).or_default();
        entry.2 += 1;
        entry.3 = index;
    }
    let mut candidates: Vec<(usize, usize)> = seen
        .values()
        .filter(|(base_count, _, other_count, _)| *base_count == 1 && *other_count == 1)
        .map(|(_, base_index, _, other_index)| (*base_index, *other_index))
        .collect();
    candidates.sort_unstable();
    // Patience sorting keeps the longest run whose other indices also increase.
    let mut pile_tops: Vec<(usize, usize)> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; candidates.len()];
    for (index, (_, other_index)) in candidates.iter().enumerate() {
        let pile = pile_tops.partition_point(|(top, _)| top < other_index);
        if pile > 0 {
            previous[index] = Some(pile_tops[pile - 1].1);
        }
        if pile == pile_tops.len() {
            pile_tops.push((*other_index, index));
        } else {
            pile_tops[pile] = (*other_index, index);
        }
    }
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let mut cursor = pile_tops.last().map(|(_, index)| *index);
    while let Some(index) = cursor {
        anchors.push(candidates[index]);
        cursor = previous[index];
    }
    anchors.reverse();
    anchors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(html: &str) -> SiteContent {
        SiteContent {
            html: html.into(),
            css: String::new(),
            javascript: String::new(),
        }
    }

    #[test]
    fn changes_in_different_lines_are_both_kept() {
        let base = "<main>\n  <img src=\"a.png\">\n  <button>Go</button>\n</main>\n";
        let ours = "<main>\n  <img src=\"a.png\" style=\"margin-left:24px\">\n  <button>Go</button>\n</main>\n";
        let theirs =
            "<main>\n  <img src=\"a.png\">\n  <button class=\"primary\">Go now</button>\n</main>\n";
        let merged = merge_text(base, ours, theirs).unwrap();
        assert!(merged.contains("margin-left:24px"));
        assert!(merged.contains("<button class=\"primary\">Go now</button>"));
        assert!(!merged.contains("<<<<<<<"));
    }

    #[test]
    fn neighbouring_lines_merge() {
        let merged = merge_text("a\nb\nc\n", "A\nb\nc\n", "a\nB\nc\n").unwrap();
        assert_eq!(merged, "A\nB\nc\n");
    }

    #[test]
    fn the_same_line_from_both_sides_conflicts() {
        let base = "<p>Hello</p>\n";
        assert!(merge_text(base, "<p>Hello there</p>\n", "<p>Hi</p>\n").is_err());
    }

    #[test]
    fn a_long_single_line_merges_at_tag_boundaries() {
        let base = "<main><img src=\"a.png\" alt=\"Me\"><button>Go</button><p>Footer</p></main>";
        let ours = "<main><img src=\"a.png\" alt=\"Me\" style=\"margin-left:24px\"><button>Go</button><p>Footer</p></main>";
        let theirs = "<main><img src=\"a.png\" alt=\"Me\"><button class=\"primary\">Go now</button><p>Footer</p></main>";
        let merged = merge_text(base, ours, theirs).unwrap();
        assert!(merged.contains("style=\"margin-left:24px\""));
        assert!(merged.contains("<button class=\"primary\">Go now</button>"));
    }

    #[test]
    fn a_changed_line_keeps_a_neighbouring_insertion() {
        let base = "<main>\n  <h1>Studio</h1>\n</main>";
        let ours = "<!DOCTYPE html><html><body><main data-desegnos-id=\"1\">\n  <h1 data-desegnos-id=\"2\">Studio</h1>\n</main></body></html>";
        let theirs = "<main>\n  <h1>Studio</h1>\n  <section data-desegnos-region=\"r\">New</section>\n</main>";
        let merged = merge_text(base, ours, theirs).unwrap();
        assert!(merged.contains("data-desegnos-id=\"1\""));
        assert!(merged.contains("<section data-desegnos-region=\"r\">New</section>"));
        assert!(merged.ends_with("</main></body></html>"));
    }

    #[test]
    fn long_single_line_conflicts_stay_conflicts() {
        let base = "<main><button>Go</button></main>";
        assert!(
            merge_text(
                base,
                "<main><button>Go now</button></main>",
                "<main><button>Stop</button></main>"
            )
            .is_err()
        );
    }

    #[test]
    fn identical_edits_and_unchanged_sides_need_no_diff() {
        assert_eq!(merge_text("a\n", "b\n", "b\n").unwrap(), "b\n");
        assert_eq!(merge_text("a\n", "a\n", "c\n").unwrap(), "c\n");
        assert_eq!(merge_text("a\n", "b\n", "a\n").unwrap(), "b\n");
    }

    #[test]
    fn every_text_part_is_merged() {
        let base = source("<p>One</p>");
        let ours = SiteContent {
            html: "<p>One</p>".into(),
            css: "p{color:red}".into(),
            javascript: String::new(),
        };
        let theirs = SiteContent {
            html: "<p>Two</p>".into(),
            css: String::new(),
            javascript: "document.body.dataset.ready='1'".into(),
        };
        let merged = merge_content(&base, &ours, &theirs).unwrap();
        assert_eq!(merged.html, "<p>Two</p>");
        assert_eq!(merged.css, "p{color:red}");
        assert_eq!(merged.javascript, "document.body.dataset.ready='1'");
    }
}
