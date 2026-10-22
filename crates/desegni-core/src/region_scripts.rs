//! Request-owned inert source blocks. Persisting in HTML gives source editing and history the
//! same ownership/removal/Undo semantics as the region's other children; no sidecar can go stale.
use crate::{GenerationCandidate, Project, RequirementsSnapshot, Result, ensure};
use dom_query::NodeRef;
use std::collections::BTreeMap;

pub(crate) const SCRIPT_TYPE: &str = "application/x-desegnos-region";
const SELECTOR: &str = "script[data-desegnos-script]";

fn blocks(html: &str) -> Result<BTreeMap<String, (String, String)>> {
    let doc = crate::editing::parse(html)?;
    let mut result = BTreeMap::new();
    for script in doc.select(SELECTOR).nodes() {
        let id = script.attr("data-desegnos-script").unwrap().to_string();
        ensure(
            uuid::Uuid::parse_str(&id).is_ok(),
            "Region script needs its request UUID.",
        )?;
        ensure(
            script.attr("type").as_deref() == Some(SCRIPT_TYPE) && script.attr("src").is_none(),
            "Region script must be an inert, inline source block.",
        )?;
        let root = script
            .parent()
            .ok_or_else(|| crate::Error("Region script needs a root.".into()))?;
        let identity = root
            .attr(crate::ELEMENT_ID_ATTRIBUTE)
            .or_else(|| root.attr("data-desegnos-region"))
            .unwrap_or_default()
            .to_string();
        ensure(
            !identity.is_empty()
                && result
                    .insert(id, (identity, script.html().to_string()))
                    .is_none(),
            "Region script needs one uniquely owned root per request.",
        )?;
    }
    Ok(result)
}

pub(crate) fn strip(html: &str) -> Result<String> {
    let doc = crate::editing::parse(html)?;
    for node in doc.select(SELECTOR).nodes() {
        node.remove_from_parent();
    }
    crate::editing::serialized(&doc)
}

/// Validate changed ownership before hiding inert blocks from the shared-code scope comparison.
pub(crate) fn validate(
    candidate: &GenerationCandidate,
    snapshot: &RequirementsSnapshot,
    project: &Project,
) -> Result<()> {
    let targets = snapshot.target_ids()?;
    for generated in &candidate.pages {
        let page = project.page(generated.page_id)?;
        let old = blocks(page.content.as_ref().map_or("", |c| c.html.as_str()))?;
        let new = blocks(&generated.content.html)?;
        let doc = crate::editing::parse(&generated.content.html)?;
        let whole_page = candidate.implemented_whole_page(snapshot, page);
        if !whole_page {
            ensure(
                generated.content.javascript
                    == page.content.as_ref().map_or("", |c| c.javascript.as_str()),
                "Local requests must preserve shared JavaScript byte-for-byte.",
            )?;
            let before =
                crate::editing::parse(page.content.as_ref().map_or("", |c| c.html.as_str()))?;
            let shared = |doc: &dom_query::Document| {
                doc.select("script:not([data-desegnos-script])")
                    .nodes()
                    .iter()
                    .map(|n| n.html().to_string())
                    .collect::<Vec<_>>()
            };
            ensure(
                shared(&before) == shared(&doc),
                "Local requests must preserve embedded shared scripts unchanged.",
            )?;
        }
        for id in old.keys().chain(new.keys()) {
            if old.get(id) == new.get(id) {
                continue;
            }
            let owner = page.regions.iter().find(|r| r.id.to_string() == *id);
            let region =
                owner.ok_or_else(|| crate::Error("Region script has no owning request.".into()))?;
            ensure(
                (whole_page
                    || targets.contains(&(page.id, region.id))
                        && candidate.regions.iter().any(|r| {
                            r.region_id == region.id && r.status == crate::RegionStatus::Implemented
                        }))
                    && !region.mobile_only,
                "Region script changed another or conflicted request.",
            )?;
            for script in doc
                .select(&format!("script[data-desegnos-script=\"{id}\"]"))
                .nodes()
            {
                let root = script.parent().unwrap();
                ensure(
                    if region.has_targets() {
                        region.targets().iter().any(|t| {
                            root.attr(crate::ELEMENT_ID_ATTRIBUTE).as_deref() == Some(t.id.as_str())
                        })
                    } else {
                        root.attr("data-desegnos-region").as_deref() == Some(id.as_str())
                    },
                    "Region script must be a direct child of its selected root.",
                )?;
                let subtree = crate::editing::parse(root.html().as_ref())?;
                ensure(
                    subtree
                        .select("[data-desegnos-region]")
                        .nodes()
                        .iter()
                        .all(|n| {
                            n.attr(crate::ELEMENT_ID_ATTRIBUTE)
                                == root.attr(crate::ELEMENT_ID_ATTRIBUTE)
                                && n.attr("data-desegnos-region")
                                    == root.attr("data-desegnos-region")
                        }),
                    "Region script root contains another region.",
                )?;
                validate_code(script.text().as_ref(), root)?;
            }
        }
    }
    Ok(())
}

