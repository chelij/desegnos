use crate::model::normalized_swatch;
use crate::model::{attachment_id, reference_picture_name};
use crate::*;
use std::collections::BTreeSet;
use uuid::Uuid;

impl Project {
    pub(crate) fn reference_mut(&mut self, id: Uuid) -> Result<&mut ReferenceItem> {
        self.reference
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| Error("This reference item no longer exists.".into()))
    }

    pub(crate) fn place_reference(&mut self, placements: Vec<ReferencePlacement>) -> Result<()> {
        for placement in placements {
            let item = self.reference_mut(placement.item_id)?;
            item.rect.x = placement.x;
            item.rect.y = placement.y;
        }
        Ok(())
    }

    pub(crate) fn add_reference_item(
        &mut self,
        mut item: ReferenceItem,
        picture_data_uri: Option<String>,
    ) -> Result<()> {
        if let ReferenceKind::Swatch { color } = &mut item.kind {
            *color = normalized_swatch(color);
        }
        if let ReferenceKind::Image { picture, .. } = &mut item.kind {
            if let Some(uri) = picture_data_uri {
                let id = crate::project_assets::store_picture_uri(&uri)?;
                self.pictures.insert(id, std::sync::Arc::from(uri.as_str()));
                *picture = id;
            }
        } else {
            ensure(
                picture_data_uri.is_none(),
                "Only a reference picture takes picture bytes.",
            )?;
        }
        ensure(
            self.reference.len() < MAX_REFERENCE_ITEMS,
            "A reference holds at most 1,000 items.",
        )?;
        ensure(
            !self.reference.iter().any(|existing| existing.id == item.id),
            "This reference item already exists.",
        )?;
        self.reference.push(item);
        Ok(())
    }

    pub(crate) fn update_reference_item(
        &mut self,
        item_id: Uuid,
        rect: Option<Rect>,
        text: Option<String>,
        url: Option<String>,
        title: Option<String>,
        color: Option<String>,
    ) -> Result<()> {
        let has_content = text.is_some() || url.is_some() || title.is_some() || color.is_some();
        let item = self.reference_mut(item_id)?;
        if let Some(rect) = rect {
            if (rect.width != item.rect.width || rect.height != item.rect.height)
                && let ReferenceKind::Element { element, size } = &mut item.kind
            {
                let resized = Rect {
                    width: rect.width,
                    height: rect.height,
                    ..element.rect
                };
                if let Some(size) = size {
                    let original = element
                        .overrides
                        .get(size)
                        .and_then(|over| over.rect)
                        .unwrap_or(element.rect);
                    element.overrides.entry(*size).or_default().rect = Some(Rect {
                        width: rect.width,
                        height: rect.height,
                        ..original
                    });
                } else {
                    element.rect = resized;
                }
            }
            item.rect = rect;
        }
        if !has_content {
            return Ok(());
        }
        match &mut item.kind {
            ReferenceKind::Note { text: value } => {
                ensure(
                    url.is_none() && title.is_none() && color.is_none(),
                    "This reference item is not a link or a swatch.",
                )?;
                if let Some(next) = text {
                    *value = next;
                }
            }
            ReferenceKind::Link {
                url: link,
                title: name,
            } => {
                ensure(
                    text.is_none() && color.is_none(),
                    "This reference item is not a note or a swatch.",
                )?;
                if let Some(next) = url {
                    *link = next;
                }
                if let Some(next) = title {
                    *name = next;
                }
            }
            ReferenceKind::Swatch { color: value } => {
                ensure(
                    text.is_none() && url.is_none() && title.is_none(),
                    "This reference item is not a note or a link.",
                )?;
                if let Some(next) = color {
                    *value = normalized_swatch(&next);
                }
            }
            ReferenceKind::Image { .. } | ReferenceKind::Element { .. } => {
                return Err(Error(
                    "Move this reference layer into a boundary to edit its content.".into(),
                ));
            }
        }
        Ok(())
    }

    /// Moving a layer into reference preserves its complete editable tree. Requests follow
    /// their targets into reference, rather than turning layer names into prompts.
    pub(crate) fn element_to_reference(
        &mut self,
        page_id: Uuid,
        element_id: Uuid,
        rect: Rect,
        size_id: Option<Uuid>,
    ) -> Result<()> {
        ensure(
            rect.width >= 0.0 && rect.height >= 0.0,
            "Layer dimensions cannot be negative.",
        )?;
        let page = self.page_mut(page_id)?;
        ensure(
            size_id.is_none_or(|id| page.sizes.iter().any(|size| size.id == id)),
            "This boundary size no longer exists.",
        )?;
        let size_id = size_id.filter(|id| *id != page.base_size_id());
        let element = crate::elements::remove_element(&mut page.elements, element_id)
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        let item_id = Uuid::new_v4();
        fn contains(element: &crate::Element, id: Uuid) -> bool {
            element.id == id || element.children.iter().any(|child| contains(child, id))
        }
        for request in &mut self.requests {
            for target in &mut request.targets {
                if matches!(target, RequestTarget::Element { id, .. } if contains(&element, *id)) {
                    *target = RequestTarget::Reference { id: item_id };
                }
            }
            request.targets.dedup();
        }
        self.reference.push(ReferenceItem {
            id: item_id,
            rect: Rect {
                width: rect.width.max(1.0),
                height: rect.height.max(1.0),
                ..rect
            },
            kind: ReferenceKind::Element {
                element: Box::new(element),
                size: size_id,
            },
        });
        Ok(())
    }

    /// Selected reference items become request attachments on these pending requests.
    pub(crate) fn attach_reference_context(
        &mut self,
        request_ids: &[Uuid],
        item_ids: &[Uuid],
    ) -> Result<()> {
        ensure(!request_ids.is_empty(), "Write a request first.")?;
        let contexts: Vec<(
            String,
            crate::AttachmentKind,
            Option<String>,
            Option<String>,
        )> = item_ids
            .iter()
            .map(|id| {
                let item = self
                    .reference
                    .iter()
                    .find(|item| item.id == *id)
                    .ok_or_else(|| Error("This reference item no longer exists.".into()))?;
                Ok(match &item.kind {
                    ReferenceKind::Element { element, .. } => (
                        element.label(),
                        crate::AttachmentKind::Text,
                        Some(crate::requirements::reference_element_value(element).to_string()),
                        None,
                    ),
                    ReferenceKind::Note { text } => (
                        "Reference note".to_string(),
                        crate::AttachmentKind::Text,
                        Some(text.clone()),
                        None,
                    ),
                    ReferenceKind::Link { url, title } => (
                        if title.trim().is_empty() {
                            url.clone()
                        } else {
                            title.clone()
                        },
                        crate::AttachmentKind::Link,
                        None,
                        Some(url.clone()),
                    ),
                    // Reference pictures travel by name only; their bytes never leave the project.
                    ReferenceKind::Image { picture, name } => (
                        reference_picture_name(*picture, name),
                        crate::AttachmentKind::Text,
                        Some(reference_picture_name(*picture, name)),
                        None,
                    ),
                    ReferenceKind::Swatch { color } => (
                        "Reference colour".to_string(),
                        crate::AttachmentKind::Text,
                        Some(color.clone()),
                        None,
                    ),
                })
            })
            .collect::<Result<_>>()?;
        for request_id in request_ids {
            let request = self
                .requests
                .iter_mut()
                .find(|request| request.id == *request_id)
                .ok_or_else(|| Error("This request no longer exists.".into()))?;
            let mut used: BTreeSet<String> = request
                .attachments
                .iter()
                .map(|item| item.id.clone())
                .collect();
            let mut additions = Vec::new();
            for (name, kind, text, url) in &contexts {
                // An empty note carries no reference.
                if *kind == crate::AttachmentKind::Text && text.as_deref().is_none_or(str::is_empty)
                {
                    continue;
                }
                let duplicate =
                    request
                        .attachments
                        .iter()
                        .chain(additions.iter())
                        .any(|existing| {
                            existing.kind == *kind && existing.text == *text && existing.url == *url
                        });
                if duplicate {
                    continue;
                }
                let id = (1..)
                    .map(|n| format!("a{n}"))
                    .find(|id| !used.contains(id))
                    .expect("an unused identifier");
                used.insert(id.clone());
                additions.push(crate::RequestAttachment {
                    id,
                    name: name.chars().take(crate::limits::MAX_NAME_CHARS).collect(),
                    kind: *kind,
                    data_uri: None,
                    preview: None,
                    text: text.clone(),
                    url: url.clone(),
                });
            }
            ensure(
                request.attachments.len() + additions.len() <= 16,
                "A request supports at most 16 attachments.",
            )?;
            request.attachments.extend(additions);
        }
        Ok(())
    }
}

