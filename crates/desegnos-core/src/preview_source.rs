//! Disposable provenance for a scripted preview. Never store this markup in a project.
use crate::Result;
use uuid::Uuid;

/// Bind rendered nodes to source identities before author scripts see the document.
/// Files without editor IDs keep a source selector and hash instead of acquiring IDs.
pub fn preview_source_document(html: &str) -> Result<String> {
    let doc = crate::editing::parse_document(html, true)?;
    let body = crate::editing::body(&doc)?;
    let nodes: Vec<_> = std::iter::once(body)
        .chain(body.descendants_it().filter(|node| node.is_element()))
        .collect();
    for node in nodes {
        let source = if let Some(id) = node.attr(crate::stable_targets::ELEMENT_ID_ATTRIBUTE) {
            format!("id:{id}")
        } else if let Some(selector) = crate::editing::selector(node, body) {
            format!("selector:{selector}")
        } else {
            continue;
        };
        node.set_attr(
            "data-desegnos-preview-key",
            &Uuid::new_v4().simple().to_string(),
        );
        node.set_attr("data-desegnos-preview-source", &source);
    }
    crate::editing::serialized(&doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_covers_idless_source_and_does_not_change_editor_ids() {
        let source = "<html><head></head><body><button data-desegnos-id=\"button01\">Source</button><p>Plain</p></body></html>";
        let annotated = preview_source_document(source).unwrap();
        let doc = dom_query::Document::from(annotated);
        assert_eq!(
            doc.select("button")
                .attr("data-desegnos-preview-source")
                .as_deref(),
            Some("id:button01")
        );
        assert_eq!(
            doc.select("p")
                .attr("data-desegnos-preview-source")
                .as_deref(),
            Some("selector:body > p:nth-of-type(1)")
        );
        assert!(!doc.select("p").has_attr("data-desegnos-id"));
        let keys: std::collections::BTreeSet<_> = doc
            .select("[data-desegnos-preview-key]")
            .nodes()
            .iter()
            .map(|node| node.attr("data-desegnos-preview-key").unwrap().to_string())
            .collect();
        assert_eq!(keys.len(), 3);
        assert!(!source.contains("data-desegnos-preview-"));
    }
}
