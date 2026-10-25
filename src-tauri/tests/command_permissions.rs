//! Every registered command must be in the permission manifest and allowed for the editor,
//! or Tauri rejects it at runtime before it runs.
use std::collections::BTreeSet;

fn quoted(text: &str) -> BTreeSet<String> {
    text.split('"').skip(1).step_by(2).map(str::to_owned).collect()
}

#[test]
fn registered_commands_are_permitted_for_the_editor() {
    let main = include_str!("../src/main.rs");
    let handlers = main.split("generate_handler![").nth(1).unwrap().split(']').next().unwrap();
    let registered: BTreeSet<String> = handlers
        .split(',')
        .map(|entry| entry.trim().rsplit("::").next().unwrap().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    let build = include_str!("../build.rs");
    let manifest = quoted(build.split("commands(&[").nth(1).unwrap().split("])").next().unwrap());
    let capability: BTreeSet<String> = quoted(include_str!("../capabilities/editor.json"))
        .into_iter()
        .filter_map(|permission| permission.strip_prefix("allow-").map(|name| name.replace('-', "_")))
        .collect();
    let unlisted: Vec<_> = registered.difference(&manifest).collect();
    assert!(unlisted.is_empty(), "missing from build.rs commands: {unlisted:?}");
    let denied: Vec<_> = manifest.difference(&capability).collect();
    assert!(denied.is_empty(), "missing from capabilities/editor.json: {denied:?}");
}
