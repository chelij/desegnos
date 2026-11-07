use crate::elements::*;
use crate::{
    Error, MAX_PROMPT_CHARS, Page, Project, Rect, RequestAttachment, Result, ensure,
    model::validate_text,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

impl Project {
    /// The page and element for an id, wherever the tree holds it.
    pub fn element(&self, id: Uuid) -> Option<(&Page, &Element)> {
        self.pages
            .iter()
            .find_map(|page| find_element(&page.elements, id).map(|element| (page, element)))
    }

    pub fn element_mut(&mut self, id: Uuid) -> Option<&mut Element> {
        for page in &mut self.pages {
            if let Some(element) = find_element_mut(&mut page.elements, id) {
                return Some(element);
            }
        }
        None
    }

    /// A request waiting for Generate that names this layer as a target.
    pub fn element_has_pending_request(&self, id: Uuid) -> bool {
        self.requests.iter().any(|request| {
            !request.text.trim().is_empty()
                && request
                    .targets
                    .iter()
                    .any(|target| matches!(target, crate::RequestTarget::Element { id: target_id, .. } if *target_id == id))
        })
    }

    pub fn pending_request_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|request| !request.text.trim().is_empty())
            .count()
    }

    /// Element ids in a layer's subtree, including the layer itself and the layers a linked
    /// copy resolves from its definition.
    pub fn element_subtree_ids(&self, element_id: Uuid) -> BTreeSet<Uuid> {
        let mut ids = BTreeSet::new();
        if let Some((_, element)) = self.element(element_id) {
            collect_resolved_ids(self, std::slice::from_ref(element), &mut ids);
        } else if let Some(location) = link_location(self, element_id) {
            let definition = self.linked.get(&location.definition);
            if let Some(definition) = definition {
                let mut node = &definition.master;
                for index in &location.path {
                    match node.children.get(*index) {
                        Some(child) => node = child,
                        None => return ids,
                    }
                }
                ids.insert(element_id);
                let mut path = Vec::new();
                collect_nested_ids(location.instance, node, &mut path, &mut ids);
            }
        }
        ids
    }

    /// The layer a generation step edits: a stored layer, or the definition behind a copy's
    /// nested layer. A copy's position, parent and request history stay with the copy.
    pub(crate) fn editable_element_mut(&mut self, id: Uuid) -> Option<&mut Element> {
        let location = link_location(self, id);
        match location {
            None => self.element_mut(id),
            Some(location) => {
                let definition = self.linked.get_mut(&location.definition)?;
                let mut node = &mut definition.master;
                for index in &location.path {
                    node = node.children.get_mut(*index)?;
                }
                Some(node)
            }
        }
    }

    pub(crate) fn remove_editable_element(&mut self, id: Uuid) -> bool {
        for page in &mut self.pages {
            if remove_element(&mut page.elements, id).is_some() {
                return true;
            }
        }
        let Some(location) = link_location(self, id) else {
            return false;
        };
        let Some((last, parent)) = location.path.split_last() else {
            return false;
        };
        let Some(definition) = self.linked.get_mut(&location.definition) else {
            return false;
        };
        let mut node = &mut definition.master;
        for index in parent {
            match node.children.get_mut(*index) {
                Some(child) => node = child,
                None => return false,
            }
        }
        if *last < node.children.len() {
            node.children.remove(*last);
            return true;
        }
        false
    }

    /// Add shared elements to a boundary. The complete changed tree is adapted for narrower
    /// linked sizes at the end of the project transaction.
    pub(crate) fn insert_elements(
        &mut self,
        page_id: Uuid,
        parent_id: Option<Uuid>,
        index: usize,
        mut elements: Vec<Element>,
        picture_data_uri: Option<String>,
        size_id: Option<Uuid>,
    ) -> Result<()> {
        ensure(!elements.is_empty(), "Add at least one element.")?;
        if let Some(uri) = picture_data_uri {
            let picture = crate::project_assets::store_picture_uri(&uri)?;
            self.pictures
                .insert(picture, std::sync::Arc::from(uri.as_str()));
            for element in &mut elements {
                if matches!(element.kind, ElementKind::Image) && element.image.is_none() {
                    element.image = Some(picture);
                }
            }
        }
        // New layers get numbered default names per kind, counting the whole project so two
        // boxes never share a name. Pasted and duplicated layers arrive with a name already.
        let mut used = used_default_names(self);
        assign_default_names(&mut elements, &mut used);
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let source = size_id.unwrap_or(page.base_size_id());
        ensure(
            page.size(source).is_some(),
            "This size is not part of the boundary.",
        )?;
        let children = match parent_id {
            None => &mut page.elements,
            Some(parent_id) => {
                let parent = find_element_mut(&mut page.elements, parent_id)
                    .ok_or_else(|| Error("This box no longer exists.".into()))?;
                ensure(
                    parent.kind.holds_children(),
                    "Only a box or a group can hold children.",
                )?;
                &mut parent.children
            }
        };
        let at = index.min(children.len());
        children.splice(at..at, elements);
        Ok(())
    }

    pub(crate) fn update_elements(
        &mut self,
        page_id: Uuid,
        patches: Vec<ElementPatch>,
    ) -> Result<()> {
        ensure(!patches.is_empty(), "Choose a layer to change.")?;
        let page = self
            .pages
            .iter()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let targets: Vec<SizeTarget> = patches
            .iter()
            .map(|patch| size_target_for_patch(self, page, patch))
            .collect::<Result<_>>()?;
        for (mut patch, target) in patches.into_iter().zip(targets) {
            let mut picture = None;
            if let Some(uri) = patch.picture_data_uri.take() {
                let stored = crate::project_assets::store_picture_uri(&uri)?;
                self.pictures
                    .insert(stored, std::sync::Arc::from(uri.as_str()));
                picture = Some(stored);
            }
            if let Some(location) = link_location(self, patch.id) {
                self.apply_linked_patch(location, patch, picture, target)?;
                continue;
            }
            let page = self
                .pages
                .iter_mut()
                .find(|page| page.id == page_id)
                .ok_or_else(|| Error("This page no longer exists.".into()))?;
            let element = find_element_mut(&mut page.elements, patch.id)
                .ok_or_else(|| Error("This layer no longer exists.".into()))?;
            if let Some(picture) = picture {
                ensure(
                    matches!(element.kind, ElementKind::Image),
                    "Only an image element uses a picture.",
                )?;
                element.image = Some(picture);
            }
            apply_sized_patch(element, patch, target)?;
        }
        Ok(())
    }

    /// A patch on a copy: content changes the definition, position and visibility stay here.
    fn apply_linked_patch(
        &mut self,
        location: LinkLocation,
        mut patch: ElementPatch,
        picture: Option<Uuid>,
        target: SizeTarget,
    ) -> Result<()> {
        // Only a copy's own layer stores a per-size override; a layer inside the shared
        // definition edits every copy, so it keeps its values in the definition.
        let target = if location.path.is_empty() {
            target
        } else {
            SizeTarget::Base
        };
        if let SizeTarget::Size(size_id) = target {
            let (over, visible, dismissed) = sized_override(&mut patch)?;
            let owner = self
                .override_owner_mut(&location)
                .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
            let entry = owner.overrides.entry(size_id).or_default();
            merge_override(entry, &over, visible, dismissed);
            if entry.is_empty() {
                owner.overrides.remove(&size_id);
            }
        } else if target == SizeTarget::AllSizes {
            let cleared = cleared_fields(&patch);
            if let Some(owner) = self.override_owner_mut(&location) {
                for over in owner.overrides.values_mut() {
                    over.clear(cleared.0, cleared.1, cleared.2, cleared.3);
                }
                owner.overrides.retain(|_, over| !over.is_empty());
            }
        }
        if let Some(picture) = picture {
            let node = self.definition_node(&location)?;
            ensure(
                matches!(node.kind, ElementKind::Image),
                "Only an image element uses a picture.",
            )?;
            patch.image = Some(Some(picture));
        }
        if location.path.is_empty() {
            // A copy's own name stays on the layer; the linked name renames the shared group.
            if let Some(name) = patch.name.take() {
                let name = name
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty());
                if let Some(name) = &name {
                    validate_text(name, "Layer name", MAX_ELEMENT_NAME_CHARS, false)?;
                }
                if let Some(element) = self.element_mut(location.instance) {
                    element.name = name;
                }
            }
            if let Some(name) = patch.linked_name.take().flatten() {
                self.rename_definition(location.definition, &name)?;
            }
            if let Some(rect) = patch.rect.take() {
                validate_element_rect(&rect)?;
                let definition = self
                    .linked
                    .get_mut(&location.definition)
                    .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
                definition.master.rect.width = rect.width;
                definition.master.rect.height = rect.height;
                let instance = self
                    .element_mut(location.instance)
                    .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                instance.rect.x = rect.x;
                instance.rect.y = rect.y;
            }
            if let Some(visible) = patch.visible.take()
                && let Some(element) = self.element_mut(location.instance)
            {
                element.visible = visible;
            }
            if let Some(locked) = patch.locked.take()
                && let Some(element) = self.element_mut(location.instance)
            {
                element.locked = locked;
            }
        }
        let definition = self
            .linked
            .get_mut(&location.definition)
            .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
        let mut node = &mut definition.master;
        for index in &location.path {
            node = node
                .children
                .get_mut(*index)
                .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        }
        patch.apply(node)?;
        Ok(())
    }

    /// The stored element a copy's size override belongs to: the copy itself for its root, the
    /// definition node for a layer inside it.
    fn override_owner_mut(&mut self, location: &LinkLocation) -> Option<&mut Element> {
        if location.path.is_empty() {
            return self.element_mut(location.instance);
        }
        let definition = self.linked.get_mut(&location.definition)?;
        let mut node = &mut definition.master;
        for index in &location.path {
            node = node.children.get_mut(*index)?;
        }
        Some(node)
    }

    /// The design one linked size shows, after linked-element resolution and size overrides.
    pub fn resolve_for_size(&self, page_id: Uuid, size_id: Uuid) -> Result<Vec<Element>> {
        let page = self.page(page_id)?;
        ensure(
            page.size(size_id).is_some(),
            "This size is not part of the boundary.",
        )?;
        Ok(resolve_for_size(self, page, size_id))
    }

    fn definition_node(&self, location: &LinkLocation) -> Result<&Element> {
        let definition = self
            .linked
            .get(&location.definition)
            .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
        let mut node = &definition.master;
        for index in &location.path {
            node = node
                .children
                .get(*index)
                .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        }
        Ok(node)
    }

    /// Making a layer linked: its tree becomes the shared definition, while the layer keeps
    /// its own name.
    fn promote_to_linked(&mut self, element_id: Uuid, name: String) -> Result<()> {
        let content = self
            .element(element_id)
            .map(|(_, element)| resolve_element(self, element))
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        let definition = Uuid::new_v4();
        self.linked.insert(
            definition,
            LinkedDefinition {
                id: definition,
                name: name.clone(),
                master: definition_master(&content),
            },
        );
        let element = self
            .element_mut(element_id)
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        element.link = Some(definition);
        element.children.clear();
        Ok(())
    }

    fn rename_definition(&mut self, definition: Uuid, name: &str) -> Result<()> {
        validate_text(name, "Linked element name", MAX_ELEMENT_NAME_CHARS, true)?;
        ensure(
            !self.linked.iter().any(|(other, item)| {
                *other != definition && item.name.eq_ignore_ascii_case(name.trim())
            }),
            "A linked element with this name already exists.",
        )?;
        let item = self
            .linked
            .get_mut(&definition)
            .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
        item.name = name.trim().into();
        Ok(())
    }

    fn unique_link_name(&self, base: Option<&str>) -> String {
        let taken = |name: &str| {
            self.linked
                .values()
                .any(|item| item.name.eq_ignore_ascii_case(name))
        };
        if let Some(base) = base.map(str::trim).filter(|base| !base.is_empty()) {
            let base: String = base.chars().take(MAX_ELEMENT_NAME_CHARS).collect();
            if !taken(&base) {
                return base;
            }
        }
        let mut number = 1;
        loop {
            let name = format!("Linked {number}");
            if !taken(&name) {
                return name;
            }
            number += 1;
        }
    }

    /// Detaches one copy into a plain layer with its current resolved content.
    fn detach_instance(&mut self, instance: Uuid) -> Result<()> {
        let mut concrete = self
            .element(instance)
            .map(|(_, element)| resolve_element(self, element))
            .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        concrete.link = None;
        let element = self
            .element_mut(instance)
            .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
        *element = concrete;
        Ok(())
    }

    /// Make one layer linked, or join two or more under one definition. The definition takes
    /// the layer's label as its name when it is free; every layer keeps its own name.
    pub(crate) fn link_elements(&mut self, ids: Vec<Uuid>, source_id: Option<Uuid>) -> Result<()> {
        ensure(!ids.is_empty(), "Choose a layer to link.")?;
        let mut unique = BTreeSet::new();
        ensure(
            ids.iter().all(|id| unique.insert(*id)),
            "A layer was selected twice.",
        )?;
        let mut contents = Vec::with_capacity(ids.len());
        for id in &ids {
            let (_, element) = self
                .element(*id)
                .ok_or_else(|| Error("These layers no longer exist.".into()))?;
            ensure(element.link.is_none(), "These layers are already linked.")?;
            contents.push((*id, resolve_element(self, element)));
        }
        if contents.len() == 1 {
            let name = self.unique_link_name(Some(&contents[0].1.label()));
            return self.promote_to_linked(contents[0].0, name);
        }
        let winner = source_id
            .and_then(|source| contents.iter().find(|(id, _)| *id == source))
            .unwrap_or(&contents[0])
            .1
            .clone();
        for (_, content) in &contents {
            ensure(
                same_element_structure(&winner, content),
                "Different structure, can't link",
            )?;
        }
        let name = self.unique_link_name(Some(&winner.label()));
        let definition = Uuid::new_v4();
        self.linked.insert(
            definition,
            LinkedDefinition {
                id: definition,
                name: name.clone(),
                master: definition_master(&winner),
            },
        );
        for (id, _) in &contents {
            if let Some(element) = self.element_mut(*id) {
                element.link = Some(definition);
                element.children.clear();
            }
        }
        Ok(())
    }

    pub(crate) fn unlink_elements(&mut self, ids: Vec<Uuid>) -> Result<()> {
        ensure(!ids.is_empty(), "Select a linked layer to unlink.")?;
        let mut instances: Vec<Uuid> = Vec::new();
        for id in &ids {
            if let Some(location) = link_location(self, *id)
                && !instances.contains(&location.instance)
            {
                instances.push(location.instance);
            }
        }
        ensure(!instances.is_empty(), "These layers are not linked.")?;
        for instance in instances {
            self.detach_instance(instance)?;
        }
        Ok(())
    }

    /// Delete a linked definition with its copies: each copy becomes a plain layer with the
    /// definition's content, so no layer loses what it showed.
    pub(crate) fn delete_linked_definition(&mut self, id: Uuid) -> Result<()> {
        ensure(
            self.linked.contains_key(&id),
            "This linked element no longer exists.",
        )?;
        let mut instances = Vec::new();
        for page in &self.pages {
            collect_link_instances(&page.elements, id, &mut instances);
        }
        for instance in instances {
            self.detach_instance(instance)?;
        }
        self.linked.remove(&id);
        Ok(())
    }

    pub(crate) fn remove_linked_definition(&mut self, id: Uuid) -> Result<()> {
        ensure(
            self.linked.contains_key(&id),
            "This linked element no longer exists.",
        )?;
        ensure(
            !self
                .pages
                .iter()
                .any(|page| tree_has_link(&page.elements, id)),
            "This linked element is still used on the canvas.",
        )?;
        self.linked.remove(&id);
        Ok(())
    }

    pub(crate) fn paste_linked_instances(&mut self, items: Vec<LinkedPasteInstance>) -> Result<()> {
        ensure(!items.is_empty(), "Choose a layer to paste.")?;
        for item in items {
            ensure(!item.id.is_nil(), "A pasted layer needs an identifier.")?;
            ensure(
                self.element(item.id).is_none(),
                "A pasted layer already exists.",
            )?;
            let definition = match link_location(self, item.source_id) {
                Some(location) => location.definition,
                None => {
                    let content = self
                        .element(item.source_id)
                        .map(|(_, element)| resolve_element(self, element))
                        .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                    let name = self.unique_link_name(Some(&content.label()));
                    let definition = Uuid::new_v4();
                    self.linked.insert(
                        definition,
                        LinkedDefinition {
                            id: definition,
                            name: name.clone(),
                            master: definition_master(&content),
                        },
                    );
                    if let Some(element) = self.element_mut(item.source_id) {
                        element.link = Some(definition);
                        element.children.clear();
                    }
                    definition
                }
            };
            let mut instance = self
                .linked
                .get(&definition)
                .map(|item| item.master.clone())
                .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
            instance.id = item.id;
            instance.rect = item.rect;
            instance.link = Some(definition);
            instance.children.clear();
            instance.name = None;
            instance.meta = ElementMeta::default();
            self.insert_elements(
                item.page_id,
                item.parent_id,
                item.index,
                vec![instance],
                None,
                None,
            )?;
        }
        Ok(())
    }

    pub(crate) fn nudge_elements(
        &mut self,
        moves: Vec<ElementMove>,
        dx: f64,
        dy: f64,
    ) -> Result<()> {
        ensure(!moves.is_empty(), "Choose a layer to move.")?;
        ensure(dx.is_finite() && dy.is_finite(), "Invalid nudge distance.")?;
        let mut pages: BTreeMap<Uuid, Vec<ElementMove>> = BTreeMap::new();
        for movement in moves {
            pages.entry(movement.to_page_id).or_default().push(movement);
        }
        for (page_id, moves) in pages {
            let page = self
                .pages
                .iter()
                .find(|page| page.id == page_id)
                .ok_or_else(|| Error("This page no longer exists.".into()))?;
            let resolved = resolve_elements(self, &page.elements);
            let mut patches = Vec::with_capacity(moves.len());
            for movement in moves {
                let element = find_element(&resolved, movement.id)
                    .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                let rect = movement
                    .size_id
                    .and_then(|id| element.overrides.get(&id))
                    .and_then(|over| over.rect)
                    .unwrap_or(element.rect);
                patches.push(ElementPatch {
                    id: movement.id,
                    rect: Some(Rect {
                        x: rect.x + dx,
                        y: rect.y + dy,
                        ..rect
                    }),
                    size_id: movement.size_id,
                    all_sizes: movement.all_sizes,
                    ..Default::default()
                });
            }
            self.update_elements(page_id, patches)?;
        }
        Ok(())
    }

    pub(crate) fn move_elements(&mut self, moves: Vec<ElementMove>) -> Result<()> {
        ensure(!moves.is_empty(), "Choose a layer to move.")?;
        let mut moving = BTreeSet::new();
        let mut nested = Vec::new();
        let mut targets = Vec::with_capacity(moves.len());
        for movement in &moves {
            ensure(
                moving.insert(movement.id),
                "A layer was moved twice in one step.",
            )?;
            let destination = self
                .pages
                .iter()
                .find(|page| page.id == movement.to_page_id)
                .ok_or_else(|| Error("The destination page no longer exists.".into()))?;
            let target = size_target(destination, movement.size_id, movement.all_sizes)?;
            targets.push(target);
            validate_element_rect(&movement.rect)?;
            let Some(element) = self.element(movement.id).map(|(_, element)| element) else {
                // A layer inside a linked copy moves in the definition; parent and order stay.
                let location = link_location(self, movement.id)
                    .filter(|location| !location.path.is_empty())
                    .ok_or_else(|| Error("This layer no longer exists.".into()))?;
                let (index, parent_path) = location
                    .path
                    .split_last()
                    .expect("a nested layer has a parent path");
                ensure(
                    movement.index == *index,
                    "Reorder layers inside a linked element in one copy.",
                )?;
                if let Some(parent_id) = movement.to_parent_id {
                    let parent = link_location(self, parent_id)
                        .ok_or_else(|| Error("Move the box this layer sits in.".into()))?;
                    ensure(
                        parent.definition == location.definition
                            && parent.instance == location.instance
                            && parent.path == parent_path,
                        "A layer inside a linked element keeps its parent.",
                    )?;
                }
                nested.push((location, movement.rect, target));
                continue;
            };
            if let Some(parent_id) = movement.to_parent_id {
                let parent = self
                    .element(parent_id)
                    .map(|(_, element)| element)
                    .ok_or_else(|| Error("The destination box no longer exists.".into()))?;
                ensure(
                    parent.kind.holds_children(),
                    "Only a box or a group can hold children.",
                )?;
                let mut descendants = BTreeSet::new();
                collect_ids(&element.children, &mut descendants);
                ensure(
                    !descendants.contains(&parent_id) && parent_id != element.id,
                    "A layer cannot be moved into itself.",
                )?;
            }
        }
        for (location, rect, _target) in nested {
            let definition = self
                .linked
                .get_mut(&location.definition)
                .ok_or_else(|| Error("This linked element no longer exists.".into()))?;
            let mut node = &mut definition.master;
            for index in &location.path {
                node = node
                    .children
                    .get_mut(*index)
                    .ok_or_else(|| Error("This linked layer no longer exists.".into()))?;
            }
            // A nested layer keeps its position in the shared definition.
            write_rect(node, rect, SizeTarget::Base);
        }
        let stored: Vec<(ElementMove, SizeTarget)> = moves
            .into_iter()
            .zip(targets)
            .filter(|(movement, _)| self.element(movement.id).is_some())
            .collect();
        if stored.is_empty() {
            return Ok(());
        }
        let mut removed = Vec::with_capacity(stored.len());
        for (movement, _) in &stored {
            let element = self
                .pages
                .iter_mut()
                .find_map(|page| remove_element(&mut page.elements, movement.id))
                .ok_or_else(|| Error("This layer no longer exists.".into()))?;
            removed.push(element);
        }
        for ((movement, target), mut element) in stored.into_iter().zip(removed) {
            write_rect(&mut element, movement.rect, target);
            let page = self
                .pages
                .iter_mut()
                .find(|page| page.id == movement.to_page_id)
                .ok_or_else(|| Error("The destination page no longer exists.".into()))?;
            let children = match movement.to_parent_id {
                None => &mut page.elements,
                Some(parent_id) => {
                    let parent = find_element_mut(&mut page.elements, parent_id)
                        .ok_or_else(|| Error("The destination box no longer exists.".into()))?;
                    ensure(
                        parent.kind.holds_children(),
                        "Only a box or a group can hold children.",
                    )?;
                    &mut parent.children
                }
            };
            let at = movement.index.min(children.len());
            children.insert(at, element);
        }
        Ok(())
    }

    pub(crate) fn remove_elements(&mut self, page_id: Uuid, ids: Vec<Uuid>) -> Result<()> {
        ensure(!ids.is_empty(), "Choose a layer to remove.")?;
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        for id in &ids {
            ensure(
                find_element(&page.elements, *id).is_some(),
                "This layer no longer exists.",
            )?;
        }
        // Removing an ancestor removes its subtree; never remove the same element twice.
        let mut present = BTreeSet::new();
        for id in &ids {
            if find_element(&page.elements, *id).is_some() {
                present.insert(*id);
            }
        }
        remove_selected(&mut page.elements, &present);
        self.prune_requests();
        Ok(())
    }

    pub(crate) fn group_elements(&mut self, page_id: Uuid, ids: Vec<Uuid>) -> Result<()> {
        self.wrap_elements(page_id, ids, false)
    }

    pub(crate) fn stack_elements(&mut self, page_id: Uuid, ids: Vec<Uuid>) -> Result<()> {
        if ids.len() == 1 {
            let page = self
                .pages
                .iter()
                .find(|page| page.id == page_id)
                .ok_or_else(|| Error("This page no longer exists.".into()))?;
            let resolved = resolve_elements(self, &page.elements);
            if let Some(element) = find_element(&resolved, ids[0])
                && element.kind == ElementKind::Box
            {
                if matches!(element.layout, ElementLayout::Stack { .. }) {
                    return Ok(());
                }
                // Like switching to Stack in Properties, a box with children then hugs them.
                let mut style = element.style.clone();
                if !element.children.is_empty() {
                    style.sizing.width = SizingMode::Fit;
                    style.sizing.height = SizingMode::Fit;
                }
                return self.update_elements(
                    page_id,
                    vec![ElementPatch {
                        id: ids[0],
                        layout: Some(default_stack()),
                        style: Some(style),
                        ..Default::default()
                    }],
                );
            }
        }
        self.wrap_elements(page_id, ids, true)
    }

    fn wrap_elements(&mut self, page_id: Uuid, ids: Vec<Uuid>, stack: bool) -> Result<()> {
        ensure(
            ids.len() >= if stack { 1 } else { 2 },
            if stack {
                "Choose layers to wrap."
            } else {
                "Choose at least two layers to group."
            },
        )?;
        // A new group or stack is a new layer too, so it takes the next numbered name.
        let group_name = next_default_name(
            self,
            if stack {
                &ElementKind::Box
            } else {
                &ElementKind::Group
            },
        );
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let mut unique = BTreeSet::new();
        for id in &ids {
            ensure(unique.insert(*id), "A layer was grouped twice.")?;
        }
        // Every selected layer must be a direct sibling.
        let mut parent: Option<Vec<usize>> = None;
        let mut positions = Vec::with_capacity(unique.len());
        for id in &unique {
            let path = path_to_element(&page.elements, *id)
                .ok_or_else(|| Error("These layers no longer exist.".into()))?;
            let (index, parent_path) = path.split_last().expect("an element path");
            match &parent {
                None => parent = Some(parent_path.to_vec()),
                Some(expected) if expected != parent_path => {
                    return Err(Error(
                        "Select layers with the same parent before grouping.".into(),
                    ));
                }
                Some(_) => {}
            }
            positions.push(*index);
        }
        positions.sort_unstable();
        let parent = parent.expect("selected layers have a parent");
        let top = *positions.last().expect("selected layers");
        let children = container_mut(&mut page.elements, &parent);
        let union = positions
            .iter()
            .fold(None::<Rect>, |union, index| {
                let rect = children[*index].rect;
                Some(match union {
                    None => rect,
                    Some(union) => union_rect(&union, &rect),
                })
            })
            .expect("selected layers have a union");
        let mut grouped = Vec::with_capacity(positions.len());
        for index in &positions {
            grouped.push(children[*index].clone());
        }
        for index in positions.iter().rev() {
            children.remove(*index);
        }
        let mut group = group_element(union, grouped);
        group.name = Some(group_name);
        if stack {
            group.kind = ElementKind::Box;
            group.layout = default_stack();
            // A new stack wraps its children, like switching a box to Stack in Properties.
            group.style.sizing.width = SizingMode::Fit;
            group.style.sizing.height = SizingMode::Fit;
        }
        for child in &mut group.children {
            child.rect.x -= union.x;
            child.rect.y -= union.y;
        }
        let at = top + 1 - group.children.len();
        children.insert(at.min(children.len()), group);
        Ok(())
    }

    pub(crate) fn ungroup_elements(&mut self, page_id: Uuid, id: Uuid) -> Result<()> {
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(|| Error("This page no longer exists.".into()))?;
        let path = path_to_element(&page.elements, id)
            .ok_or_else(|| Error("This group no longer exists.".into()))?;
        let (index, parent_path) = path.split_last().expect("an element path");
        let children = container_mut(&mut page.elements, parent_path);
        ensure(
            matches!(children[*index].kind, ElementKind::Group),
            "Only a group can be ungrouped.",
        )?;
        let group = children.remove(*index);
        let origin = (group.rect.x, group.rect.y);
        let mut inner = group.children;
        for child in &mut inner {
            child.rect.x += origin.0;
            child.rect.y += origin.1;
            child.rotation += group.rotation;
        }
        children.splice(*index..*index, inner);
        Ok(())
    }

    pub(crate) fn update_element_context(
        &mut self,
        element_id: Uuid,
        context: Vec<RequestAttachment>,
    ) -> Result<()> {
        ensure(
            context.len() <= MAX_ELEMENT_CONTEXT,
            "A layer supports at most 32 context attachments.",
        )?;
        for attachment in &context {
            attachment.validate()?;
        }
        let element = self
            .element_mut(element_id)
            .ok_or_else(|| Error("This layer no longer exists.".into()))?;
        element.meta.context = context;
        Ok(())
    }
}

