use desegni_core::*;
use std::cell::Cell;
fn project() -> Project {
    Project::new(
        "Typed website".into(),
        Viewport {
            width: 1200,
            height: 800,
        },
        Appearance::Light,
    )
    .unwrap()
}
fn edit(html: &str, css: &str, javascript: &str) -> PageSourceEdit {
    PageSourceEdit {
        html: Some(html.into()),
        css: Some(css.into()),
        javascript: Some(javascript.into()),
    }
}
#[test]
fn source_edits_are_exact_atomic_one_step_and_no_ops_do_not_write() {
    let project = project();
    let id = project.home_page_id;
    let mut session = Session::new(project.clone()).unwrap();
    let writes = Cell::new(0);
    let change = edit(
        "<main>\n  <h1 id='title'>Création 💡</h1>\n</main>\n",
        "h1 { color: blue; }",
        "document.querySelector('h1').title = 'Edited';",
    );
    assert!(
        session
            .edit_page_source(0, id, &change, |_| Err(Error("Disk full".into())))
            .is_err()
    );
    assert_eq!(session.project(), &project);
    session
        .edit_page_source(0, id, &change, |_| {
            writes.set(writes.get() + 1);
            Ok(())
        })
        .unwrap();
    let accepted = session.project().clone();
    assert_eq!(
        accepted.pages[0].content.as_ref().unwrap().html,
        change.html.clone().unwrap()
    );
    session
        .edit_page_source(accepted.revision, id, &change, |_| {
            writes.set(writes.get() + 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(writes.get(), 1);
    session.undo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, project.pages);
    session.redo(|_| Ok(())).unwrap();
    assert_eq!(session.project().pages, accepted.pages);
    let exported = compile_website(session.project()).unwrap();
    assert!(
        String::from_utf8(exported.files["index.html"].clone())
            .unwrap()
            .contains("Création 💡")
    );
    assert!(
        session
            .edit_page_source(0, id, &change, |_| Ok(()))
            .is_err()
    );
}
#[test]
fn invalid_syntax_and_unsafe_imports_report_the_actual_language_and_line() {
    let project = project();
    for (change, language, line) in [
        (
            edit(
                "<main>\n<script src='https://bad.example/code.js'></script>\n</main>",
                "",
                "",
            ),
            "html",
            2,
        ),
        (
            edit("<p>Safe</p>", "", "const good = 1;\nconst broken = ;"),
            "javascript",
            2,
        ),
        (
            edit(
                "<p>Safe</p>",
                "h1 {color:blue}\nh2 {color: \"unterminated}",
                "",
            ),
            "css",
            2,
        ),
        (
            edit(
                "<p>Safe</p>",
                "",
                "import remote from 'https://bad.example/code.js';",
            ),
            "javascript",
            1,
        ),
        (
            edit(
                "<main>\n<a href='java&#x09;script:alert(1)'>Bad</a>\n</main>",
                "",
                "",
            ),
            "html",
            2,
        ),
        (
            edit(
                "<main>\n<h1 id='same'>One</h1>\n<p id='same'>Two</p>\n</main>",
                "",
                "",
            ),
            "html",
            3,
        ),
        (edit("<main>\n<div class='unfinished", "", ""), "html", 2),
    ] {
        let error = project
            .edit_page_source(0, project.home_page_id, &change)
            .unwrap_err();
        assert!(
            error
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.language == language && diagnostic.line == line),
            "{language} expected line {line}: {error:?}"
        );
        assert!(project.pages[0].content.is_none());
    }
}
#[test]
fn source_map_matches_nested_browser_selectors_and_utf16_columns_without_rewriting() {
    let project = project();
    let html = "<main>\n  <h1>Création 💡</h1><p>你好</p>\n  <table><tr><td>Cell</td></tr></table>\n  <svg><g><title>Mark</title><path d='M0 0'/></g></svg>\n</main>";
    let next = project
        .edit_page_source(0, project.home_page_id, &edit(html, "", ""))
        .unwrap();
    let map = next.page_source_map(project.home_page_id).unwrap();
    assert_eq!(map.html_sha256, digest(html.as_bytes()));
    let heading = map
        .ranges
        .iter()
        .find(|range| range.selector == "body > main:nth-of-type(1) > h1:nth-of-type(1)")
        .unwrap();
    assert_eq!(
        (heading.start_line, heading.start_column, heading.end_line),
        (2, 3, 2)
    );
    let paragraph = map
        .ranges
        .iter()
        .find(|range| range.selector.ends_with("p:nth-of-type(1)"))
        .unwrap();
    assert_eq!(
        paragraph.start_column,
        "  <h1>Création 💡</h1>".encode_utf16().count() + 1
    );
    assert!(map.ranges.iter().any(|range| {
        range
            .selector
            .contains("tbody:nth-of-type(1) > tr:nth-of-type(1) > td:nth-of-type(1)")
    }));
    assert!(map.ranges.iter().any(|range| {
        range
            .selector
            .ends_with("g:nth-of-type(1) > path:nth-of-type(1)")
    }));
    assert_eq!(next.pages[0].content.as_ref().unwrap().html, html);
}
#[test]
fn a_saved_format_eight_packet_still_validates_without_changing_its_bytes() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/requirements8.json")).unwrap();
    let project: Project = serde_json::from_value(golden["project"].clone()).unwrap();
    let snapshot: RequirementsSnapshot =
        serde_json::from_value(golden["requirements"].clone()).unwrap();
    assert_eq!(snapshot.format_version().unwrap(), 8);
    snapshot.validate_current(&project).unwrap();
}

