//! Text-only project-local history, derived from frozen job requests and outcomes.
use crate::{
    Project, Rect, RegionOutcome, RegionStatus, RequirementsSnapshot, Result, digest, ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

const HEADING: &str = "\n## Earlier requests for these areas (context only — the current requests are the authority)\n\n```jsonl\n";
pub const REQUEST_HISTORY_CHARS: usize = 3000;
const RETRY_HEADING: &str = "\n## Previous validation rejections for retried areas\n\nThese are app checks, not user rejections. Correct each listed area while preserving the current scope.\n\n```jsonl\n";
const REJECTION_CHARS: usize = 600;
const RETRY_HISTORY_CHARS: usize = crate::MAX_GENERATION_REGIONS * 4500;
const CHANGES_HEADING: &str = "\n## Changes you made since the last result\n\nPreserve these edits in the current page source, including text, images, removals, styles and layout, unless the current request explicitly changes them. Component names below identify changed source, not permission to expand scope.\n\n```jsonl\n";

/// Small fingerprints keep historical source out of the bounded request-history index.
pub fn source_fingerprints(content: Option<&crate::SiteContent>) -> BTreeMap<String, String> {
    let mut hashes = BTreeMap::new();
    if let Some(content) = content {
        for (name, source) in [
            ("html", &content.html),
            ("css", &content.css),
            ("javascript", &content.javascript),
        ] {
            hashes.insert(name.into(), digest(source.as_bytes()));
        }
    }
    hashes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EarlierRequest {
    pub project_id: Uuid,
    pub job_id: Uuid,
    pub page_id: Uuid,
    pub region_id: Uuid,
    pub rect: Rect,
    pub whole_page: bool,
    pub requested_at: i64,
    pub request: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_rejection: Option<String>,
    pub summary: String,
    /// The complete original scope of the job-wide summary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_targets: Vec<(Uuid, Uuid)>,
    /// Exact stored source after application, or base source for unapplied results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprints: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_source_fingerprints: Option<BTreeMap<String, String>>,
    /// Format 23: fingerprints of every editable document this job covered, by document key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub document_fingerprints: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub base_document_fingerprints: BTreeMap<String, String>,
}

/// Read request blocks and source hashes, never attachments, source text, or prior context.
pub fn earlier_requests(
    snapshot: &RequirementsSnapshot,
    requested_at: i64,
    outcome: &str,
    summary: &str,
    regions: &[RegionOutcome],
) -> Vec<EarlierRequest> {
    let mut requests = Vec::new();
    let mut sources = BTreeMap::new();
    let mut fence = None;
    let mut block = String::new();
    for line in snapshot.markdown.lines() {
        if let Some(end) = &fence {
            if line == end {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&block) {
                    if let Some(page_id) = value
                        .get("pageId")
                        .and_then(|id| serde_json::from_value::<Uuid>(id.clone()).ok())
                    {
                        if let Some(hashes) = value.get("sourceHashes").and_then(|hashes| {
                            serde_json::from_value::<BTreeMap<String, String>>(hashes.clone()).ok()
                        }) {
                            sources.insert(page_id, hashes);
                        } else if value.get("content").is_some_and(serde_json::Value::is_null) {
                            sources.insert(page_id, BTreeMap::new());
                        }
                    }
                    let parse = || {
                        let page_id = serde_json::from_value(value.get("pageId")?.clone()).ok()?;
                        let region_id =
                            serde_json::from_value(value.get("regionId")?.clone()).ok()?;
                        let rect =
                            serde_json::from_value(value.get("placementHint")?.clone()).ok()?;
                        let request = value.get("prompt")?.as_str()?;
                        let question = regions.iter().find(|r| {
                            r.region_id == region_id && r.status == RegionStatus::Conflict
                        });
                        let outcome =
                            if !matches!(outcome, "undone" | "rejected by user" | "failed") {
                                question
                                    .map(|r| {
                                        format!("needs details: {}", clip(&r.explanation, 300))
                                    })
                                    .unwrap_or_else(|| outcome.to_owned())
                            } else {
                                outcome.to_owned()
                            };
                        Some(EarlierRequest {
                            project_id: snapshot.project_id,
                            job_id: snapshot.job_id,
                            page_id,
                            region_id,
                            rect,
                            whole_page: value.get("scope")?.as_str()? == "page",
                            requested_at,
                            request: clip(request, 300),
                            outcome,
                            validation_rejection: None,
                            summary: clip(
                                &summary.split_whitespace().collect::<Vec<_>>().join(" "),
                                200,
                            ),
                            summary_targets: Vec::new(),
                            source_fingerprints: None,
                            base_source_fingerprints: None,
                            document_fingerprints: BTreeMap::new(),
                            base_document_fingerprints: BTreeMap::new(),
                        })
                    };
                    if let Some(request) = parse() {
                        requests.push(request);
                    }
                }
                fence = None;
            } else {
                block.push_str(line);
                block.push('\n');
            }
        } else if let Some(start) = line.strip_suffix("json")
            && start.len() >= 3
            && start.bytes().all(|b| b == b'`')
        {
            fence = Some(start.to_owned());
            block.clear();
        }
    }
    let summary_targets = snapshot.target_ids().unwrap_or_default();
    // Every editable document's frozen fingerprint, including pages and site files the request
    // was given beyond its framed document.
    let documents: BTreeMap<String, String> = snapshot
        .dependencies
        .iter()
        .filter_map(|(key, hash)| {
            key.strip_prefix("document:")
                .map(|key| (key.to_string(), hash.clone()))
        })
        .collect();
    for request in &mut requests {
        request.summary_targets = summary_targets.clone();
        request.source_fingerprints = sources.get(&request.page_id).cloned();
        request.base_source_fingerprints = request.source_fingerprints.clone();
        request.document_fingerprints = documents.clone();
        request.base_document_fingerprints = documents.clone();
    }
    requests
}

/// Keep only the newest five records for each historical area in the local index.
pub fn compact_request_history(records: &mut Vec<EarlierRequest>) {
    records.sort_by_key(|r| {
        (
            std::cmp::Reverse(r.requested_at),
            std::cmp::Reverse(r.job_id),
        )
    });
    let mut counts = std::collections::BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    records.retain(|r| {
        let count = counts
            .entry((r.project_id, r.page_id, r.region_id))
            .or_insert(0);
        if *count >= 5 || !seen.insert((r.project_id, r.job_id, r.region_id)) {
            return false;
        }
        *count += 1;
        true
    });
}

fn clip(value: &str, max: usize) -> String {
    let mut text: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        text.pop();
        text.push('…');
    }
    text
}
/// Preserve the validator's wording, including punctuation, up to the per-area limit.
pub fn record_validation_rejection(request: &mut EarlierRequest, reason: &str) {
    request.outcome = "rejected by the app's checks".into();
    request.validation_rejection = Some(clip(reason, REJECTION_CHARS));
}