/// A patch changes only the fields it names; every other setting stays as it was.
#[derive(ts_rs::TS, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementPatch {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ElementStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub image: Option<Option<Uuid>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub url: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<ElementLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<Option<String>>,
    /// Renames the shared linked element a copy belongs to, without renaming the copy itself.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub linked_name: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<InputType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// A freshly picked picture; its bytes enter the project store in the same undo step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_data_uri: Option<String>,
    /// The linked size this edit belongs to. The base size edits base values; any other size
    /// stores the field as an override, so the other sizes keep what they inherit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_id: Option<Uuid>,
    /// Double-click editing: change base values and clear the same fields from every override.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub all_sizes: bool,
    /// Keeps a layer hidden in one size without a placeholder (Dismiss).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed: Option<bool>,
}

impl ElementPatch {
    fn apply(self, element: &mut Element) -> Result<()> {
        if let Some(rect) = self.rect {
            validate_element_rect(&rect)?;
            element.rect = rect;
        }
        if let Some(rotation) = self.rotation {
            ensure(
                rotation.is_finite() && rotation.abs() <= 360_000.0,
                "An element rotation must be finite.",
            )?;
            element.rotation = rotation;
        }
        if let Some(style) = self.style {
            style.validate()?;
            element.style = style;
        }
        if let Some(text) = self.text {
            ensure(
                text.chars().count() <= MAX_PROMPT_CHARS && !text.contains('\0'),
                "Element text is limited to 16,384 characters.",
            )?;
            element.text = text;
        }
        if let Some(image) = self.image {
            ensure(
                matches!(element.kind, ElementKind::Image),
                "Only an image element uses a picture.",
            )?;
            element.image = image;
        }
        if let Some(url) = self.url {
            if let Some(url) = &url {
                validate_element_url(url)?;
            }
            element.url = url;
        }
        if let Some(layout) = self.layout {
            ensure(
                element.kind.holds_children(),
                "Only a box lays out its children.",
            )?;
            layout.validate()?;
            element.layout = layout;
        }
        if let Some(visible) = self.visible {
            element.visible = visible;
        }
        if let Some(locked) = self.locked {
            element.locked = locked;
        }
        if let Some(name) = self.name {
            if let Some(name) = &name {
                validate_text(name, "Layer name", MAX_ELEMENT_NAME_CHARS, false)?;
            }
            element.name = name;
        }
        ensure(
            self.linked_name.is_none(),
            "Only a linked layer names its linked element.",
        )?;
        if let Some(input_type) = self.input_type {
            match &mut element.kind {
                ElementKind::Input { input_type: value } => *value = input_type,
                _ => return Err(Error("Only an input has an input type.".into())),
            }
        }
        if let Some(options) = self.options {
            match &mut element.kind {
                ElementKind::Select { options: value } => *value = options,
                _ => return Err(Error("Only a select has options.".into())),
            }
        }
        if let Some(language) = self.language {
            match &mut element.kind {
                ElementKind::Code {
                    language: value, ..
                } => {
                    validate_text(&language, "Code language", 40, false)?;
                    *value = language;
                }
                _ => return Err(Error("Only a code block has a language.".into())),
            }
        }
        if let Some(source) = self.source {
            match &mut element.kind {
                ElementKind::Code { source: value, .. } => {
                    ensure(
                        source.chars().count() <= MAX_ELEMENT_CODE_CHARS && !source.contains('\0'),
                        "A code block is limited to 200,000 characters.",
                    )?;
                    *value = source;
                }
                _ => return Err(Error("Only a code block has source.".into())),
            }
        }
        Ok(())
    }
}

