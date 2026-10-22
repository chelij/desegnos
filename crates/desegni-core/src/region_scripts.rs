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
    validate_write_boundary(&tree)?;
    validate_script_tree(&tree)?;
    let _ = root; // Ownership and nested-region boundaries are checked by validate above.

    Ok(())
}

const GLOBAL_READ_EVENTS: &[&str] = &[
    "pointermove",
    "pointerdown",
    "pointerup",
    "pointercancel",
    "mousemove",
    "mousedown",
    "mouseup",
    "click",
    "touchstart",
    "touchmove",
    "touchend",
    "wheel",
    "scroll",
    "resize",
    "keydown",
    "keyup",
    "keypress",
    "visibilitychange",
    "focus",
    "blur",
    "orientationchange",
];

fn member_name(value: &serde_json::Value) -> &str {
    value["property"]["value"]
        .as_str()
        .or(value["property"]["expression"]["value"].as_str())
        .unwrap_or("")
}

/// Track global-derived references separately from local DOM/data. The runtime also enforces
/// this boundary, including aliases passed through callbacks and dynamically selected nodes.
fn validate_write_boundary(tree: &serde_json::Value) -> Result<()> {
    use serde_json::Value;
    use std::collections::BTreeSet;
    fn outside(v: &Value, aliases: &BTreeSet<String>, guarded: &BTreeSet<String>) -> bool {
        match v["type"].as_str().unwrap_or("") {
            "Identifier" => v["value"].as_str().is_some_and(|n| aliases.contains(n)),
            "MemberExpression" => {
                if member_name(v) == "currentTarget" {
                    return true;
                }
                if member_name(v) == "target" {
                    return !v["object"]["value"]
                        .as_str()
                        .is_some_and(|n| guarded.contains(n));
                }
                if [
                    "clientX",
                    "clientY",
                    "pageX",
                    "pageY",
                    "innerWidth",
                    "innerHeight",
                    "scrollX",
                    "scrollY",
                    "visibilityState",
                    "hidden",
                    "matches",
                    "media",
                ]
                .contains(&member_name(v))
                {
                    return false;
                }
                outside(&v["object"], aliases, guarded)
            }
            "CallExpression" => {
                let callee = &v["callee"];
                if ["createElement", "createTextNode"].contains(&member_name(callee))
                    && callee["object"]["value"] == "document"
                {
                    return false;
                }
                if [
                    "getBoundingClientRect",
                    "getClientRects",
                    "getAttribute",
                    "hasAttribute",
                    "matches",
                    "contains",
                    "getPropertyValue",
                    "getPropertyPriority",
                ]
                .contains(&member_name(callee))
                {
                    return false;
                }
                if callee["object"]["value"] == "Array" && member_name(callee) == "from" {
                    return outside(&v["arguments"][0]["expression"], aliases, guarded);
                }
                member_name(callee) == "closest" || outside(callee, aliases, guarded)
            }
            "ParenthesisExpression" | "OptionalChainingExpression" => {
                outside(&v["expression"], aliases, guarded) || outside(&v["base"], aliases, guarded)
            }
            "ConditionalExpression" => {
                outside(&v["consequent"], aliases, guarded)
                    || outside(&v["alternate"], aliases, guarded)
            }
            "ObjectExpression" => v["properties"].as_array().into_iter().flatten().any(|p| {
                outside(&p["value"], aliases, guarded)
                    || outside(&p["expression"], aliases, guarded)
            }),
            "ArrayExpression" => v["elements"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|p| outside(&p["expression"], aliases, guarded)),
            _ => false,
        }
    }
    fn target_guard(v: &Value) -> Option<&str> {
        if v["type"] == "ParenthesisExpression" {
            return target_guard(&v["expression"]);
        }
        let callee = &v["callee"];
        let target = &v["arguments"][0]["expression"];
        (v["type"] == "CallExpression"
            && callee["object"]["value"] == "root"
            && member_name(callee) == "contains"
            && member_name(target) == "target")
            .then(|| target["object"]["value"].as_str())
            .flatten()
    }
    fn exits(v: &Value) -> bool {
        v["type"] == "ReturnStatement"
            || v["type"] == "ThrowStatement"
            || v["stmts"].is_array()
                && v["stmts"]
                    .as_array()
                    .is_some_and(|s| s.len() == 1 && exits(&s[0]))
    }
    fn taint_binding(v: &Value, aliases: &mut BTreeSet<String>) {
        if v["type"] == "Identifier" {
            if let Some(name) = v["value"].as_str() {
                aliases.insert(name.into());
            }
        } else if let Some(o) = v.as_object() {
            for (key, child) in o {
                if key != "key" {
                    taint_binding(child, aliases);
                }
            }
        } else if let Some(a) = v.as_array() {
            for child in a {
                taint_binding(child, aliases);
            }
        }
    }
    let mut functions = BTreeMap::new();
    let mut pending = vec![tree];
    while let Some(v) = pending.pop() {
        if v["type"] == "FunctionDeclaration"
            && let Some(name) = v["identifier"]["value"].as_str()
        {
            functions.insert(name, v);
        }
        if v["type"] == "VariableDeclarator"
            && matches!(
                v["init"]["type"].as_str(),
                Some("ArrowFunctionExpression" | "FunctionExpression")
            )
            && let Some(name) = v["id"]["value"].as_str()
        {
            functions.insert(name, &v["init"]);
        }
        if let Some(o) = v.as_object() {
            pending.extend(o.values());
        }
        if let Some(a) = v.as_array() {
            pending.extend(a);
        }
    }
    let mut aliases = BTreeSet::from(["window".into(), "document".into()]);
    let empty = BTreeSet::new();
    loop {
        let before = aliases.len();
        for (name, function) in &functions {
            if outside(&function["body"], &aliases, &empty) {
                aliases.insert((*name).into());
            }
            let mut returns = vec![&function["body"]];
            while let Some(v) = returns.pop() {
                if v["type"] == "ReturnStatement" && outside(&v["argument"], &aliases, &empty) {
                    aliases.insert((*name).into());
                }
                if let Some(o) = v.as_object() {
                    returns.extend(o.values());
                }
                if let Some(a) = v.as_array() {
                    returns.extend(a);
                }
            }
        }
        let mut pending = vec![tree];
        while let Some(v) = pending.pop() {
            let (id, init) = if v["type"] == "VariableDeclarator" {
                (&v["id"], &v["init"])
            } else if v["type"] == "AssignmentExpression" {
                (&v["left"], &v["right"])
            } else {
                (&Value::Null, &Value::Null)
            };
            if outside(init, &aliases, &empty) {
                taint_binding(id, &mut aliases);
            }
            if v["type"] == "CallExpression"
                && let Some(function) = v["callee"]["value"]
                    .as_str()
                    .and_then(|name| functions.get(name))
            {
                for (index, arg) in v["arguments"].as_array().into_iter().flatten().enumerate() {
                    if outside(&arg["expression"], &aliases, &empty) {
                        taint_binding(&function["params"][index], &mut aliases);
                    }
                }
            }
            if v["type"] == "CallExpression"
                && outside(&v["callee"]["object"], &aliases, &empty)
                && ["forEach", "map", "filter", "find", "some", "every"]
                    .contains(&member_name(&v["callee"]))
            {
                let callback = &v["arguments"][0]["expression"];
                let callback = callback["value"]
                    .as_str()
                    .and_then(|name| functions.get(name))
                    .copied()
                    .unwrap_or(callback);
                taint_binding(&callback["params"][0], &mut aliases);
            }
            if v["type"] == "ForOfStatement"
                && outside(&v["right"], &aliases, &empty)
                && let Some(name) = v["left"]["declarations"][0]["id"]["value"].as_str()
            {
                aliases.insert(name.into());
            }
            if let Some(o) = v.as_object() {
                pending.extend(o.values());
            }
            if let Some(a) = v.as_array() {
                pending.extend(a);
            }
        }
        if aliases.len() == before {
            break;
        }
    }
    fn visit(v: &Value, aliases: &BTreeSet<String>, guarded: &BTreeSet<String>) -> Result<()> {
        let kind = v["type"].as_str().unwrap_or("");
        if kind == "MemberExpression"
            && member_name(v) == "top"
            && outside(&v["object"], aliases, guarded)
        {
            return Err(crate::Error("This script reaches outside its box.".into()));
        }
        if [
            "AssignmentExpression",
            "UpdateExpression",
            "UnaryExpression",
        ]
        .contains(&kind)
        {
            let target = if kind == "AssignmentExpression" {
                &v["left"]
            } else {
                &v["argument"]
            };
            if kind != "UnaryExpression" || v["operator"] == "delete" {
                ensure(
                    !outside(target, aliases, guarded)
                        && !(target["type"] == "MemberExpression"
                            && outside(&target["object"], aliases, guarded)),
                    "This script changes things outside its box.",
                )?;
            }
        }
        if kind == "CallExpression" {
            let callee = &v["callee"];
            let name = member_name(callee);
            if [
                "preventDefault",
                "stopPropagation",
                "stopImmediatePropagation",
            ]
            .contains(&name)
            {
                ensure(
                    callee["object"]["value"]
                        .as_str()
                        .is_some_and(|n| guarded.contains(n)),
                    "This script blocks events outside its box.",
                )?;
            } else if outside(&callee["object"], aliases, guarded) {
                ensure(
                    [
                        "querySelector",
                        "querySelectorAll",
                        "getElementById",
                        "getElementsByTagName",
                        "getElementsByClassName",
                        "getBoundingClientRect",
                        "getClientRects",
                        "closest",
                        "matches",
                        "contains",
                        "getAttribute",
                        "hasAttribute",
                        "item",
                        "entries",
                        "keys",
                        "values",
                        "forEach",
                        "map",
                        "filter",
                        "find",
                        "some",
                        "every",
                        "slice",
                        "includes",
                        "indexOf",
                        "toString",
                        "getPropertyValue",
                        "getPropertyPriority",
                        "addEventListener",
                        "removeEventListener",
                        "matchMedia",
                        "createElement",
                        "createTextNode",
                        "setTimeout",
                        "clearTimeout",
                        "requestAnimationFrame",
                        "cancelAnimationFrame",
                    ]
                    .contains(&name),
                    if [
                        "setAttribute",
                        "removeAttribute",
                        "toggleAttribute",
                        "append",
                        "appendChild",
                        "prepend",
                        "insertBefore",
                        "replaceChild",
                        "replaceChildren",
                        "remove",
                        "replaceWith",
                        "add",
                        "toggle",
                        "replace",
                        "setProperty",
                        "removeProperty",
                        "focus",
                        "blur",
                        "click",
                        "dispatchEvent",
                        "scroll",
                        "scrollTo",
                        "scrollBy",
                    ]
                    .contains(&name)
                    {
                        "This script changes things outside its box."
                    } else {
                        "This script uses an unsupported action."
                    },
                )?;
            }
            if ["addEventListener", "removeEventListener"].contains(&name)
                && outside(&callee["object"], aliases, guarded)
            {
                ensure(
                    v["arguments"][0]["expression"]["value"]
                        .as_str()
                        .is_some_and(|event| {
                            GLOBAL_READ_EVENTS.contains(&event) || event == "change"
                        }),
                    "This script uses an unsupported global event.",
                )?;
            }
            // Moving a node out of the rest of the page is an outside write too.
            if [
                "append",
                "appendChild",
                "prepend",
                "insertBefore",
                "replaceChild",
                "replaceChildren",
            ]
            .contains(&name)
            {
                for arg in v["arguments"].as_array().into_iter().flatten() {
                    ensure(
                        !outside(&arg["expression"], aliases, guarded),
                        "This script changes things outside its box.",
                    )?;
                }
            }
            if name == "addEventListener" && !outside(&callee["object"], aliases, guarded) {
                let callback = &v["arguments"][1]["expression"];
                let param = &callback["params"][0];
                let mut local = guarded.clone();
                if let Some(n) = param["value"].as_str().or(param["pat"]["value"].as_str()) {
                    local.insert(n.into());
                }
                visit(&callback["body"], aliases, &local)?;
                // Other call children still require normal validation.
                for (key, child) in v.as_object().unwrap() {
                    if key != "arguments" {
                        visit(child, aliases, guarded)?;
                    }
                }
                for (index, arg) in v["arguments"].as_array().into_iter().flatten().enumerate() {
                    if index != 1 {
                        visit(arg, aliases, guarded)?;
                    }
                }
                return Ok(());
            }
        }
        if kind == "IfStatement" {
            visit(&v["test"], aliases, guarded)?;
            let mut inside = guarded.clone();
            if let Some(n) = target_guard(&v["test"]) {
                inside.insert(n.into());
            }
            visit(&v["consequent"], aliases, &inside)?;
            visit(&v["alternate"], aliases, guarded)?;
            return Ok(());
        }
        if kind == "BinaryExpression" && v["operator"] == "&&" {
            visit(&v["left"], aliases, guarded)?;
            let mut inside = guarded.clone();
            if let Some(n) = target_guard(&v["left"]) {
                inside.insert(n.into());
            }
            return visit(&v["right"], aliases, &inside);
        }
        if v["stmts"].is_array() {
            let mut inside = guarded.clone();
            for stmt in v["stmts"].as_array().into_iter().flatten() {
                visit(stmt, aliases, &inside)?;
                if stmt["type"] == "IfStatement"
                    && stmt["test"]["operator"] == "!"
                    && exits(&stmt["consequent"])
                    && stmt["alternate"].is_null()
                    && let Some(n) = target_guard(&stmt["test"]["argument"])
                {
                    inside.insert(n.into());
                }
            }
            return Ok(());
        }
        if let Some(o) = v.as_object() {
            for child in o.values() {
                visit(child, aliases, guarded)?;
            }
        }
        if let Some(a) = v.as_array() {
            for child in a {
                visit(child, aliases, guarded)?;
            }
        }
        Ok(())
    }
    visit(tree, &aliases, &empty)
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
            if value["type"] == "FunctionDeclaration"
                && let Some(name) = value["identifier"]["value"].as_str()
            {
                functions.insert(name.to_owned());
            }
            if value["type"] == "VariableDeclarator"
                && matches!(
                    value["init"]["type"].as_str(),
                    Some("ArrowFunctionExpression" | "FunctionExpression")
                )
                && let Some(name) = value["id"]["value"].as_str()
            {
                functions.insert(name.to_owned());
            }
            if value["type"] == "VariableDeclarator" {
                let init = &value["init"];
                let method = init["callee"]["property"]["value"].as_str().unwrap_or("");
                if (matches!(
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
                    .contains(&method))
                    && let Some(name) = value["id"]["value"].as_str()
                {
                    data_variables.insert(name.to_owned());
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
                    "Number",
                    "String",
                    "Boolean",
                    "Array",
                    "Date",
                    "console",
                    "parseInt",
                    "parseFloat",
                    "isNaN",
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
                let mut base = &value["object"];
                while base["type"] == "MemberExpression" {
                    base = &base["object"];
                }
                // A chained property still belongs to the shared native object.
                let native = [
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
                .contains(&base["value"].as_str().unwrap_or(""));
                ensure(
                    !native || called,
                    "Region script cannot alias or change shared native objects.",
                )?;
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
                                "scrollX",
                                "scrollY",
                                "matchMedia",
                                "body",
                                "documentElement",
                                "visibilityState",
                                "hidden",
                                "querySelector",
                                "querySelectorAll",
                                "getElementById",
                                "getElementsByTagName",
                                "getElementsByClassName",
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
                                .is_some_and(|event| GLOBAL_READ_EVENTS.contains(&event)),
                            "This script uses an unsupported global event.",
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
        output.push_str(
            &include_str!("region_script_runtime.js")
                .replace("__REGION_SELECTOR__", &selector)
                .replace(
                    "__GLOBAL_READ_EVENTS__",
                    &serde_json::to_string(GLOBAL_READ_EVENTS).unwrap(),
                )
                .replace("__REGION_CODE__", &code),
        );
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
    fn global_reads_and_guarded_events_are_allowed() {
        for receiver in [
            "window",
            "document",
            "document.documentElement",
            "document.body",
        ] {
            for event in super::GLOBAL_READ_EVENTS {
                check(&format!("{receiver}.addEventListener('{event}', e => {{ root.dataset.x = e.pageX; root.dataset.y = e.pageY; }});" )).unwrap();
            }
        }
        for code in [
            "const box = document.querySelector('p').getBoundingClientRect(); root.style.left = box.left + 'px';",
            "const outside = root.closest('main'); const matches = outside.matches('main'); root.dataset.matches = matches;",
            "root.dataset.dark = window.matchMedia('(prefers-color-scheme: dark)').matches;",
            "root.style.left = window.scrollX + window.scrollY + 'px';",
            "document.addEventListener('click', e => { if (root.contains(e.target)) { e.preventDefault(); e.stopPropagation(); } });",
            "document.body.addEventListener('touchmove', e => { if (!root.contains(e.target)) return; e.preventDefault(); });",
            "window.addEventListener('wheel', e => root.contains(e.target) && e.preventDefault());",
            "root.addEventListener('click', e => e.preventDefault());",
            "root.querySelector('button').addEventListener('click', e => e.preventDefault());",
            "const width = window.innerWidth.toString(); root.dataset.width = width;",
            "const color = document.body.style.getPropertyValue('color'); root.style.color = color;",
            "document.addEventListener('click', e => { root.dataset.hit = e.target.closest('button')?.matches('.active'); });",
        ] {
            check(code).unwrap_or_else(|err| panic!("{code}: {err}"));
        }
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
            "const alias = root; const key = 'parentElement'; alias[key]",
            "root.setAttribute('onclick', 'eval(1)')",
            "Math.random = () => 0",
            "Math.random.call = () => 0",
            "const shared = Number; shared.parseInt = () => 0",
            "parseInt.extra = root",
        ] {
            assert!(check(code).is_err(), "accepted: {code}");
        }
    }
}
