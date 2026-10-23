//! Review and merge a website snapshot without replacing project-owned context.
use crate::{Project, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteImportReview {
    pub updated: Vec<String>,
    pub added: Vec<String>,
    pub kept: Vec<WebsiteImportPage>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebsiteImportPage {
    pub id: Uuid,
    pub name: String,
    pub path: String,
}

fn checked_import(files: &BTreeMap<String, Vec<u8>>) -> Result<Project> {
    let mut routes = BTreeSet::new();
    for (path, bytes) in files {
        ensure(
            !path.is_empty()
                && !path.contains(['\\', ':', '\0'])
                && path
                    .split('/')
                    .all(|p| !p.is_empty() && p != "." && p != ".."),
            "The website contains an unsafe path.",
        )?;
        if ["css", "js", "mjs"].contains(
            &path
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
        ) {
            ensure(
                std::str::from_utf8(bytes).is_ok(),
                "A stylesheet or script is not UTF-8.",
            )?;
        }
        if path.to_ascii_lowercase().ends_with(".html")
            || path.to_ascii_lowercase().ends_with(".htm")
        {
            let html = std::str::from_utf8(bytes)
                .map_err(|_| crate::Error("An HTML file is not UTF-8.".into()))?;
            ensure(
                bytes.len() <= crate::MAX_SITE_SOURCE_BYTES,
                "An HTML file is too large to import.",
            )?;
            crate::source::validate_html_before_repair(html)
                .map_err(|_| crate::Error("An HTML file could not be parsed.".into()))?;
            crate::editing::parse_document(html, true)?;
            ensure(
                routes.insert(crate::source_page_path(path).to_lowercase()),
                "Two imported files have the same page path.",
            )?;
        }
    }
    ensure(
        routes.len() <= 100,
        "A website can contain at most 100 pages.",
    )?;
    let pages = files
        .keys()
        .filter(|p| {
            p.to_ascii_lowercase().ends_with(".html") || p.to_ascii_lowercase().ends_with(".htm")
        })
        .cloned()
        .collect();
    let mut imported = crate::website_import::import_website_pages("Website", files, pages)?;
    // A selection containing only about.html must remain /about, rather than becoming home.
    for page in &mut imported.pages {
        page.path = crate::source_page_path(
            &imported.website.as_ref().expect("imported site").page_files[&page.id],
        );
    }
    Ok(imported)
}

// Position means the full sequence of element-child indices, so inserting an unrelated
// sibling cannot accidentally transfer identity across different parents.
fn reuse_ids(old: &str, new: &str) -> Result<String> {
    let before = crate::editing::parse_document(old, true)?;
    let after = crate::editing::parse_document(&crate::source::strip_editor_ids(new), true)?;
    let mut pending = vec![(before.root(), after.root())];
    while let Some((old, new)) = pending.pop() {
        let old_children: Vec<_> = old
            .children()
            .into_iter()
            .filter(|n| n.is_element())
            .collect();
        let new_children: Vec<_> = new
            .children()
            .into_iter()
            .filter(|n| n.is_element())
            .collect();
        for (old, new) in old_children.into_iter().zip(new_children) {
            if old.node_name() != new.node_name() {
                continue;
            }
            let normalize = |s: &str| {
                s.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            };
            let a = normalize(&old.text());
            let b = normalize(&new.text());
            let a_words: BTreeSet<_> = a.split_whitespace().collect();
            let b_words: BTreeSet<_> = b.split_whitespace().collect();
            let similar = a == b
                || (!a_words.is_empty()
                    && a_words.intersection(&b_words).count() * 3
                        >= a_words.union(&b_words).count() * 2);
            if similar && let Some(id) = old.attr(crate::ELEMENT_ID_ATTRIBUTE) {
                new.set_attr(crate::ELEMENT_ID_ATTRIBUTE, &id);
            }
            pending.push((old, new));
        }
    }
    crate::source::assign_editor_ids(&format!("<!doctype html>\n{}", after.html_root().html()))
}

impl Project {
    pub fn review_website_import(
        &self,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<WebsiteImportReview> {
        let imported = checked_import(files)?;
        Ok(WebsiteImportReview {
            updated: imported
                .pages
                .iter()
                .filter(|p| self.pages.iter().any(|old| old.path == p.path))
                .map(|p| p.path.clone())
                .collect(),
            added: imported
                .pages
                .iter()
                .filter(|p| !self.pages.iter().any(|old| old.path == p.path))
                .map(|p| p.path.clone())
                .collect(),
            kept: self
                .pages
                .iter()
                .filter(|p| !imported.pages.iter().any(|new| new.path == p.path))
                .map(|p| WebsiteImportPage {
                    id: p.id,
                    name: p.name.clone(),
                    path: p.path.clone(),
                })
                .collect(),
        })
    }

    pub fn import_website(
        &self,
        revision: u64,
        files: &BTreeMap<String, Vec<u8>>,
        remove: &[Uuid],
    ) -> Result<Self> {
        ensure(
            revision == self.revision,
            "The project changed; review the import again.",
        )?;
        let review = self.review_website_import(files)?;
        ensure(
            !remove.contains(&self.home_page_id),
            "The home page cannot be removed.",
        )?;
        ensure(
            remove
                .iter()
                .all(|id| review.kept.iter().any(|p| p.id == *id)),
            "Only pages absent from the import can be removed.",
        )?;
        let exported = crate::compile_website(self)
            .ok()
            .map(|site| site.files)
            .unwrap_or_default();
        if remove.is_empty() && exported == *files {
            return Ok(self.clone());
        }
        let imported = checked_import(files)?;
        let mut next = self.clone();
        // Retain exported resources too: pages omitted from a partial import still need them.
        let mut resources = exported.clone();
        resources.extend(files.clone());
        let mut website = self.website.clone().unwrap_or(crate::ImportedWebsite {
            page_files: self
                .pages
                .iter()
                .map(|p| (p.id, crate::export::output_path(&p.path)))
                .collect(),
            files: BTreeMap::new(),
            framework: None,
            skipped: vec![],
        });
        let resources_changed = files
            .iter()
            .filter(|(path, _)| !path.ends_with(".html") && !path.ends_with(".htm"))
            .any(|(path, bytes)| exported.get(path) != Some(bytes));
        for mut page in imported.pages {
            let source =
                imported.website.as_ref().expect("imported site").page_files[&page.id].clone();
            if let Some(old) = next.pages.iter_mut().find(|old| old.path == page.path) {
                website.page_files.insert(old.id, source.clone());
                let same_source =
                    old.content
                        .as_ref()
                        .zip(page.content.as_ref())
                        .is_some_and(|(a, b)| {
                            a.css == b.css
                                && a.javascript == b.javascript
                                && dom_query::Document::from(crate::source::strip_editor_ids(
                                    &a.html,
                                ))
                                .html_root()
                                .html()
                                    == dom_query::Document::from(crate::source::strip_editor_ids(
                                        &b.html,
                                    ))
                                    .html_root()
                                    .html()
                        });
                if (exported.get(&source) == files.get(&source) || same_source)
                    && !resources_changed
                {
                    continue;
                }
                let mut content = page.content.take().expect("imported content");
                if let Some(previous) = &old.content {
                    content.html = reuse_ids(&previous.html, &content.html)?;
                }
                // Keep all requests, including unresolved targets, open and visible.
                old.content = Some(content);
                if let Some(content) = &old.content {
                    let doc = crate::editing::parse_document(&content.html, true)?;
                    let body = crate::editing::body(&doc)?;
                    let hash = content.sha256()?;
                    for request in old.regions.iter_mut().filter(|r| !r.completed) {
                        let targets = request
                            .targets()
                            .into_iter()
                            .cloned()
                            .map(|target| {
                                crate::editing::locate(&doc, &target)
                                    .ok()
                                    .and_then(|node| crate::editing::target(node, body, &hash))
                                    .unwrap_or(target)
                            })
                            .collect();
                        request.elements = targets;
                        request.element = None;
                        request.area_bounds.clear();
                    }
                }
            } else {
                website.page_files.insert(page.id, source);
                next.pages.push(page);
            }
        }
        next.pages.retain(|p| !remove.contains(&p.id));
        ensure(
            !next.pages.is_empty(),
            "The project must keep at least one page.",
        )?;
        website
            .page_files
            .retain(|id, _| next.pages.iter().any(|p| p.id == *id));
        let html_paths: BTreeSet<_> = website
            .page_files
            .values()
            .cloned()
            .chain(
                self.pages
                    .iter()
                    .map(|p| crate::export::output_path(&p.path)),
            )
            .collect();
        for (path, bytes) in resources {
            if !html_paths.contains(&path) && !path.ends_with(".html") && !path.ends_with(".htm") {
                website.files.insert(path, STANDARD.encode(bytes).into());
            }
        }
        next.website = Some(website);
        next.pictures.extend(imported.pictures);
        next.store_inline_images()?;
        if next == *self {
            return Ok(self.clone());
        }
        next.revision = revision
            .checked_add(1)
            .ok_or_else(|| crate::Error("Project revision limit reached.".into()))?;
        next.validate()?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Appearance, Command, Rect, Session, SiteContent, Viewport};

    fn html(text: &str) -> Vec<u8> {
        format!("<!doctype html><html><head><title>Site</title></head><body><main><h1>{text}</h1><p>Keep this text</p></main></body></html>").into_bytes()
    }
    fn site() -> Project {
        crate::import_website_files(
            "Original",
            &BTreeMap::from([
                ("index.html".into(), html("Home")),
                ("about/index.html".into(), html("About us")),
                ("keep.html".into(), html("Keep")),
            ]),
        )
        .unwrap()
    }
    fn element_id(html: &str, selector: &str) -> String {
        dom_query::Document::from(html)
            .select_single(selector)
            .nodes()[0]
            .attr(crate::ELEMENT_ID_ATTRIBUTE)
            .unwrap()
            .into()
    }

    #[test]
    fn matches_flat_and_directory_routes_without_promoting_a_partial_import_to_home() {
        let project = site();
        for path in ["about.html", "about/index.html"] {
            let review = project
                .review_website_import(&BTreeMap::from([(path.into(), html("About us updated"))]))
                .unwrap();
            assert_eq!(review.updated, ["/about"]);
            assert!(review.added.is_empty());
            assert_eq!(review.kept.len(), 2);
        }
        let review = project
            .review_website_import(&BTreeMap::from([("index.html".into(), html("Home"))]))
            .unwrap();
        assert_eq!(review.updated, ["/"]);
    }

    #[test]
    fn reuses_only_matching_tag_position_and_similar_text() {
        let old =
            crate::source::assign_editor_ids(std::str::from_utf8(&html("About our team")).unwrap())
                .unwrap();
        let new = reuse_ids(
            &old,
            std::str::from_utf8(&html("About our team today")).unwrap(),
        )
        .unwrap();
        assert_eq!(element_id(&old, "h1"), element_id(&new, "h1"));
        assert_eq!(element_id(&old, "p"), element_id(&new, "p"));
        let different = reuse_ids(
            &old,
            std::str::from_utf8(&html("Completely unrelated")).unwrap(),
        )
        .unwrap();
        assert_ne!(element_id(&old, "h1"), element_id(&different, "h1"));
        let tag = reuse_ids(
            &old,
            &String::from_utf8(html("About our team"))
                .unwrap()
                .replace("h1", "h2"),
        )
        .unwrap();
        assert_ne!(element_id(&old, "h1"), element_id(&tag, "h2"));
        crate::stable_targets::validate_ids(&dom_query::Document::from(new)).unwrap();
    }

    #[test]
    fn add_update_remove_is_one_durable_undo_step_and_preserves_context() {
        let project = site();
        let about = project
            .pages
            .iter()
            .find(|p| p.path == "/about")
            .unwrap()
            .id;
        let keep = project.pages.iter().find(|p| p.path == "/keep").unwrap().id;
        let files = BTreeMap::from([
            ("about.html".into(), html("About us today")),
            ("new/index.html".into(), html("New")),
            ("shared.js".into(), b"console.log('shared')".to_vec()),
        ]);
        let mut session = Session::new(project.clone()).unwrap();
        let mut writes = 0;
        session
            .import_website(project.revision, &files, &[keep], |_| {
                writes += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(writes, 1);
        let merged = session.project();
        assert_eq!(merged.id, project.id);
        assert_eq!(merged.brief, project.brief);
        assert_eq!(merged.decisions, project.decisions);
        assert_eq!(merged.page(about).unwrap().id, about);
        assert!(
            merged
                .page(about)
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .html
                .contains("About us today")
        );
        assert!(merged.pages.iter().any(|p| p.path == "/new"));
        assert!(!merged.pages.iter().any(|p| p.id == keep));
        assert_eq!(
            merged.website_files().unwrap().unwrap()["shared.js"],
            files["shared.js"]
        );
        session.undo(|_| Ok(())).unwrap();
        assert_eq!(session.project().pages, project.pages);
        session.redo(|_| Ok(())).unwrap();
        assert!(session.project().pages.iter().any(|p| p.path == "/new"));
    }

    #[test]
    fn missing_pages_are_kept_unless_selected_and_home_is_protected() {
        let project = site();
        let files = BTreeMap::from([("about.html".into(), html("Updated"))]);
        let next = project
            .import_website(project.revision, &files, &[])
            .unwrap();
        assert_eq!(next.pages.len(), project.pages.len());
        assert!(
            project
                .import_website(project.revision, &files, &[project.home_page_id])
                .is_err()
        );
        let about = project
            .pages
            .iter()
            .find(|p| p.path == "/about")
            .unwrap()
            .id;
        assert!(
            project
                .import_website(project.revision, &files, &[about])
                .is_err()
        );
    }

    #[test]
    fn pending_requests_survive_existing_and_missing_targets() {
        let mut project = site();
        let page = project.home_page_id;
        project = project
            .apply(
                project.revision,
                Command::AddRegion {
                    page_id: page,
                    rect: Rect {
                        x: 10.,
                        y: 10.,
                        width: 100.,
                        height: 100.,
                    },
                },
            )
            .unwrap();
        let old = project.page(page).unwrap().content.as_ref().unwrap();
        let doc = crate::editing::parse_document(&old.html, true).unwrap();
        let target = crate::editing::target(
            doc.select_single("h1").nodes()[0],
            crate::editing::body(&doc).unwrap(),
            &old.sha256().unwrap(),
        )
        .unwrap();
        let request = &mut project
            .pages
            .iter_mut()
            .find(|p| p.id == page)
            .unwrap()
            .regions[0];
        request.element = Some(target.clone());
        request.prompt = "Change this heading".into();
        for text in ["Home today", "Unrelated replacement"] {
            let next = project
                .import_website(
                    project.revision,
                    &BTreeMap::from([("index.html".into(), html(text))]),
                    &[],
                )
                .unwrap();
            let request = &next.page(page).unwrap().regions[0];
            assert!(!request.completed);
            assert_eq!(request.prompt, "Change this heading");
            assert_eq!(request.targets()[0].id, target.id);
        }
    }

    #[test]
    fn unchanged_export_round_trip_has_no_project_or_history_changes() {
        let mut generated = Project::new(
            "Generated".into(),
            Viewport {
                width: 1440,
                height: 900,
            },
            Appearance::Light,
        )
        .unwrap();
        generated.pages[0].content = Some(SiteContent {
            html: crate::source::assign_editor_ids(
                "<main><h1>Hello</h1><a href='/about'>About</a></main>",
            )
            .unwrap(),
            css: "h1 { color: red }".into(),
            javascript: "console.log('hello');".into(),
        });
        for project in [site(), generated] {
            let files = crate::compile_website(&project).unwrap().files;
            let mut session = Session::new(project.clone()).unwrap();
            session
                .import_website(project.revision, &files, &[], |_| {
                    panic!("No-op must not journal")
                })
                .unwrap();
            assert_eq!(session.project(), &project);
            assert!(!session.can_undo());
        }
    }

    #[test]
    fn rejects_unsafe_duplicate_routes_invalid_utf8_and_stale_reviews() {
        let project = site();
        for files in [
            BTreeMap::from([("../index.html".into(), html("Bad"))]),
            BTreeMap::from([
                ("about.html".into(), html("A")),
                ("about/index.html".into(), html("B")),
            ]),
            BTreeMap::from([("index.html".into(), vec![255])]),
        ] {
            assert!(project.review_website_import(&files).is_err());
        }
        assert!(
            project
                .import_website(
                    project.revision + 1,
                    &BTreeMap::from([("index.html".into(), html("Home"))]),
                    &[]
                )
                .is_err()
        );
    }
}