/// Where an element edit lands. The base size edits base values; any other size stores the
/// field as an override; double-click editing changes base values and clears the overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeTarget {
    Base,
    AllSizes,
    Size(Uuid),
}

pub(crate) fn size_target(
    page: &Page,
    size_id: Option<Uuid>,
    all_sizes: bool,
) -> Result<SizeTarget> {
    if all_sizes {
        return Ok(SizeTarget::AllSizes);
    }
    match size_id {
        None => Ok(SizeTarget::Base),
        Some(id) if page.base_size_id() == id => Ok(SizeTarget::Base),
        Some(id) if page.size(id).is_some() => Ok(SizeTarget::Size(id)),
        Some(_) => Err(Error("This size is not part of the boundary.".into())),
    }
}

/// Where a patch lands, with one exception: a placeholder hides a layer in a size with an
/// override. Include and dismiss are edits to that override even when it belongs to the base
/// size, whose other values stay inherited.
pub(crate) fn size_target_for_patch(
    project: &Project,
    page: &Page,
    patch: &ElementPatch,
) -> Result<SizeTarget> {
    let target = size_target(page, patch.size_id, patch.all_sizes)?;
    if target != SizeTarget::Base || (patch.visible.is_none() && patch.dismissed.is_none()) {
        return Ok(target);
    }
    let Some(size_id) = patch.size_id else {
        return Ok(target);
    };
    let has_override = project
        .element(patch.id)
        .is_some_and(|(_, element)| element.overrides.contains_key(&size_id))
        || link_location(project, patch.id).is_some_and(|location| {
            location.path.is_empty()
                && project
                    .element(location.instance)
                    .is_some_and(|(_, element)| element.overrides.contains_key(&size_id))
        });
    Ok(if has_override {
        SizeTarget::Size(size_id)
    } else {
        target
    })
}

