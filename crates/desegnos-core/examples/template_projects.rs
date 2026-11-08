//! Prints the start-page templates as the editor receives them. The Node editor-shell test
//! feeds this JSON through the real React render path.
use desegnos_core::{
    Appearance, CanvasTemplate, ElementKind, Project, Rect, ReferenceItem, ReferenceKind,
    empty_element,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn main() {
    let projects: Vec<Value> = CanvasTemplate::ALL
        .into_iter()
        .map(|template| {
            let name = serde_json::to_value(template)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            let mut project =
                Project::untitled(format!("{name} template"), Appearance::Light, template)
                    .expect("the start templates are valid");
            if let Some(page) = project.pages.first_mut() {
                page.elements.push(empty_element(
                    ElementKind::Box,
                    Rect {
                        x: 24.0,
                        y: 32.0,
                        width: 320.0,
                        height: 180.0,
                    },
                ));
                page.elements.push(empty_element(
                    ElementKind::Text,
                    Rect {
                        x: 48.0,
                        y: 56.0,
                        width: 240.0,
                        height: 32.0,
                    },
                ));
            }
            project.reference.push(ReferenceItem {
                id: Uuid::new_v4(),
                rect: Rect {
                    x: -260.0,
                    y: 40.0,
                    width: 200.0,
                    height: 120.0,
                },
                kind: ReferenceKind::Note {
                    text: "Reference note".into(),
                },
            });
            json!({ "template": name, "project": project })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string(&projects).expect("templates serialize")
    );
}