fn validate_code(code: &str, root: NodeRef<'_>) -> Result<()> {
    let wrapped = format!("(function(root, window){{ 'use strict';\n{code}\n}})");
    crate::source::validate_behavior_javascript(&wrapped, true)?;
    let map = swc_common::SourceMap::default();
    let file = map.new_source_file(
        swc_common::FileName::Custom("region.js".into()).into(),
        wrapped,
    );
    let mut errors = vec![];
    let ast = swc_ecma_parser::parse_file_as_script(
        &file,
        swc_ecma_parser::Syntax::Es(Default::default()),
        swc_ecma_ast::EsVersion::latest(),
        None,
        &mut errors,
    )
    .map_err(|_| crate::Error("Region script cannot be parsed.".into()))?;
    let tree = serde_json::to_value(ast).map_err(|e| crate::Error(e.to_string()))?;
    let region_doc = crate::editing::parse(root.html().as_ref())?;
    let region_root = crate::editing::body(&region_doc)?
        .element_children()
        .first()
        .copied()
        .ok_or_else(|| crate::Error("Region script needs an element root.".into()))?;
    let mut pending = vec![(&tree, false)];
    while let Some((value, called)) = pending.pop() {
        if let Some(object) = value.as_object() {
            if value["type"] == "CallExpression" {
                let callee = &value["callee"];
                let method = callee["property"]["value"].as_str().unwrap_or("");
                if matches!(method, "querySelector" | "querySelectorAll") {
                    // Unknown/dynamic selectors require review. All accepted queries start at root;
                    // callbacks can use the resulting descendants without a document capability.
                    ensure(
                        callee["object"]["type"] == "Identifier"
                            && callee["object"]["value"] == "root",
                        "Region script selectors must be rooted at root.",
                    )?;
                    let selector = value["arguments"][0]["expression"]["value"]
                        .as_str()
                        .ok_or_else(|| {
                            crate::Error("Region script dynamic selector requires review.".into())
                        })?;
                    ensure(
                        !selector.contains(":has(")
                            && !region_doc.select(selector).nodes().is_empty(),
                        "Region script selector does not resolve inside its root.",
                    )?;
                    for node in region_doc.select(selector).nodes() {
                        let mut ancestor = node.parent();
                        let mut inside = false;
                        while let Some(node) = ancestor {
                            if node.id == region_root.id {
                                inside = true;
                                break;
                            }
                            ancestor = node.parent();
                        }
                        ensure(
                            inside,
                            "Region script selector does not resolve to a descendant of root.",
                        )?;
                        let mut parent = Some(*node);
                        while let Some(node) = parent {
                            ensure(
                                node.attr("data-desegnos-region").is_none_or(|id| {
                                    root.attr("data-desegnos-region").as_deref()
                                        == Some(id.as_ref())
                                }),
                                "Region script selector touches another region.",
                            )?;
                            parent = node.parent();
                        }
                    }
                }
            }
            let kind = value["type"].as_str().unwrap_or("");
            if kind == "Identifier"
                && [
                    "Math",
                    "JSON",
                    "Number",
                    "String",
                    "Boolean",
                    "Array",
                    "Date",
                    "console",
                    "parseInt",
                    "parseFloat",
                    "isNaN",
                ]
                .contains(&value["value"].as_str().unwrap_or(""))
            {
                ensure(
                    called,
                    "Region script cannot alias or change shared native objects.",
                )?;
            }
            if kind == "MemberExpression" {
                let name = value["property"]["value"].as_str().unwrap_or("");
                let native = [
                    "Math", "JSON", "Number", "String", "Boolean", "Array", "Date", "console",
                ]
                .contains(&value["object"]["value"].as_str().unwrap_or(""));
                let method = [
                    "querySelector",
                    "querySelectorAll",
                    "getAttribute",
                    "addEventListener",
                    "removeEventListener",
                    "appendChild",
                    "append",
                    "remove",
                    "sort",
                    "map",
                    "forEach",
                    "filter",
                    "reduce",
                    "find",
                    "matches",
                    "contains",
                    "getBoundingClientRect",
                ]
                .contains(&name);
                ensure(
                    !(native || method) || called,
                    "Region script methods must be called directly, without aliasing shared capabilities.",
                )?;
                pending.push((&value["object"], native && called));
                // Member property names are not variable references.
            } else {
                pending.extend(object.iter().map(|(key, value)| {
                    (
                        value,
                        key == "callee" && matches!(kind, "CallExpression" | "NewExpression"),
                    )
                }));
            }
        } else if let Some(array) = value.as_array() {
            pending.extend(array.iter().map(|v| (v, false)));
        }
    }
    Ok(())
}