/// The fields a double-click edit changes in base values and clears from every size override.
fn cleared_fields(patch: &ElementPatch) -> (bool, bool, bool, bool) {
    (
        patch.rect.is_some(),
        patch.style.is_some(),
        patch.layout.is_some(),
        patch.visible.is_some() || patch.dismissed.is_some(),
    )
}

/// The part of a single-size patch that becomes an override, and the visibility flags it sets.
fn sized_override(
    patch: &mut ElementPatch,
) -> Result<(ElementOverride, Option<bool>, Option<bool>)> {
    let mut over = ElementOverride::default();
    if let Some(rect) = patch.rect.take() {
        validate_element_rect(&rect)?;
        over.rect = Some(rect);
    }
    if let Some(style) = patch.style.take() {
        style.validate()?;
        over.style = Some(style);
    }
    if let Some(layout) = patch.layout.take() {
        layout.validate()?;
        over.layout = Some(layout);
    }
    Ok((over, patch.visible.take(), patch.dismissed.take()))
}

fn merge_override(
    entry: &mut ElementOverride,
    over: &ElementOverride,
    visible: Option<bool>,
    dismissed: Option<bool>,
) {
    if over.rect.is_some()
        || over.style.is_some()
        || over.layout.is_some()
        || visible.is_some()
        || dismissed.is_some()
    {
        entry.automatic = false;
    }
    if over.rect.is_some() {
        entry.rect = over.rect;
    }
    if over.style.is_some() {
        entry.style = over.style.clone();
    }
    if over.layout.is_some() {
        entry.layout = over.layout.clone();
    }
    if let Some(visible) = visible {
        entry.hidden = !visible;
        if visible {
            entry.dismissed = false;
        }
    }
    if let Some(dismissed) = dismissed {
        entry.dismissed = dismissed;
        if dismissed {
            entry.hidden = true;
        }
    }
}

