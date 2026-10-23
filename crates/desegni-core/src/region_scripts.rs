//! Request-owned inert source blocks. Persisting in HTML gives source editing and history the
//! same ownership/removal/Undo semantics as the region's other children; no sidecar can go stale.
use crate::{GenerationCandidate, Project, RequirementsSnapshot, Result, ensure};
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
                parse_code(script.text().as_ref())?;
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_code(code: &str) -> Result<()> {
    let wrapped = format!(
        "(function(root, window, document, setTimeout, requestAnimationFrame){{ 'use strict';\n{code}\n}})"
    );
    let map = swc_common::SourceMap::default();
    let file = map.new_source_file(
        swc_common::FileName::Custom("region.js".into()).into(),
        wrapped,
    );
    let mut errors = vec![];
    swc_ecma_parser::parse_file_as_script(
        &file,
        swc_ecma_parser::Syntax::Es(Default::default()),
        swc_ecma_ast::EsVersion::latest(),
        None,
        &mut errors,
    )
    .map_err(|_| crate::Error("Region script cannot be parsed.".into()))?;
    ensure(errors.is_empty(), "Region script cannot be parsed.")
}

/// Compile accepted source into ordinary JS functions, never eval/Function. Use a separate
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
                .replace("__REGION_CODE__", &code),
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    fn check(code: &str) -> crate::Result<()> {
        super::parse_code(code)
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
            for event in ["pointermove", "resize", "custom-event"] {
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
    fn region_scripts_allow_page_apis_and_arbitrary_events() {
        for code in [
            "document.addEventListener('custom-event', event => event.preventDefault());",
            "document.querySelector('p').remove();",
            "root.parentElement.classList.add('changed');",
            "localStorage.clear(); sessionStorage.setItem('key', 'value');",
            "fetch('/local'); import('remote');",
            "eval('1'); new Function('return 1')();",
            "document.cookie; top.location; parent.location; opener.location;",
            "document.createElement('script'); document.createElement('iframe');",
            "root.setAttribute('onclick', 'alert(1)');",
            "root.innerHTML = '<script>run()</script>';",
            "Math.random = () => 0;",
        ] {
            check(code).unwrap_or_else(|err| panic!("{code}: {err}"));
        }
    }

    #[test]
    fn region_scripts_must_parse_as_function_bodies() {
        check("return; const value = 1;").unwrap();
        for code in [
            "const =",
            "if (",
            "import x from 'remote'",
            "const document = {};",
            "} window.changed = true; (function() {",
        ] {
            assert!(check(code).is_err(), "accepted: {code}");
        }
    }
}
