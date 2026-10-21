use desegni_core::*;

#[test]
fn tidy_code_formats_an_existing_page_once_without_the_ai() {
    let mut project = Project::new(
        "Tidy".into(),
        Viewport {
            width: 800,
            height: 600,
        },
        Appearance::Light,
    )
    .unwrap();
    project.pages[0].content = Some(SiteContent {
        html: "<main><section><h1>Hi</h1><p>One <b>line</b></p></section><footer><p>End</p></footer></main>".into(),
        css: "main{padding:4px}h1{margin:0}".into(),
        javascript: String::new(),
    });
    let page_id = project.pages[0].id;
    let tidy = |project: &Project| {
        project
            .apply(project.revision, Command::FormatPageCode { page_id })
            .unwrap()
    };
    let tidied = tidy(&project);
    let content = tidied.pages[0].content.as_ref().unwrap();
    assert!(content.html.lines().count() > 1, "{}", content.html);
    assert!(content.css.lines().count() > 1, "{}", content.css);
    assert!(
        content.html.contains("<p>One <b>line</b></p>"),
        "inline text is untouched"
    );
    assert_eq!(tidied.revision, project.revision + 1);
    // Already tidy: nothing changes, so no new history entry.
    assert_eq!(tidy(&tidied).revision, tidied.revision);
}