/// A single-size patch changes that size's override; everything shared (text, kind, name,
/// children) still lands in the base model.
pub(crate) fn apply_sized_patch(
    element: &mut Element,
    mut patch: ElementPatch,
    target: SizeTarget,
) -> Result<()> {
    match target {
        SizeTarget::Base => {
            ensure(
                patch.dismissed.is_none(),
                "Only a size can dismiss a layer.",
            )?;
            patch.apply(element)
        }
        SizeTarget::Size(size_id) => {
            let (over, visible, dismissed) = sized_override(&mut patch)?;
            let entry = element.overrides.entry(size_id).or_default();
            merge_override(entry, &over, visible, dismissed);
            if entry.is_empty() {
                element.overrides.remove(&size_id);
            }
            patch.apply(element)
        }
        SizeTarget::AllSizes => {
            let (rect, style, layout, visible) = cleared_fields(&patch);
            patch.apply(element)?;
            let mut empty = Vec::new();
            for (size_id, over) in element.overrides.iter_mut() {
                over.clear(rect, style, layout, visible);
                if over.is_empty() {
                    empty.push(*size_id);
                }
            }
            for size_id in empty {
                element.overrides.remove(&size_id);
            }
            Ok(())
        }
    }
}

fn write_rect(element: &mut Element, rect: Rect, target: SizeTarget) {
    match target {
        SizeTarget::Size(size_id) => {
            let entry = element.overrides.entry(size_id).or_default();
            entry.automatic = false;
            entry.rect = Some(rect);
        }
        SizeTarget::AllSizes => {
            element.rect = rect;
            for over in element.overrides.values_mut() {
                over.rect = None;
            }
            element.overrides.retain(|_, over| !over.is_empty());
        }
        SizeTarget::Base => element.rect = rect,
    }
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementMove {
    pub id: Uuid,
    pub to_page_id: Uuid,
    pub to_parent_id: Option<Uuid>,
    pub index: usize,
    pub rect: Rect,
    /// The linked size the move happened in; a non-base size stores the position as an override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_id: Option<Uuid>,
    /// Double-click editing: move in base values and clear every size's position override.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub all_sizes: bool,
}

/// A named optional field can be cleared: missing keeps it, null clears it.
fn double_option<'de, D, T>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn union_rect(first: &Rect, second: &Rect) -> Rect {
    let x = first.x.min(second.x);
    let y = first.y.min(second.y);
    Rect {
        x,
        y,
        width: (first.x + first.width).max(second.x + second.width) - x,
        height: (first.y + first.height).max(second.y + second.height) - y,
    }
}

