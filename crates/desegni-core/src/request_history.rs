//! Text-only project-local history, derived from frozen job requests and outcomes.
use crate::{
    Project, Rect, RegionOutcome, RegionStatus, RequirementsSnapshot, Result, digest, ensure,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const HEADING: &str = "\n## Earlier requests for these areas (context only — the current requests are the authority)\n\n```jsonl\n";
pub const REQUEST_HISTORY_CHARS: usize = 3000;

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
    pub summary: String,
    /// The complete original scope of the job-wide summary. Empty for older indexes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_targets: Vec<(Uuid, Uuid)>,
}

/// Read only request data blocks, never attachments, page source, or prior context.
pub fn earlier_requests(
    snapshot: &RequirementsSnapshot,
    requested_at: i64,
    outcome: &str,
    summary: &str,
    regions: &[RegionOutcome],
) -> Vec<EarlierRequest> {
    let mut requests = Vec::new();
    let mut fence = None;
    let mut block = String::new();
    for line in snapshot.markdown.lines() {
        if let Some(end) = &fence {
            if line == end {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&block) {
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
                            whole_page: value.get("scope").map_or_else(
                                || {
                                    value["viewport"]["width"]
                                        .as_f64()
                                        .is_some_and(|w| rect.x <= 0. && rect.width >= w)
                                        && value["viewport"]["height"]
                                            .as_f64()
                                            .is_some_and(|h| rect.y <= 0. && rect.height >= h)
                                },
                                |scope| scope == "page",
                            ),
                            requested_at,
                            request: clip(request, 300),
                            outcome,
                            summary: clip(
                                &summary.split_whitespace().collect::<Vec<_>>().join(" "),
                                200,
                            ),
                            summary_targets: Vec::new(),
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
    for request in &mut requests {
        request.summary_targets = summary_targets.clone();
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
        if self.format_version()? < 19 {
            return Ok(self);
        }
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
            // narrower request, or when an older index cannot establish its scope.
            let summary = (!r.summary_targets.is_empty()
                && r.summary_targets.iter().all(|target| ids.contains(target)))
            .then(|| clip(&r.summary, 200));
            let entry = serde_json::json!({"page":r.page_id,"area":r.region_id,"when":relative(now,r.requested_at),
                "request":clip(&r.request,300),"outcome":clip(&r.outcome,320),"summary":summary});
            let line = format!("{}\n", entry);
            if HEADING.chars().count() + section.chars().count() + line.chars().count() + 4
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
            section
        } else {
            format!("{HEADING}{section}```\n")
        })?;
        Ok(self)
    }

    pub(crate) fn set_request_history(&mut self, section: String) -> Result<()> {
        ensure(
            section.is_empty()
                || (self.format_version()? >= 19
                    && section.starts_with(HEADING)
                    && section.chars().count() <= REQUEST_HISTORY_CHARS),
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
            summary: format!("Changed the photo {n}."),
            summary_targets: vec![(p.home_page_id, p.pages[0].regions[0].id)],
        }
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
    fn request_history_older_index_omits_summary_with_unknown_scope() {
        let p = project();
        let mut value = serde_json::to_value(record(&p, 1)).unwrap();
        value.as_object_mut().unwrap().remove("summaryTargets");
        let entry: EarlierRequest = serde_json::from_value(value).unwrap();
        let snapshot = p
            .prepare_requirements()
            .unwrap()
            .with_earlier_requests(&p, &[entry], 7200)
            .unwrap();
        assert!(snapshot.request_history.contains("Exact request 1"));
        assert!(snapshot.request_history.contains("\"summary\":null"));
        assert!(!snapshot.request_history.contains("Changed the photo"));
        snapshot.validate_current(&p).unwrap();
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