impl Project {
    pub(crate) fn reference_to_element(
        &mut self,
        item_id: Uuid,
        page_id: Uuid,
        rect: Rect,
        size_id: Option<Uuid>,
    ) -> Result<()> {
        let item = self
            .reference
            .iter()
            .position(|item| item.id == item_id)
            .map(|index| self.reference.remove(index))
            .ok_or_else(|| Error("This reference item no longer exists.".into()))?;
        let attachment = |id: String,
                          name: String,
                          kind: crate::AttachmentKind,
                          data_uri: Option<std::sync::Arc<str>>,
                          url: Option<String>| crate::RequestAttachment {
            id,
            name,
            kind,
            data_uri,
            preview: None,
            text: None,
            url,
        };
        let mut request: Option<(String, Vec<crate::RequestAttachment>)> = None;
        let restored = matches!(&item.kind, ReferenceKind::Element { .. });
        let mut element = match item.kind {
            ReferenceKind::Element { element, size } => {
                let mut element = *element;
                let page = self.page(page_id)?;
                // On a different page, carry the visible design into the destination rather
                // than leaving overrides tied to sizes that page does not own.
                fn rebase(element: &mut crate::Element, sizes: &[PageSize], source: Option<Uuid>) {
                    if element
                        .overrides
                        .keys()
                        .any(|id| !sizes.iter().any(|size| size.id == *id))
                    {
                        if let Some(over) = source.and_then(|id| element.overrides.get(&id)) {
                            if let Some(rect) = over.rect {
                                element.rect = rect;
                            }
                            if let Some(style) = &over.style {
                                element.style = style.clone();
                            }
                            if let Some(layout) = &over.layout {
                                element.layout = layout.clone();
                            }
                        }
                        element
                            .overrides
                            .retain(|id, _| sizes.iter().any(|size| size.id == *id));
                    }
                    for child in &mut element.children {
                        rebase(child, sizes, source);
                    }
                }
                rebase(&mut element, &page.sizes, size);
                element
            }
            ReferenceKind::Image { picture, .. } => {
                let mut element = crate::empty_element(crate::ElementKind::Image, rect);
                element.image = Some(picture);
                element
            }
            ReferenceKind::Note { text } => {
                let element = crate::empty_element(crate::ElementKind::Box, rect);
                if !text.trim().is_empty() {
                    request = Some((text, vec![]));
                }
                element
            }
            ReferenceKind::Link { url, title } => {
                let name: String = if title.trim().is_empty() {
                    url.clone()
                } else {
                    title
                }
                .chars()
                .take(crate::limits::MAX_NAME_CHARS)
                .collect();
                let element = crate::empty_element(crate::ElementKind::Box, rect);
                request = Some((
                    String::new(),
                    vec![attachment(
                        attachment_id(),
                        name,
                        crate::AttachmentKind::Link,
                        None,
                        Some(url),
                    )],
                ));
                element
            }
            ReferenceKind::Swatch { color } => {
                let element = crate::empty_element(crate::ElementKind::Box, rect);
                request = Some((format!("Use {color}"), vec![]));
                element
            }
        };
        let page = self.page(page_id)?;
        ensure(
            size_id.is_none_or(|id| page.sizes.iter().any(|size| size.id == id)),
            "This boundary size no longer exists.",
        )?;
        if restored {
            if let Some(size) = size_id.filter(|id| *id != page.base_size_id()) {
                let original = element
                    .overrides
                    .get(&size)
                    .and_then(|over| over.rect)
                    .unwrap_or(element.rect);
                element.overrides.entry(size).or_default().rect = Some(Rect {
                    x: rect.x,
                    y: rect.y,
                    ..original
                });
            } else {
                element.rect.x = rect.x;
                element.rect.y = rect.y;
            }
        } else {
            element.rect = rect;
        }
        let page = self.page_mut(page_id)?;
        page.elements.push(element.clone());
        let target_size = size_id.or(Some(page.base_size_id()));
        for request in &mut self.requests {
            for target in &mut request.targets {
                if matches!(target, RequestTarget::Reference { id } if *id == item_id) {
                    *target = RequestTarget::Element {
                        page: page_id,
                        size: target_size,
                        id: element.id,
                    };
                }
            }
        }
        if let Some((text, attachments)) = request {
            let request = PendingRequest {
                boundary_retry_order: Vec::new(),
                id: Uuid::new_v4(),
                text,
                attachments,
                targets: vec![RequestTarget::Element {
                    page: page_id,
                    size: target_size,
                    id: element.id,
                }],
                area: None,
                requested_at: "reference".into(),
            };
            self.add_request(request)?;
        }
        Ok(())
    }
}
