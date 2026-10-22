//! Request-owned inert source blocks. Persisting in HTML gives source editing and history the
//! same ownership/removal/Undo semantics as the region's other children; no sidecar can go stale.
use crate::{GenerationCandidate, Project, RequirementsSnapshot, Result, ensure};
use dom_query::NodeRef;
use std::collections::BTreeMap;

pub(crate) const SCRIPT_TYPE: &str = "application/x-desegnos-region";
pub(crate) const SHARED_JAVASCRIPT_REASON: &str =
    "Local requests must preserve shared JavaScript byte-for-byte.";
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
                SHARED_JAVASCRIPT_REASON,
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
    ensure(errors.is_empty(), "This script could not be read.")?;
    validate_script_tree(&tree)?;
    let _ = root; // Ownership and nested-region boundaries are checked by validate above.

    Ok(())
}

// This is a capability check, not a property allowlist: array/data operations and selectors
// built from request data are ordinary DOM code. Global document/window are limited facades.
fn validate_script_tree(tree: &serde_json::Value) -> Result<()> {
    use serde_json::Value;
    use std::collections::BTreeSet;
    fn binding(value: &Value, locals: &mut BTreeSet<String>) {
        if value["type"] == "Identifier" {
            if let Some(name) = value["value"].as_str() {
                locals.insert(name.into());
            }
        } else if let Some(object) = value.as_object() {
            for child in object.values() {
                binding(child, locals);
            }
        } else if let Some(array) = value.as_array() {
            for child in array {
                binding(child, locals);
            }
        }
    }
    let mut locals: BTreeSet<String> = [
        "root",
        "window",
        "document",
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
        "undefined",
        "NaN",
        "Infinity",
        "setTimeout",
        "clearTimeout",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "localStorage",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let mut functions = BTreeSet::new();
    let mut data_variables = BTreeSet::new();
    let mut pending = vec![tree];
    while let Some(value) = pending.pop() {
        if let Some(object) = value.as_object() {
            match value["type"].as_str().unwrap_or("") {
                "VariableDeclarator" => binding(&value["id"], &mut locals),
                "FunctionDeclaration" | "FunctionExpression" => {
                    binding(&value["identifier"], &mut locals)
                }
                _ => (),
            }
            if value["type"] == "FunctionDeclaration" {
                if let Some(name) = value["identifier"]["value"].as_str() {
                    functions.insert(name.to_owned());
                }
            }
            if value["type"] == "VariableDeclarator"
                && matches!(
                    value["init"]["type"].as_str(),
                    Some("ArrowFunctionExpression" | "FunctionExpression")
                )
            {
                if let Some(name) = value["id"]["value"].as_str() {
                    functions.insert(name.to_owned());
                }
            }
            if value["type"] == "VariableDeclarator" {
                let init = &value["init"];
                let method = init["callee"]["property"]["value"].as_str().unwrap_or("");
                if matches!(
                    init["type"].as_str(),
                    Some("ArrayExpression" | "ObjectExpression")
                ) || init["type"] == "CallExpression"
                    && [
                        "querySelectorAll",
                        "from",
                        "map",
                        "filter",
                        "split",
                        "slice",
                    ]
                    .contains(&method)
                {
                    if let Some(name) = value["id"]["value"].as_str() {
                        data_variables.insert(name.to_owned());
                    }
                }
            }
            if let Some(params) = value["params"].as_array() {
                for param in params {
                    binding(param.get("pat").unwrap_or(param), &mut locals);
                }
            }
            pending.extend(object.values());
        } else if let Some(array) = value.as_array() {
            pending.extend(array);
        }
    }
    let forbidden = [
        "constructor",
        "prototype",
        "__proto__",
        "eval",
        "Function",
        "AsyncFunction",
        "GeneratorFunction",
        "fetch",
        "XMLHttpRequest",
        "WebSocket",
        "sendBeacon",
        "navigator",
        "parent",
        "opener",
        "frames",
        "globalThis",
        "self",
        "ownerDocument",
        "defaultView",
        "nextSibling",
        "previousSibling",
        "nextElementSibling",
        "previousElementSibling",
        "before",
        "after",
        "insertAdjacentElement",
        "parentNode",
        "parentElement",
        "getRootNode",
        "closest",
        "cookie",
        "write",
        "writeln",
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "createContextualFragment",
        "srcdoc",
        "contentWindow",
        "contentDocument",
        "location",
        "view",
        "path",
        "composedPath",
        "createElementNS",
        "setAttributeNS",
        "getElementById",
        "getElementsByTagName",
        "getElementsByClassName",
        "body",
        "documentElement",
    ];
    let mut pending = vec![(tree, false)];
    while let Some((value, called)) = pending.pop() {
        if let Some(object) = value.as_object() {
            let kind = value["type"].as_str().unwrap_or("");
            ensure(
                !matches!(
                    kind,
                    "Import"
                        | "ImportDeclaration"
                        | "ThisExpression"
                        | "WithStatement"
                        | "Super"
                        | "MetaProperty"
                        | "TaggedTemplateExpression"
                ) && (kind != "NewExpression" || value["callee"]["value"] == "Date"),
                "This script tries to load or run unsafe code.",
            )?;
            if kind == "Identifier" {
                let name = value["value"].as_str().unwrap_or("");
                ensure(
                    locals.contains(name) && !forbidden.contains(&name),
                    "This script reaches outside its box.",
                )?;
                if [
                    "window",
                    "document",
                    "localStorage",
                    "Math",
                    "JSON",
                    "Array",
                    "Date",
                    "console",
                    "setTimeout",
                ]
                .contains(&name)
                {
                    ensure(called, "This script cannot share access outside its box.")?;
                }
            }
            if kind == "MemberExpression" {
                let property = &value["property"];
                let computed = property["type"] == "Computed";
                let name = if computed && property["expression"]["type"] == "StringLiteral" {
                    property["expression"]["value"].as_str()
                } else if computed {
                    None
                } else {
                    property["value"].as_str()
                }
                .unwrap_or("");
                let receiver = value["object"]["value"].as_str().unwrap_or("");
                ensure(
                    !forbidden.contains(&name),
                    "This script reaches outside its box.",
                )?;
                if ["window", "document"].contains(&receiver) {
                    ensure(
                        !computed
                            && [
                                "addEventListener",
                                "removeEventListener",
                                "scrollY",
                                "innerWidth",
                                "innerHeight",
                            ]
                            .contains(&name)
                            || !computed
                                && receiver == "window"
                                && [
                                    "setTimeout",
                                    "clearTimeout",
                                    "requestAnimationFrame",
                                    "cancelAnimationFrame",
                                ]
                                .contains(&name)
                            || !computed
                                && receiver == "document"
                                && ["createElement", "createTextNode"].contains(&name),
                        "This script reaches outside its box.",
                    )?;
                }
                if receiver == "localStorage" {
                    ensure(
                        !computed && called && ["getItem", "setItem", "removeItem"].contains(&name),
                        "This script tries to use storage outside its box.",
                    )?;
                }
                if computed {
                    let expr = &property["expression"];
                    let data = value["object"]["property"]["value"]
                        .as_str()
                        .is_some_and(|p| ["dataset", "style"].contains(&p));
                    // Numeric indexing and data keys are safe; unknown DOM method names are not.
                    ensure(
                        data || !name.is_empty()
                            || expr["type"] == "NumericLiteral"
                            || !called
                                && expr["type"] == "Identifier"
                                && data_variables.contains(receiver),
                        "This script uses an unknown action outside its box.",
                    )?;
                    pending.push((expr, false));
                }
                pending.push((&value["object"], true));
            } else {
                if kind == "CallExpression" {
                    let callee = &value["callee"];
                    let name = callee["property"]["value"]
                        .as_str()
                        .or(callee["property"]["expression"]["value"].as_str())
                        .or(callee["value"].as_str())
                        .unwrap_or("");
                    if name == "setTimeout" {
                        ensure(
                            matches!(
                                value["arguments"][0]["expression"]["type"].as_str(),
                                Some("ArrowFunctionExpression" | "FunctionExpression")
                            ) || value["arguments"][0]["expression"]["value"]
                                .as_str()
                                .is_some_and(|name| functions.contains(name)),
                            "This script tries to run text as code.",
                        )?;
                    }
                    if ["getItem", "setItem", "removeItem"].contains(&name) {
                        ensure(
                            value["arguments"][0]["expression"]["value"]
                                .as_str()
                                .is_some_and(|key| key.starts_with("desegnos:") && key.len() > 9),
                            "This script tries to use storage outside its box.",
                        )?;
                    }
                    if name == "setAttribute" {
                        ensure(
                            value["arguments"][0]["expression"]["value"]
                                .as_str()
                                .is_some_and(|attribute| {
                                    let attribute = attribute.to_ascii_lowercase();
                                    !attribute.starts_with("on")
                                        && !["srcdoc", "src", "href", "type", "is", "formaction"]
                                            .contains(&attribute.as_str())
                                }),
                            "This script tries to add unsafe content.",
                        )?;
                    }
                    if name == "createElement" {
                        ensure(
                            value["arguments"][0]["expression"]["value"]
                                .as_str()
                                .is_some_and(|tag| {
                                    ![
                                        "script", "iframe", "object", "embed", "link", "base",
                                        "style", "meta",
                                    ]
                                    .contains(&tag.to_ascii_lowercase().as_str())
                                }),
                            "This script tries to add unsafe content.",
                        )?;
                    }
                    if ["addEventListener", "removeEventListener"].contains(&name)
                        && ["window", "document"]
                            .contains(&callee["object"]["value"].as_str().unwrap_or(""))
                    {
                        ensure(
                            value["arguments"][0]["expression"]["value"]
                                .as_str()
                                .is_some_and(|event| {
                                    [
                                        "scroll",
                                        "resize",
                                        "pointermove",
                                        "pointerdown",
                                        "pointerup",
                                        "pointercancel",
                                        "keydown",
                                        "keyup",
                                        "keypress",
                                    ]
                                    .contains(&event)
                                }),
                            "This script listens outside its box.",
                        )?;
                    }
                }
                pending.extend(
                    object
                        .iter()
                        .filter(|(key, child)| {
                            !["params", "id", "identifier"].contains(&key.as_str())
                                && !(key.as_str() == "key"
                                    && matches!(
                                        kind,
                                        "KeyValueProperty"
                                            | "MethodProperty"
                                            | "GetterProperty"
                                            | "SetterProperty"
                                    )
                                    && child["type"] != "Computed")
                        })
                        .map(|(key, child)| (child, key == "callee")),
                );
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
    setTimeout(callback, delay) {{ return typeof callback === 'function' ? setTimeout(callback, delay) : undefined; }},
    clearTimeout(id) {{ clearTimeout(id); }},
    requestAnimationFrame(callback) {{ return requestAnimationFrame(callback); }},
    cancelAnimationFrame(id) {{ cancelAnimationFrame(id); }},
    addEventListener(type, callback) {{
      if (!['scroll', 'resize', 'pointermove', 'pointerdown', 'pointerup', 'pointercancel', 'keydown', 'keyup', 'keypress'].includes(type)) return;
      const handler = event => {{ try {{ callback(Object.freeze({{type, clientX:event.clientX, clientY:event.clientY, key:event.key, code:event.code, preventDefault:() => event.preventDefault(), target:root, currentTarget:root}})); }} catch (error) {{ console.error('Region script event failed', error); }} }};
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
  const scopedDocument = Object.freeze({{
    addEventListener: scopedWindow.addEventListener,
    removeEventListener: scopedWindow.removeEventListener,
    createTextNode(text) {{ return document.createTextNode(text); }},
    createElement(tag) {{
      if (typeof tag !== 'string' || ['script', 'iframe', 'object', 'embed', 'link', 'base', 'style', 'meta'].includes(tag.toLowerCase())) throw new Error('Unsafe region element');
      return document.createElement(tag);
    }}
  }});
  try {{ (function(root, window, document, setTimeout, requestAnimationFrame) {{ 'use strict';
{code}
  }})(root, scopedWindow, scopedDocument, scopedWindow.setTimeout, scopedWindow.requestAnimationFrame); }} catch (error) {{ console.error('Region script failed', error); }}
}})();
"#));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    fn check(code: &str) -> crate::Result<()> {
        let doc = crate::editing::parse(
            "<section><div class='shelf'><article data-date='2026-10-22'></article></div></section>",
        )?;
        let root = doc.select("section").nodes()[0];
        super::validate_code(code, root)
    }

    #[test]
    fn region_scripts_autosort_by_date() {
        check(r#"
            const field = 'date';
            const directions = { date: 1, name: -1 };
            const direction = directions[field];
            const selector = '.' + 'shelf';
            const shelf = root.querySelector(selector);
            const cards = Array.from(root.querySelectorAll('article'));
            cards.sort((a, b) => Date.parse(b.dataset[field]) - new Date(a.getAttribute('data-date')).getTime());
            cards.forEach(card => shelf.appendChild(card));
            shelf.insertBefore(cards[0], cards[1]);
            setTimeout(() => root.classList.add('sorted'), 20);
            requestAnimationFrame(() => root.classList.remove('sorting'));
            root.addEventListener('click', () => root.classList.toggle('active'));
            window.addEventListener('resize', () => root.classList.add('resized'));
            document.addEventListener('keydown', event => root.dataset.key = event.key);
            localStorage.setItem('desegnos:shelf-order', 'date');
            const label = document.createElement('span');
            label.textContent = 'Sorted';
            root.appendChild(label);
            function finish() { root.classList.add('finished'); }
            window.setTimeout(finish, 5);
            window.requestAnimationFrame(finish);
            root.style.top = root.getBoundingClientRect().top + 'px';
            root.setAttribute('data-sorted', 'true');
        "#).unwrap();
    }

    #[test]
    fn region_scripts_blocks_unsafe_capabilities() {
        for code in [
            "eval('1')",
            "new Function('return 1')",
            "setTimeout('alert(1)', 1)",
            "import('remote')",
            "import x from 'remote'",
            "fetch('/local')",
            "fetch('https://remote')",
            "new XMLHttpRequest()",
            "new WebSocket('wss://remote')",
            "navigator.sendBeacon('/local', 'x')",
            "document.write('x')",
            "document.cookie",
            "localStorage.getItem('other')",
            "localStorage.clear()",
            "top.location",
            "parent.location",
            "opener.location",
            "root.ownerDocument",
            "root.parentElement",
            "root.innerHTML = '<script>bad()</script>'",
            "document.createElement('script')",
            "document.createElement('iframe')",
            "root.constructor.constructor('return window')()",
            "window['fetch']('/local')",
            "document.querySelector('body')",
            "const alias = root; const key = 'parentElement'; alias[key]",
            "root.setAttribute('onclick', 'eval(1)')",
        ] {
            assert!(check(code).is_err(), "accepted: {code}");
        }
    }
}