fn prevention_rule(reason: &str) -> &'static str {
    if reason.contains("Region script") {
        "Use one inert inline script per request UUID, directly inside its uniquely identified owning root; preserve other requests' scripts."
    } else if reason.contains("editor")
        || reason.contains("element ID")
        || reason.contains("data-desegnos-id")
    {
        "Preserve every existing editor ID on its original element; leave data-desegnos-id off new elements and never copy an existing ID."
    } else if reason.contains("CSS") || reason.contains("stylesheet") {
        "Preserve existing shared CSS and add only rules scoped to this area's root or selected elements."
    } else if reason.contains("JavaScript") || reason.contains("shared script") {
        "Preserve shared JavaScript byte-for-byte; put local behavior in one inert region script directly inside its owning root."
    } else if reason.contains("root") {
        "Return exactly one uniquely identified root for this request and preserve existing selected roots and their IDs."
    } else {
        "Change only this area's authorized roots; preserve unrelated markup, existing IDs, shared code and required source fields."
    }
}

fn nearby(a: Rect, b: Rect) -> bool {
    const GAP: f64 = 32.;
    a.x <= b.x + b.width + GAP
        && b.x <= a.x + a.width + GAP
        && a.y <= b.y + b.height + GAP
        && b.y <= a.y + a.height + GAP
}
fn relative(now: i64, then: i64) -> String {
    let seconds = now.saturating_sub(then).max(0);
    let (n, unit) = if seconds < 60 {
        return "just now".into();
    } else if seconds < 3600 {
        (seconds / 60, "minute")
    } else if seconds < 86400 {
        (seconds / 3600, "hour")
    } else {
        (seconds / 86400, "day")
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}
impl RequirementsSnapshot {
    pub fn with_earlier_requests(
        mut self,
        project: &Project,
        records: &[EarlierRequest],
        now: i64,
    ) -> Result<Self> {
        ensure(
            self.project_id == project.id,
            "History belongs to another project.",
        )?;

        let ids = self.target_ids()?;
        let targets: Vec<_> = project
            .pages
            .iter()
            .flat_map(|page| {
                page.regions
                    .iter()
                    .filter(|r| ids.contains(&(page.id, r.id)))
                    .map(move |r| (page, r))
            })
            .collect();
        let mut records: Vec<_> = records
            .iter()
            .filter(|r| r.project_id == project.id && r.job_id != self.job_id)
            .collect();
        records.sort_by_key(|r| {
            (
                std::cmp::Reverse(r.requested_at),
                std::cmp::Reverse(r.job_id),
                r.region_id,
            )
        });
        // Exact area identity, never proximity: a nearby area's failure is not this retry's.
        // Reserve separate bounded space so five older records cannot crowd out a rejection.
        let mut retry = String::new();
        for (page, area) in &targets {
            if let Some(reason) = records
                .iter()
                .find(|r| r.page_id == page.id && r.region_id == area.id)
                .and_then(|r| r.validation_rejection.as_deref())
            {
                let reason = clip(reason, REJECTION_CHARS);
                let instruction = format!(
                    "Your previous attempt for this area was rejected because: {reason} Avoid this by: {}",
                    prevention_rule(&reason)
                );
                retry.push_str(&format!(
                    "{}\n",
                    serde_json::json!({"page":page.id,"area":area.id,"instruction":instruction})
                ));
            }
        }
        let retry = if retry.is_empty() {
            retry
        } else {
            format!("{RETRY_HEADING}{retry}```\n")
        };
        ensure(
            retry.chars().count() <= RETRY_HISTORY_CHARS,
            "The retry history exceeds its limit.",
        )?;
        let mut changes = String::new();
        let mut compared = std::collections::BTreeSet::new();
        // Compare only the newest relevant result for each page. A later result on a
        // different page must not hide edits on this page. Undone results use their base.
        for r in &records {
            if !targets.iter().any(|(p, area)| {
                r.page_id == p.id
                    && (r.region_id == area.id
                        || area.is_whole_page(p)
                        || r.whole_page
                        || nearby(area.rect, r.rect))
            }) {
                continue;
            }
            if !compared.insert(r.page_id) {
                continue;
            }
            let baseline = if r.outcome.starts_with("undone") {
                &r.base_source_fingerprints
            } else {
                &r.source_fingerprints
            };
            let Some(before) = baseline else { continue };
            let page = project.page(r.page_id)?;
            let current = source_fingerprints(page.content.as_ref());
            let changed: Vec<_> = ["html", "css", "javascript"]
                .into_iter()
                .filter(|name| before.get(*name) != current.get(*name))
                .collect();
            // Included documents changed by hand are protected too, not only the host page.
            let document_baseline = if r.outcome.starts_with("undone") {
                &r.base_document_fingerprints
            } else {
                &r.document_fingerprints
            };
            let changed_documents: Vec<_> = document_baseline
                .iter()
                .filter(|(key, hash)| {
                    crate::DocumentTarget::from_key(key)
                        .ok()
                        .and_then(|document| project.document_hash(&document).ok())
                        .as_deref()
                        != Some(hash.as_str())
                })
                .map(|(key, _)| key)
                .collect();
            if changed.is_empty() && changed_documents.is_empty() {
                continue;
            }
            let mut line_value = serde_json::json!({"page":r.page_id,"changed":changed});
            if !changed_documents.is_empty() {
                line_value["documents"] = serde_json::json!(changed_documents);
            }
            let line = format!("{}\n", line_value);
            if CHANGES_HEADING.chars().count() + changes.chars().count() + line.chars().count() + 4
                > 1200
            {
                break;
            }
            changes.push_str(&line);
        }
        let changes = if changes.is_empty() {
            changes
        } else {
            format!("{CHANGES_HEADING}{changes}```\n")
        };
        let mut counts = vec![0; targets.len()];
        let mut seen = std::collections::BTreeSet::new();
        let mut section = String::new();
        for r in records {
            let matched: Vec<_> = targets
                .iter()
                .enumerate()
                .filter(|(i, (p, area))| {
                    counts[*i] < 5
                        && r.page_id == p.id
                        && (r.region_id == area.id
                            || area.is_whole_page(p)
                            || r.whole_page
                            || nearby(area.rect, r.rect))
                })
                .map(|(i, _)| i)
                .collect();
            if matched.is_empty() || !seen.insert((r.job_id, r.region_id)) {
                continue;
            }
            // JSON quoting keeps exact user text as data, including line breaks and fences.
            // A per-area record still carries a job-wide summary. Never expose it to a
            // narrower request or one whose summary scope is not established.
            let summary = (!r.summary_targets.is_empty()
                && r.summary_targets.iter().all(|target| ids.contains(target)))
            .then(|| clip(&r.summary, 200));
            let entry = serde_json::json!({"page":r.page_id,"area":r.region_id,"when":relative(now,r.requested_at),
                "request":clip(&r.request,300),"outcome":clip(&r.outcome,320),"summary":summary,
                "validationReason":r.validation_rejection.as_deref().map(|reason| clip(reason, REJECTION_CHARS))});
            let line = format!("{}\n", entry);
            if changes.chars().count()
                + HEADING.chars().count()
                + section.chars().count()
                + line.chars().count()
                + 4
                > REQUEST_HISTORY_CHARS
            {
                break;
            }
            section.push_str(&line);
            for i in matched {
                counts[i] += 1;
            }
        }
        self.set_request_history(if section.is_empty() {
            format!("{retry}{changes}")
        } else {
            format!("{retry}{changes}{HEADING}{section}```\n")
        })?;
        Ok(self)
    }

    pub(crate) fn set_request_history(&mut self, section: String) -> Result<()> {
        ensure(
            section.is_empty()
                || ((section.starts_with(HEADING)
                    || section.starts_with(CHANGES_HEADING)
                    || section.starts_with(RETRY_HEADING))
                    && section.chars().count() <= REQUEST_HISTORY_CHARS + RETRY_HISTORY_CHARS),
            "The request history is invalid.",
        )?;
        ensure(
            self.request_history.is_empty(),
            "Request history is already frozen.",
        )?;
        if !section.is_empty() {
            let position = self
                .markdown
                .find("\n## Site routes\n")
                .ok_or_else(|| crate::Error("Missing site routes.".into()))?;
            self.markdown.insert_str(position, &section);
            ensure(
                self.markdown.len() <= crate::MAX_REQUIREMENTS_BYTES,
                "The requirements file exceeds its limit.",
            )?;
            self.sha256 = digest(self.markdown.as_bytes());
        }
        self.request_history = section;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Appearance, Command, Viewport};

    fn project() -> Project {
        let mut p = Project::new(
            "History".into(),
            Viewport {
                width: 1440,
                height: 900,
            },
            Appearance::Light,
        )
        .unwrap();
        p = p
            .apply(
                p.revision,
                Command::AddRegion {
                    page_id: p.home_page_id,
                    rect: Rect {
                        x: 100.,
                        y: 100.,
                        width: 200.,
                        height: 200.,
                    },
                },
            )
            .unwrap();
        p.pages[0].regions[0].prompt = "Point the photo at the cursor".into();
        p
    }
    fn record(p: &Project, n: i64) -> EarlierRequest {
        EarlierRequest {
            project_id: p.id,
            job_id: Uuid::from_u128(n as u128 + 1),
            page_id: p.home_page_id,
            region_id: p.pages[0].regions[0].id,
            rect: p.pages[0].regions[0].rect,
            whole_page: false,
            requested_at: n * 3600,
            request: format!("Exact request {n}: café\nplease keep this"),
            outcome: "applied".into(),
            validation_rejection: None,
            summary: format!("Changed the photo {n}."),
            summary_targets: vec![(p.home_page_id, p.pages[0].regions[0].id)],
            source_fingerprints: None,
            base_source_fingerprints: None,
            document_fingerprints: BTreeMap::new(),
            base_document_fingerprints: BTreeMap::new(),
        }
    }
    #[test]
    fn retries_use_only_latest_exact_area_rejection_and_bound_technical_text() {
        let p = project();
        let mut rejected = record(&p, 1);
        record_validation_rejection(
            &mut rejected,
            &format!(
                "Editor element IDs must be valid and unique within the page. {}",
                "é".repeat(4000)
            ),
        );
        assert_eq!(
            rejected
                .validation_rejection
                .as_ref()
                .unwrap()
                .chars()
                .count(),
            REJECTION_CHARS
        );
        let render = |records: &[EarlierRequest]| {
            p.prepare_requirements()
                .unwrap()
                .with_earlier_requests(&p, records, 7200)
                .unwrap()
        };
        let retry = render(&[rejected.clone()]);
        assert!(
            retry
                .request_history
                .contains("Your previous attempt for this area was rejected because:")
        );
        assert!(retry.request_history.contains("Avoid this by:"));
        assert!(retry.request_history.chars().count() <= REQUEST_HISTORY_CHARS + 1200);
        retry.validate_current(&p).unwrap();
        let newer = record(&p, 2);
        assert!(
            !render(&[rejected.clone(), newer])
                .request_history
                .contains(RETRY_HEADING)
        );
        rejected.region_id = Uuid::new_v4();
        assert!(!render(&[rejected]).request_history.contains(RETRY_HEADING));
    }

    #[test]
    fn manual_edits_compare_current_source_and_preserve_snapshot_validation() {
        let mut p = project();
        p.pages[0].content = Some(crate::SiteContent {
            html: "<section><h1>Old text</h1><img src=\"old.png\"><p>Delete me</p></section>"
                .into(),
            css: "section { width: 200px; }".into(),
            javascript: String::new(),
        });
        let mut previous = record(&p, 1);
        previous.source_fingerprints = Some(source_fingerprints(p.pages[0].content.as_ref()));
        let render = |p: &Project, previous: &EarlierRequest| {
            p.prepare_requirements()
                .unwrap()
                .with_earlier_requests(p, std::slice::from_ref(previous), 7200)
                .unwrap()
        };
        assert!(
            !render(&p, &previous)
                .request_history
                .contains(CHANGES_HEADING)
        );
        p.pages[0].content = Some(crate::SiteContent {
            html: "<section><h1>My new text</h1><img src=\"new.png\"></section>".into(),
            css: "section { width: 400px; transform: translate(40px, 20px); color: red; }".into(),
            javascript: String::new(),
        });
        p.revision += 1;
        let snapshot = render(&p, &previous);
        assert!(snapshot.request_history.contains(CHANGES_HEADING));
        assert!(
            snapshot
                .request_history
                .contains("\"changed\":[\"html\",\"css\"]")
        );
        assert!(!snapshot.request_history.contains("javascript"));
        assert!(snapshot.markdown.contains("My new text"));
        assert!(!snapshot.markdown.contains("Delete me"));
        assert!(snapshot.request_history.chars().count() <= REQUEST_HISTORY_CHARS);
        snapshot.validate_current(&p).unwrap();
    }

    #[test]
    fn manual_edits_ignore_other_areas_and_compare_undo_to_base() {
        let p = project();
        let mut previous = record(&p, 1);
        previous.source_fingerprints = Some(source_fingerprints(Some(&crate::SiteContent {
            html: "<p>Rejected result</p>".into(),
            css: String::new(),
            javascript: String::new(),
        })));
        previous.base_source_fingerprints = Some(source_fingerprints(p.pages[0].content.as_ref()));
        previous.outcome = "undone (do not repeat)".into();
        let render = |r: EarlierRequest| {
            p.prepare_requirements()
                .unwrap()
                .with_earlier_requests(&p, &[r], 7200)
                .unwrap()
        };
        assert!(
            !render(previous.clone())
                .request_history
                .contains(CHANGES_HEADING)
        );
        previous.outcome = "applied".into();
        previous.region_id = Uuid::new_v4();
        previous.rect.x = 1000.;
        assert!(!render(previous).request_history.contains(CHANGES_HEADING));
    }

    #[test]
    fn manual_edit_notes_and_history_share_the_existing_budget() {
        let mut p = project();
        let previous = p.clone();
        let mut records = Vec::new();
        for n in 1..30 {
            let mut r = record(&p, n);
            r.source_fingerprints = Some(source_fingerprints(previous.pages[0].content.as_ref()));
            r.request = "あ".repeat(1000);
            records.push(r);
        }
        p.pages[0].content = Some(crate::SiteContent {
            html: "<p>Manual edit</p>".into(),
            css: String::new(),
            javascript: String::new(),
        });
        let snapshot = p
            .prepare_requirements()
            .unwrap()
            .with_earlier_requests(&p, &records, 200000)
            .unwrap();
        assert!(snapshot.request_history.contains(CHANGES_HEADING));
        assert!(snapshot.request_history.chars().count() <= REQUEST_HISTORY_CHARS);
        snapshot.validate_current(&p).unwrap();
    }

    #[test]
    fn request_history_included_newest_first_undone_rejected_and_project_isolated() {
        let p = project();
        let mut entries: Vec<_> = (1..=4).map(|n| record(&p, n)).collect();
        entries[2].outcome = "undone (do not repeat)".into();
        entries[1].outcome = "rejected by user".into();
        let mut foreign = record(&p, 5);
        foreign.project_id = Uuid::new_v4();
        foreign.request = "OTHER PROJECT".into();
        entries.push(foreign);
        let persisted = serde_json::to_string(&entries).unwrap();
        let entries: Vec<EarlierRequest> = serde_json::from_str(&persisted).unwrap();
        let snapshot = p
            .prepare_requirements()
            .unwrap()
            .with_earlier_requests(&p, &entries, 6 * 3600)
            .unwrap();
        assert!(snapshot.request_history.contains("2 hours ago"));
        assert!(snapshot.request_history.contains("undone (do not repeat)"));
        assert!(snapshot.request_history.contains("rejected by user"));
        assert!(snapshot.request_history.contains("café\\nplease keep this"));
        assert!(!snapshot.markdown.contains("OTHER PROJECT"));
        assert!(!snapshot.markdown.contains("Previous generation notes"));
        for n in 1..4 {
            assert!(
                snapshot
                    .request_history
                    .find(&format!("Exact request {}", n + 1))
                    .unwrap()
                    < snapshot
                        .request_history
                        .find(&format!("Exact request {n}"))
                        .unwrap()
            );
        }
        snapshot.validate_current(&p).unwrap();
        // Frozen context survives serialization and does not need history files on retry.
        let saved = serde_json::to_string(&snapshot).unwrap();
        serde_json::from_str::<RequirementsSnapshot>(&saved)
            .unwrap()
            .validate_current(&p)
            .unwrap();
    }

    #[test]
    fn request_history_budget_and_five_per_area_limit() {
        let p = project();
        let mut entries: Vec<_> = (1..=20).map(|n| record(&p, n)).collect();
        let short = p
            .prepare_requirements()
            .unwrap()
            .with_earlier_requests(&p, &entries, 30 * 3600)
            .unwrap();
        assert_eq!(short.request_history.matches("\"request\"").count(), 5);
        for r in &mut entries {
            r.request = "あ".repeat(500);
            r.summary = "é".repeat(500);
        }
        let long = p
            .prepare_requirements()
            .unwrap()
            .with_earlier_requests(&p, &entries, 30 * 3600)
            .unwrap();
        assert!(long.request_history.chars().count() <= REQUEST_HISTORY_CHARS);
        assert!(long.request_history.contains("10 hours ago"));
        assert!(!long.request_history.contains("29 hours ago"));
        assert!(!long.request_history.contains(&"あ".repeat(301)));
        assert!(!long.request_history.contains(&"é".repeat(201)));
    }
    #[test]
    fn request_history_matches_nearby_whole_page_and_excludes_far_or_other_page() {
        let mut p = project();
        let mut near = record(&p, 1);
        near.region_id = Uuid::new_v4();
        near.rect.x = 320.;
        near.request = "NEAR".into();
        let mut far = near.clone();
        far.job_id = Uuid::new_v4();
        far.rect.x = 1200.;
        far.request = "FAR".into();
        let mut other = near.clone();
        other.job_id = Uuid::new_v4();
        other.page_id = Uuid::new_v4();
        other.request = "OTHER PAGE".into();
        let mut whole = far.clone();
        whole.job_id = Uuid::new_v4();
        whole.whole_page = true;
        whole.request = "WHOLE".into();
        let entries = vec![near, far, other, whole];
        let render = |p: &Project| {
            p.prepare_requirements()
                .unwrap()
                .with_earlier_requests(p, &entries, 7200)
                .unwrap()
        };
        let s = render(&p);
        assert!(s.request_history.contains("NEAR"));
        assert!(s.request_history.contains("WHOLE"));
        assert!(!s.request_history.contains("FAR"));
        assert!(!s.request_history.contains("OTHER PAGE"));
        p.pages[0].regions[0].whole_page = true;
        assert!(render(&p).request_history.contains("FAR"));
    }
    #[test]
    fn request_history_backfill_extracts_only_requests_and_conflict_question() {
        let p = project();
        let snapshot = p.prepare_requirements().unwrap();
        let question = RegionOutcome {
            region_id: p.pages[0].regions[0].id,
            status: RegionStatus::Conflict,
            explanation: "Which direction should the photo point?".into(),
        };
        let entries = earlier_requests(
            &snapshot,
            10,
            "drafted",
            "Asked about the direction",
            &[question],
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].request, p.pages[0].regions[0].prompt);
        assert_eq!(
            entries[0].outcome,
            "needs details: Which direction should the photo point?"
        );
        assert_eq!(entries[0].summary, "Asked about the direction");
        let s = snapshot
            .with_earlier_requests(&p, &[record(&p, 1)], 7200)
            .unwrap();
        assert_eq!(earlier_requests(&s, 10, "drafted", "", &[]).len(), 1);
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    #[test]
    fn request_history_index_retains_five_per_area_across_restart() {
        let project_id = Uuid::new_v4();
        let page_id = Uuid::new_v4();
        let mut records = Vec::new();
        for region in 1..=2 {
            for n in 1..=10 {
                records.push(EarlierRequest {
                    project_id,
                    page_id,
                    job_id: Uuid::from_u128(n),
                    region_id: Uuid::from_u128(region),
                    rect: Rect {
                        x: 0.,
                        y: 0.,
                        width: 20.,
                        height: 20.,
                    },
                    whole_page: false,
                    requested_at: n as i64,
                    request: format!("request {n}"),
                    summary: "One-line change".into(),
                    summary_targets: vec![(page_id, Uuid::from_u128(region))],
                    source_fingerprints: None,
                    base_source_fingerprints: None,
                    document_fingerprints: BTreeMap::new(),
                    base_document_fingerprints: BTreeMap::new(),
                    validation_rejection: None,
                    outcome: if n == 10 {
                        "undone (do not repeat)".into()
                    } else {
                        "applied".into()
                    },
                });
            }
        }
        compact_request_history(&mut records);
        assert_eq!(records.len(), 10);
        let bytes = serde_json::to_vec(&records).unwrap();
        let mut restored: Vec<EarlierRequest> = serde_json::from_slice(&bytes).unwrap();
        compact_request_history(&mut restored);
        assert_eq!(bytes, serde_json::to_vec(&restored).unwrap());
        assert_eq!(restored[0].requested_at, 10);
        assert!(restored[0].outcome.starts_with("undone"));
        assert!(restored.iter().all(|r| r.requested_at >= 6));
    }
}