#[test]
fn full_documents_comments_crlf_and_multiline_attributes_have_exact_ranges() {
    for html in [
        "<!doctype html>\r\n<html><head><title>Site</title></head><body>\r\n<h1\r\n id='heading'>Title</h1>\r\n</body></html>",
        "<!-- mentions <html> without making this a document -->\r\n<h1\r\n id='heading'>Title</h1>",
    ] {
        let p = project();
        let next = p
            .edit_page_source(
                0,
                p.home_page_id,
                &edit(
                    html,
                    "@media (min-width: 1px) {h1 {color:blue}}",
                    "const label = 'Création 💡';",
                ),
            )
            .unwrap();
        let map = next.page_source_map(p.home_page_id).unwrap();
        let heading = map
            .ranges
            .iter()
            .find(|r| r.selector.ends_with("h1:nth-of-type(1)"))
            .unwrap();
        assert_eq!(
            heading.start_line,
            if html.starts_with("<!doctype") { 3 } else { 2 }
        );
        assert_eq!(heading.end_line, heading.start_line + 1);
        assert!(!heading.implicit);
        assert_eq!(next.pages[0].content.as_ref().unwrap().html, html);
    }
    let p = project();
    let html = "<h1>Title</h1>\n<table>\n<tr><td>Cell</td></tr>\n</table>\n<footer>End</footer>";
    let next = p
        .edit_page_source(0, p.home_page_id, &edit(html, "", ""))
        .unwrap();
    let map = next.page_source_map(p.home_page_id).unwrap();
    let tbody = map
        .ranges
        .iter()
        .find(|r| r.selector.ends_with("tbody:nth-of-type(1)"))
        .unwrap();
    assert!(tbody.implicit);
    assert_eq!((tbody.start_line, tbody.end_line), (3, 4));
}

#[test]
fn typed_source_remaps_identifiable_pending_requests_and_refuses_losing_written_prompts() {
    let p = project();
    let home = p.home_page_id;
    let mut p = p
        .edit_page_source(
            0,
            home,
            &edit("<main><h1 id='title'>Title</h1><p>Copy</p></main>", "", ""),
        )
        .unwrap();
    let target = ElementTarget {
        selector: "body > main:nth-of-type(1) > h1:nth-of-type(1)".into(),
        tag: "h1".into(),
        label: "Title".into(),
        source_sha256: p.pages[0].content.as_ref().unwrap().sha256().unwrap(),
    };
    let rect = Rect {
        x: 10.0,
        y: 10.0,
        width: 200.0,
        height: 80.0,
    };
    p = p
        .apply(
            p.revision,
            Command::AddElementRegion {
                page_id: home,
                rect,
                element: target,
            },
        )
        .unwrap();
    let region_id = p.pages[0].regions[0].id;
    p = p
        .apply(
            p.revision,
            Command::UpdateRegion {
                page_id: home,
                region_id,
                rect,
                prompt: "Keep this change request".into(),
            },
        )
        .unwrap();
    let next = p
        .edit_page_source(
            p.revision,
            home,
            &edit(
                "<main><section><h1 id='title'>Revised</h1></section><p>Copy</p></main>",
                "",
                "",
            ),
        )
        .unwrap();
    let remapped = &next.pages[0].regions[0];
    assert_eq!(remapped.prompt, "Keep this change request");
    assert!(
        remapped
            .element
            .as_ref()
            .unwrap()
            .selector
            .contains("section:nth-of-type(1)")
    );
    assert_eq!(remapped.id, region_id);
    assert!(
        next.edit_page_source(
            next.revision,
            home,
            &edit("<main><p>Copy</p></main>", "", "")
        )
        .is_err()
    );
    assert!(
        next.edit_page_source(
            next.revision,
            home,
            &edit(
                "<main><h1 id='title'>One</h1><h1 id='title'>Two</h1></main>",
                "",
                ""
            )
        )
        .is_err()
    );
    assert_eq!(next.pages[0].regions[0].prompt, "Keep this change request");
}
