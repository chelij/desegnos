//! The canvas renders imported sites in srcdoc frames that inherit the editor's policy, and
//! loads their images, fonts and stylesheets from the preview session.

#[test]
fn editor_policy_lets_canvas_frames_load_preview_resources() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    for key in ["csp", "devCsp"] {
        let policy = config["app"]["security"][key].as_str().unwrap();
        for directive in ["img-src", "font-src", "style-src"] {
            let sources = policy
                .split(';')
                .map(str::trim)
                .find_map(|part| part.strip_prefix(directive))
                .unwrap_or_else(|| panic!("{key} has no {directive}"));
            for source in ["desegnos-preview:", "http://*.desegnos-preview.localhost"] {
                assert!(
                    sources.split_whitespace().any(|item| item == source),
                    "{key} {directive} must allow {source}"
                );
            }
        }
    }
}