/// The highest number already used for each default base name across the whole project.
pub(crate) fn used_default_names(project: &Project) -> BTreeMap<String, u32> {
    fn walk(elements: &[Element], used: &mut BTreeMap<String, u32>) {
        for element in elements {
            if let Some(name) = &element.name {
                let name = name.trim();
                let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
                if digits > 0 {
                    let (base, number) = name.split_at(name.len() - digits);
                    if let Some(base) = base.strip_suffix(' ')
                        && let Ok(number) = number.parse::<u32>()
                    {
                        used.entry(base.to_string())
                            .and_modify(|seen| *seen = (*seen).max(number))
                            .or_insert(number);
                    }
                }
            }
            walk(&element.children, used);
        }
    }
    let mut used = BTreeMap::new();
    for page in &project.pages {
        walk(&page.elements, &mut used);
    }
    for definition in project.linked.values() {
        walk(&definition.master.children, &mut used);
    }
    used
}

/// Gives every unnamed layer its next numbered default name, children included.
pub(crate) fn assign_default_names(elements: &mut [Element], used: &mut BTreeMap<String, u32>) {
    for element in elements {
        if element
            .name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
        {
            let base = default_layer_name_base(&element.kind);
            let next = used.get(base).copied().unwrap_or(0) + 1;
            used.insert(base.to_string(), next);
            element.name = Some(format!("{base} {next}"));
        }
        assign_default_names(&mut element.children, used);
    }
}

