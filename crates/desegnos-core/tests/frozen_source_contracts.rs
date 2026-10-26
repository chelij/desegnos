use desegnos_core::*;
use std::collections::BTreeMap;

#[test]
fn edits_and_export_use_source_after_a_disposable_render_changes_the_dom() {
    let files = BTreeMap::from([("index.html".into(), br#"<!doctype html><html><head></head><body><button class="authored" style="color:red">Source</button><script>const b=document.querySelector('button');b.classList.add('active');b.style.opacity='.7';b.before(document.createElement('aside'));</script></body></html>"#.to_vec())]);
    let project = import_website_files("Frozen source", &files).unwrap();
    let source = project.pages[0].content.as_ref().unwrap();
    let doc = dom_query::Document::from(source.html.as_str());
    let id = doc
        .select("button")
        .attr("data-desegnos-id")
        .unwrap()
        .to_string();
    let preview = preview_source_document(&source.html).unwrap();
    let rendered = dom_query::Document::from(preview);
    rendered
        .select("button")
        .set_attr("class", "authored active");
    rendered
        .select("button")
        .set_attr("style", "color:red;opacity:.7");
    let target = ElementTarget {
        id,
        selector: "body > button:nth-of-type(2)".into(),
        tag: "button".into(),
        label: "Rendered label".into(),
        source_sha256: String::new(),
    };
    // The browser adapter sends the source style plus only the deliberate edit.
    // Even a stale rendered selector cannot redirect a stable source ID.
    let (edited, _) = project
        .edit_element(
            project.revision,
            project.home_page_id,
            &target,
            &ElementEdit::Update {
                texts: vec![TextEdit {
                    index: 0,
                    text: "Edited source".into(),
                }],
                attributes: BTreeMap::new(),
                style: Some("color:red;translate:10px 20px !important".into()),
            },
        )
        .unwrap();
    let saved = edited.pages[0].content.as_ref().unwrap();
    let saved_doc = dom_query::Document::from(saved.html.as_str());
    assert_eq!(
        saved_doc.select("button").attr("class").as_deref(),
        Some("authored")
    );
    assert_eq!(saved_doc.select("button").text().as_ref(), "Edited source");
    assert!(
        !saved_doc
            .select("button")
            .attr("style")
            .unwrap()
            .contains("opacity")
    );
    assert!(saved_doc.select("aside").is_empty());
    assert!(!saved.html.contains("data-desegnos-preview-"));
    assert_eq!(source.javascript, saved.javascript);
    let exported = compile_website(&edited).unwrap();
    let html = String::from_utf8(exported.files["index.html"].clone()).unwrap();
    assert!(html.contains("Edited source"));
    assert!(!html.contains("data-desegnos-preview-"));
    assert!(!html.contains("class=\"authored active\""));
}
