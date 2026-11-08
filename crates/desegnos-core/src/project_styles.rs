use crate::{
    Result, elements::Element, model::Project, model::ProjectStyles, model::ReferenceKind,
    validation::ensure,
};
use uuid::Uuid;

impl Project {
    pub(crate) fn detach_style(&mut self, id: Uuid, text: bool) -> Result<()> {
        ensure(
            if text {
                self.styles.text.iter().any(|s| s.id == id)
            } else {
                self.styles.colours.iter().any(|s| s.id == id)
            },
            "This style no longer exists.",
        )?;
        fn detach(styles: &ProjectStyles, elements: &mut [Element], id: Uuid) {
            for element in elements {
                for style in std::iter::once(&mut element.style).chain(
                    element
                        .overrides
                        .values_mut()
                        .filter_map(|o| o.style.as_mut()),
                ) {
                    styles.resolve(style);
                    for reference in [
                        &mut style.references.fill,
                        &mut style.references.stroke,
                        &mut style.references.text_colour,
                        &mut style.references.text,
                    ] {
                        if *reference == Some(id) {
                            *reference = None;
                        }
                    }
                }
                detach(styles, &mut element.children, id);
            }
        }
        for page in &mut self.pages {
            detach(&self.styles, &mut page.elements, id);
        }
        for item in &mut self.reference {
            if let ReferenceKind::Element { element, .. } = &mut item.kind {
                detach(&self.styles, std::slice::from_mut(element.as_mut()), id);
            }
        }
        for definition in self.linked.values_mut() {
            detach(
                &self.styles,
                std::slice::from_mut(&mut definition.master),
                id,
            );
        }
        Ok(())
    }
}