/// The next free numbered default name for one new layer.
fn next_default_name(project: &Project, kind: &ElementKind) -> String {
    let used = used_default_names(project);
    let base = default_layer_name_base(kind);
    let next = used.get(base).copied().unwrap_or(0) + 1;
    format!("{base} {next}")
}

/// An element with the neutral visuals every new layer starts from.
pub fn empty_element(kind: ElementKind, rect: Rect) -> Element {
    let fill =
        matches!(&kind, ElementKind::Box | ElementKind::Ellipse).then(|| "#d9d9d9".to_owned());
    Element {
        id: Uuid::new_v4(),
        kind,
        name: None,
        link: None,
        rect,
        rotation: 0.0,
        visible: true,
        locked: false,
        style: ElementStyle {
            flip_x: false,
            flip_y: false,
            sizing: ElementSizing::default(),
            padding: None,
            references: StyleReferences::default(),
            fill,
            stroke: ElementStroke {
                color: None,
                width: 0.0,
            },
            radius: 0.0,
            opacity: 1.0,
            shadow: ElementShadow {
                x: 0.0,
                y: 0.0,
                blur: 0.0,
                color: None,
            },
            text_style: ElementTextStyle::default(),
        },
        text: String::new(),
        image: None,
        url: None,
        layout: ElementLayout::Free,
        overrides: BTreeMap::new(),
        children: vec![],
        meta: ElementMeta::default(),
    }
}

/// Group visuals are empty: no fill, no stroke, no shadow, no text.
fn group_element(rect: Rect, children: Vec<Element>) -> Element {
    Element {
        children,
        ..empty_element(ElementKind::Group, rect)
    }
}

fn path_to_element(elements: &[Element], id: Uuid) -> Option<Vec<usize>> {
    for (index, element) in elements.iter().enumerate() {
        if element.id == id {
            return Some(vec![index]);
        }
        if let Some(mut path) = path_to_element(&element.children, id) {
            path.insert(0, index);
            return Some(path);
        }
    }
    None
}

fn container_mut<'a>(elements: &'a mut Vec<Element>, path: &[usize]) -> &'a mut Vec<Element> {
    match path.split_first() {
        None => elements,
        Some((index, rest)) => container_mut(&mut elements[*index].children, rest),
    }
}

fn remove_selected(elements: &mut Vec<Element>, ids: &BTreeSet<Uuid>) {
    elements.retain(|element| !ids.contains(&element.id));
    for element in elements.iter_mut() {
        remove_selected(&mut element.children, ids);
    }
}

/// A compact subtree for the AI: kinds, rects, styles, text, layout and children, without
/// request history, context or pending requests.
pub fn element_subtree(element: &Element) -> serde_json::Value {
    serde_json::json!({
        "id": element.id,
        "kind": element.kind,
        "name": element.name,
        "link": element.link,
        "rect": element.rect,
        "rotation": element.rotation,
        "visible": element.visible,
        "locked": element.locked,
        "style": element.style,
        "text": element.text,
        "image": element.image,
        "url": element.url,
        "layout": element.layout,
        "children": element.children.iter().map(element_subtree).collect::<Vec<_>>(),
    })
}

/// A compact outline of one boundary's elements, used to describe the design to the AI.
pub fn element_outline(elements: &[Element]) -> Vec<serde_json::Value> {
    fn walk(elements: &[Element], depth: usize) -> Vec<serde_json::Value> {
        let mut rows = Vec::new();
        for element in elements {
            rows.push(serde_json::json!({
                "id": element.id,
                "kind": element.kind.label(),
                "label": element.label(),
                "rect": element.rect,
                "depth": depth,
            }));
            if depth < 32 {
                rows.extend(walk(&element.children, depth + 1));
            }
        }
        rows
    }
    walk(elements, 0)
}