/// Compile trusted accepted source into ordinary JS functions, never eval/Function. Use a separate
/// resource after the page script so a page exception cannot prevent the region queue from running.
pub fn region_script_runtime(html: &str) -> Result<String> {
    let doc = crate::editing::parse(html)?;
    let mut output = String::new();
    for script in doc.select(SELECTOR).nodes() {
        ensure(
            script.attr("type").as_deref() == Some(SCRIPT_TYPE) && script.attr("src").is_none(),
            "Invalid region script source block.",
        )?;
        let id = script.attr("data-desegnos-script").unwrap();
        ensure(
            uuid::Uuid::parse_str(&id).is_ok(),
            "Invalid region script owner.",
        )?;
        let selector = serde_json::to_string(&format!("script[data-desegnos-script=\"{id}\"]"))
            .map_err(|e| crate::Error(e.to_string()))?;
        let code = script.text();
        output.push_str(&format!(r#"
;(() => {{
  const block = document.querySelector({selector});
  const root = block && block.parentElement;
  if (!root) return;
  const listeners = new Map();
  const scopedWindow = Object.freeze({{
    get scrollY() {{ return window.scrollY; }},
    get innerWidth() {{ return window.innerWidth; }},
    get innerHeight() {{ return window.innerHeight; }},
    addEventListener(type, callback) {{
      if (!['scroll', 'resize', 'pointermove', 'pointerdown', 'pointerup'].includes(type)) return;
      const handler = event => {{ try {{ callback(Object.freeze({{type, clientX:event.clientX, clientY:event.clientY, target:root, currentTarget:root}})); }} catch (error) {{ console.error('Region script event failed', error); }} }};
      let types = listeners.get(callback);
      if (!types) listeners.set(callback, types = new Map());
      if (types.has(type)) return;
      types.set(type, handler);
      window.addEventListener(type, handler);
    }},
    removeEventListener(type, callback) {{
      const types = listeners.get(callback);
      if (types) {{ window.removeEventListener(type, types.get(type)); types.delete(type); }}
    }}
  }});
  try {{ (function(root, window) {{ 'use strict';
{code}
  }})(root, scopedWindow); }} catch (error) {{ console.error('Region script failed', error); }}
}})();
"#));
    }
    Ok(output)
}
