use desegni_core::{
    Appearance, Brief, ElementTarget, GenerationMode, IdentityAsset, ImportedIdentity, Project,
    Rect, Region, SiteContent, Viewport, digest,
};
use desegni_storage::{ProjectFile, load_project};
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};
use uuid::Uuid;

fn id(case: usize, item: usize) -> Uuid {
    Uuid::from_u128(0xd35e6000000040008000000000000000_u128 + ((case as u128) << 16) + item as u128)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mode = args
        .next()
        .ok_or("Use --write or --check and the benchmark directory")?;
    let root = PathBuf::from(args.next().ok_or("Missing benchmark directory")?).canonicalize()?;
    if args.next().is_some() || !matches!(mode.as_str(), "--write" | "--check") {
        return Err("Use --write or --check and the benchmark directory".into());
    }
    let catalogue: Value = serde_json::from_slice(&fs::read(root.join("scenarios.json"))?)?;
    let scenarios = catalogue["scenarios"]
        .as_array()
        .ok_or("Missing scenarios")?;
    if scenarios.len() != 18 {
        return Err("The frozen benchmark must contain 18 scenarios".into());
    }
    let mut reports = vec![];
    for (offset, scenario) in scenarios.iter().enumerate() {
        let case = offset + 1;
        let name = scenario["id"].as_str().ok_or("Missing scenario id")?;
        let path = root.join("projects").join(format!("{name}.desegni"));
        {
            if mode == "--write" && path.exists() {
                return Err(format!(
                    "Refusing to replace an existing benchmark fixture: {}",
                    path.display()
                )
                .into());
            }
            let viewport: Viewport = serde_json::from_value(scenario["viewport"].clone())?;
            let mut project = Project::new(
                scenario["title"].as_str().ok_or("Missing title")?.into(),
                viewport,
                Appearance::Light,
            )?;
            project.id = id(case, 0);
            project.home_page_id = id(case, 1);
            project.pages[0].id = project.home_page_id;
            project.brief = Brief { purpose: "Build a usable, accessible standalone website from the fixed benchmark requests. Do not invent external services, testimonials, certifications or unavailable assets.".into(), audience: "Visitors using ordinary desktop and mobile browsers, including keyboard and screen reader users.".into(), brand_name: "Northstar Studio".into(), tone: "Clear, welcoming and direct.".into() };
            if scenario["importedBrand"].as_bool() == Some(true) {
                let asset: IdentityAsset =
                    serde_json::from_slice(&fs::read(root.join("brand/logo.json"))?)?;
                let imported = ImportedIdentity { source_name: "Fixed benchmark brand".into(), source_sha256: digest(&fs::read(root.join("brand/IDENTITY.md"))?), summary: "Northstar Studio uses blue #225577, warm white #faf8f2 and ink #18232a, with the approved star logo and clear, welcoming language.".into(), assets: vec![asset] };
                project = project.set_initial_identity(imported)?;
            }
            project.generation_mode = if scenario["mode"] == "strict" {
                GenerationMode::Strict
            } else {
                GenerationMode::Fuzzy
            };
            let mut pages = vec![];
            for (page_offset, definition) in scenario["pages"]
                .as_array()
                .ok_or("Missing pages")?
                .iter()
                .enumerate()
            {
                let mut page = project.pages[0].clone();
                page.id = id(case, 1 + page_offset);
                page.name = definition["name"]
                    .as_str()
                    .ok_or("Missing page name")?
                    .into();
                page.path = definition["path"].as_str().ok_or("Missing path")?.into();
                page.content = if definition["source"].is_object() {
                    Some(serde_json::from_value::<SiteContent>(
                        definition["source"].clone(),
                    )?)
                } else {
                    None
                };
                page.regions.clear();
                for (request_offset, request) in definition["requests"]
                    .as_array()
                    .ok_or("Missing requests")?
                    .iter()
                    .enumerate()
                {
                    let element = if request["element"].is_object() {
                        let source = page
                            .content
                            .as_ref()
                            .ok_or("Element revisions need accepted source")?;
                        Some(ElementTarget {
                            selector: request["element"]["selector"]
                                .as_str()
                                .ok_or("Missing selector")?
                                .into(),
                            tag: request["element"]["tag"]
                                .as_str()
                                .ok_or("Missing tag")?
                                .into(),
                            label: request["element"]["label"]
                                .as_str()
                                .ok_or("Missing label")?
                                .into(),
                            source_sha256: source.sha256()?,
                        })
                    } else {
                        None
                    };
                    page.regions.push(Region {
                        id: id(case, 100 + page_offset * 100 + request_offset),
                        rect: serde_json::from_value::<Rect>(request["rect"].clone())?,
                        z_index: request_offset as i32,
                        prompt: request["prompt"].as_str().ok_or("Missing prompt")?.into(),
                        element,
                        completed: false,
                    });
                }
                pages.push(page);
            }
            project.pages = pages;
            project.validate()?;
            for page in &project.pages {
                for region in &page.regions {
                    if let Some(target) = &region.element {
                        project.inspect_element(page.id, target)?;
                    }
                }
            }
            if mode == "--write" {
                ProjectFile::acquire(&path)?.save(&project)?;
            } else if load_project(&path)? != project {
                return Err(format!(
                    "The persisted fixture disagrees with its fixed scenario: {name}"
                )
                .into());
            }
        }
        let project = load_project(&path)?;
        project.validate()?;
        let prepared = project.prepare_requirements()?;
        if project.pending_count() == 0 || prepared.region_count == 0 {
            return Err("Every benchmark fixture needs real pending requests".into());
        }
        reports.push(json!({ "id": name, "path": format!("projects/{name}.desegni"), "sha256": digest(&fs::read(path)?), "projectId": project.id, "pendingRequests": prepared.region_count, "affectedPages": prepared.page_count, "requirementsBytes": prepared.markdown.len() }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"schemaVersion":1,"scenarios":reports}))?
    );
    Ok(())
}