fn default_stack() -> ElementLayout {
    ElementLayout::Stack {
        direction: StackDirection::Column,
        gap: 0.0,
        padding: 0.0,
        align: StackAlign::Start,
        justify: StackJustify::Start,
        wrap: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizing_defaults_and_limits_are_validated() {
        let sizing: ElementSizing =
            serde_json::from_str(r#"{"width":"fill","height":"fit","minWidth":30,"maxWidth":80}"#)
                .unwrap();
        assert_eq!(sizing.constrain(10.0, 50.0), (30.0, 50.0));
        assert_eq!(sizing.constrain(100.0, 50.0), (80.0, 50.0));
        assert!(sizing.validate().is_ok());
        assert!(
            ElementSizing {
                min_width: Some(100.0),
                max_width: Some(50.0),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ElementSizing {
                min_height: Some(f64::NAN),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let mut legacy = serde_json::to_value(
            empty_element(
                ElementKind::Box,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 40.0,
                },
            )
            .style,
        )
        .unwrap();
        legacy.as_object_mut().unwrap().remove("sizing");
        legacy.as_object_mut().unwrap().remove("references");
        let style: ElementStyle = serde_json::from_value(legacy).unwrap();
        assert_eq!(style.sizing.width, SizingMode::Fixed);
    }

    #[test]
    fn per_side_padding_is_optional_serialized_and_validated() {
        let mut style = empty_element(
            ElementKind::Box,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        )
        .style;
        assert!(style.padding.is_none());
        let value = serde_json::to_value(&style).unwrap();
        assert!(
            value.get("padding").is_none(),
            "an all-sides layout padding stays compact"
        );
        style.padding = Some(ElementPadding {
            top: 8.0,
            right: 16.0,
            bottom: 8.0,
            left: 16.0,
        });
        assert!(style.validate().is_ok());
        let value = serde_json::to_value(&style).unwrap();
        assert_eq!(value["padding"]["top"], 8.0);
        assert_eq!(value["padding"]["left"], 16.0);
        style.padding = Some(ElementPadding {
            top: -1.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        });
        assert!(style.validate().is_err());
        style.padding = Some(ElementPadding {
            top: 4097.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        });
        assert!(style.validate().is_err());
        style.padding = Some(ElementPadding {
            top: f64::NAN,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        });
        assert!(style.validate().is_err());
    }

    #[test]
    fn inserted_layers_get_numbered_default_names_per_kind() {
        let project = Project::new(
            "Names".into(),
            crate::Viewport {
                width: 800,
                height: 600,
            },
            crate::Appearance::Light,
        )
        .unwrap();
        let page = project.pages[0].id;
        let insert = |project: &Project, elements: Vec<Element>| {
            project
                .apply(
                    project.revision,
                    crate::Command::InsertElements {
                        page_id: page,
                        parent_id: None,
                        index: 0,
                        elements,
                        picture_data_uri: None,
                        size_id: None,
                    },
                )
                .unwrap()
        };
        let mut text = empty_element(
            ElementKind::Text,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 24.0,
            },
        );
        text.text = "Hello".into();
        let project = insert(
            &project,
            vec![
                empty_element(
                    ElementKind::Box,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 80.0,
                    },
                ),
                text,
            ],
        );
        let names: Vec<_> = project.pages[0]
            .elements
            .iter()
            .map(|element| element.name.clone().unwrap())
            .collect();
        assert_eq!(names, vec!["Rectangle 1".to_string(), "Text 1".to_string()]);
        // A default-named text layer still reads as its text in Layers and Properties.
        assert_eq!(project.pages[0].elements[1].label(), "Hello");
        let project = insert(
            &project,
            vec![empty_element(
                ElementKind::Box,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            )],
        );
        assert_eq!(
            project.pages[0].elements[0].name.as_deref(),
            Some("Rectangle 2")
        );
    }

    #[test]
    fn shared_style_edits_resolve_after_overrides_and_delete_preserves_values() {
        let mut project = Project::new(
            "Styles".into(),
            crate::Viewport {
                width: 800,
                height: 600,
            },
            crate::Appearance::Light,
        )
        .unwrap();
        let colour = crate::ColourStyle {
            id: Uuid::new_v4(),
            name: "Ink".into(),
            value: "#123456".into(),
        };
        project = project
            .apply(
                project.revision,
                crate::Command::AddColourStyle {
                    style: colour.clone(),
                },
            )
            .unwrap();
        let size = project.pages[0].base_size_id();
        let mut element = empty_element(
            ElementKind::Text,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );
        element.style.references.text_colour = Some(colour.id);
        let mut over = element.style.clone();
        over.references.fill = Some(colour.id);
        element.overrides.insert(
            size,
            ElementOverride {
                style: Some(over),
                ..Default::default()
            },
        );
        project.pages[0].elements.push(element);
        project.validate().unwrap();
        let resolved = resolve_for_size(&project, &project.pages[0], size);
        assert_eq!(resolved[0].style.text_style.color, "#123456");
        assert_eq!(resolved[0].style.fill.as_deref(), Some("#123456"));
        project = project
            .apply(
                project.revision,
                crate::Command::UpdateColourStyle {
                    style: crate::ColourStyle {
                        value: "#abcdef".into(),
                        ..colour.clone()
                    },
                },
            )
            .unwrap();
        assert_eq!(
            resolve_for_size(&project, &project.pages[0], size)[0]
                .style
                .text_style
                .color,
            "#abcdef"
        );
        project = project
            .apply(
                project.revision,
                crate::Command::RemoveColourStyle { id: colour.id },
            )
            .unwrap();
        let resolved = resolve_for_size(&project, &project.pages[0], size);
        assert_eq!(resolved[0].style.text_style.color, "#abcdef");
        assert_eq!(resolved[0].style.references.text_colour, None);
        assert!(
            project
                .apply(
                    project.revision,
                    crate::Command::AddColourStyle {
                        style: crate::ColourStyle {
                            value: "red".into(),
                            ..colour
                        }
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn an_embed_accepts_a_project_page_path_but_video_and_audio_stay_web_only() {
        let project = Project::new(
            "Frames".into(),
            crate::Viewport {
                width: 800,
                height: 600,
            },
            crate::Appearance::Light,
        )
        .unwrap();
        let page_path = project.pages[0].path.clone();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 240.0,
        };
        let mut embed = empty_element(ElementKind::Embed, rect);
        embed.url = Some(page_path.clone());
        embed.validate(&project).unwrap();
        embed.url = Some("https://example.com/map".into());
        embed.validate(&project).unwrap();
        embed.url = Some("javascript:alert(1)".into());
        assert!(embed.validate(&project).is_err());
        for kind in [ElementKind::Video, ElementKind::Audio] {
            let mut media = empty_element(kind, rect);
            media.url = Some(page_path.clone());
            assert!(media.validate(&project).is_err());
            media.url = Some("https://example.com/media".into());
            media.validate(&project).unwrap();
        }
    }

    #[test]
    fn colours_accept_hex_and_transparent() {
        assert!(validate_colour("#abc", "Fill").is_ok());
        assert!(validate_colour("#aabbcc", "Fill").is_ok());
        assert!(validate_colour("#aabbccdd", "Fill").is_ok());
        assert!(validate_colour("transparent", "Fill").is_ok());
        assert!(validate_colour("red", "Fill").is_err());
    }
}
