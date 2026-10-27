//! Text-only project-local history, derived from frozen job requests and outcomes.
use crate::{Project, Rect, RequestOutcome, RequestStatus, RequirementsSnapshot, Result, digest, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const HEADING: &str = "\n## Earlier requests for these layers (context only — the current requests are the authority)\n\n```jsonl\n";
pub const REQUEST_HISTORY_CHARS: usize = 3000;
const RETRY_HEADING: &str = "\n## Previous validation rejections for retried layers\n\nThese are app checks, not user rejections. Correct each listed layer while preserving the current scope.\n\n```jsonl\n";
const REJECTION_CHARS: usize = 600;
const RETRY_HISTORY_CHARS: usize = crate::MAX_GENERATION_REQUESTS * 4500;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EarlierRequest {
    pub project_id: Uuid,
    pub job_id: Uuid,
    pub page_id: Uuid,
    pub element_id: Uuid,
    pub rect: Rect,
    pub requested_at: i64,
    pub request: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_rejection: Option<String>,
    pub summary: String,
    /// The complete original scope of the job-wide summary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_targets: Vec<(Uuid, Uuid)>,
}

/// Read request blocks and source hashes, never attachments, source text, or prior context.
pub fn earlier_requests(
    snapshot: &RequirementsSnapshot,
    requested_at: i64,
    outcome: &str,
    summary: &str,
    requests: &[RequestOutcome],
) -> Vec<EarlierRequest> {
    let mut earlier = Vec::new();
    let mut fence = None;
    let mut block = String::new();
    for line in snapshot.markdown.lines() {
        if let Some(end) = &fence {
            if line == end {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&block) {
                    let parse = || {
                        let page_id = serde_json::from_value(value.get("pageId")?.clone()).ok()?;
                        let element_id =
                            serde_json::from_value(value.get("elementId")?.clone()).ok()?;
                        let rect =
                            serde_json::from_value(value.get("rect")?.clone()).ok()?;
                        let request = value.get("request")?.as_str()?;
                        let question = requests.iter().find(|r| {
                            r.element_id == element_id && r.status == RequestStatus::Conflict
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
                            element_id,
                            rect,
                            requested_at,
                            request: clip(request, 300),
                            outcome,
                            validation_rejection: None,
                            summary: clip(
                                &summary.split_whitespace().collect::<Vec<_>>().join(" "),
                                200,
                            ),
                            summary_targets: Vec::new(),
                        })
                    };
                    if let Some(request) = parse() {
                        earlier.push(request);
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
    for request in &mut earlier {
        request.summary_targets = summary_targets.clone();
    }
    earlier
}

/// Keep only the newest five records for each historical layer in the local index.
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
            .entry((r.project_id, r.page_id, r.element_id))
            .or_insert(0);
        if *count >= 5 || !seen.insert((r.project_id, r.job_id, r.element_id)) {
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
/// Preserve the validator's wording, including punctuation, up to the per-layer limit.
pub fn record_validation_rejection(request: &mut EarlierRequest, reason: &str) {
    request.outcome = "rejected by the app's checks".into();
    request.validation_rejection = Some(clip(reason, REJECTION_CHARS));
}

fn prevention_rule(reason: &str) -> &'static str {
    if reason.contains("outside its request") {
        "Change only this layer's subtree and the targets its pointers grant."
    } else if reason.contains("inside") || reason.contains("frame") {
        "Keep every new or changed element inside its parent's box, in the parent's coordinates."
    } else if reason.contains("pointer") {
        "Classify every pointer with its current comment, and use only the listed Play actions."
    } else if reason.contains("image") {
        "Reference generated pictures by their image request keys, and keep every image element in the reply's images list."
    } else {
        "Change only the requested layers and their allowed targets, and follow the reply schema exactly."
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
        let mut targets = Vec::new();
        for (page_id, element_id) in &ids {
            if let Ok(page) = project.page(*page_id)
                && let Some(element) = crate::elements::find_in(&page.elements, *element_id)
            {
                targets.push((page, element));
            }
        }
        let mut records: Vec<_> = records
            .iter()
            .filter(|r| r.project_id == project.id && r.job_id != self.job_id)
            .collect();
        records.sort_by_key(|r| {
            (
                std::cmp::Reverse(r.requested_at),
                std::cmp::Reverse(r.job_id),
                r.element_id,
            )
        });
        // Exact layer identity, never proximity: a nearby layer's failure is not this retry's.
        // Reserve separate bounded space so five older records cannot crowd out a rejection.
        let mut retry = String::new();
        for (page, element) in &targets {
            if let Some(reason) = records
                .iter()
                .find(|r| r.page_id == page.id && r.element_id == element.id)
                .and_then(|r| r.validation_rejection.as_deref())
            {
                let reason = clip(reason, REJECTION_CHARS);
                let instruction = format!(
                    "Your previous attempt for this layer was rejected because: {reason} Avoid this by: {}",
                    prevention_rule(&reason)
                );
                retry.push_str(&format!(
                    "{}\n",
                    serde_json::json!({"page":page.id,"element":element.id,"instruction":instruction})
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
        let mut counts = vec![0; targets.len()];
        let mut seen = std::collections::BTreeSet::new();
        let mut section = String::new();
        for r in records {
            let matched: Vec<_> = targets
                .iter()
                .enumerate()
                .filter(|(i, (p, element))| {
                    counts[*i] < 5
                        && r.page_id == p.id
                        && (r.element_id == element.id || nearby(element.rect, r.rect))
                })
                .map(|(i, _)| i)
                .collect();
            if matched.is_empty() || !seen.insert((r.job_id, r.element_id)) {
                continue;
            }
            // JSON quoting keeps exact user text as data, including line breaks and fences.
            // A per-layer record still carries a job-wide summary. Never expose it to a
            // narrower request or one whose summary scope is not established.
            let summary = (!r.summary_targets.is_empty()
                && r.summary_targets.iter().all(|target| ids.contains(target)))
            .then(|| clip(&r.summary, 200));
            let entry = serde_json::json!({"page":r.page_id,"element":r.element_id,"when":relative(now,r.requested_at),
                "request":clip(&r.request,300),"outcome":clip(&r.outcome,320),"summary":summary,
                "validationReason":r.validation_rejection.as_deref().map(|reason| clip(reason, REJECTION_CHARS))});
            let line = format!("{}\n", entry);
            if HEADING.chars().count()
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
            retry
        } else {
            format!("{retry}{HEADING}{section}```\n")
        })?;
        Ok(self)
    }

    pub(crate) fn set_request_history(&mut self, section: String) -> Result<()> {
        ensure(
            section.is_empty()
                || ((section.starts_with(HEADING)
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
                .find("\n## Reference items\n")
                .ok_or_else(|| crate::Error("Missing reference items.".into()))?;
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
