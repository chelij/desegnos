use crate::{
    Error, Result, limits::MAX_POINTERS, model::Pointer, model::PointerKind, model::PointerSource,
    model::PointerTarget, model::Project, validation::ensure,
};
use uuid::Uuid;

impl Project {
    pub(crate) fn pointer_mut(&mut self, id: Uuid) -> Result<&mut Pointer> {
        self.pointers
            .iter_mut()
            .find(|pointer| pointer.id == id)
            .ok_or_else(|| Error("This pointer no longer exists.".into()))
    }

    pub(crate) fn add_pointer(&mut self, pointer: Pointer) -> Result<()> {
        ensure(
            self.pointers.len() < MAX_POINTERS,
            format!(
                "A project holds at most {} pointers.",
                crate::limits::MAX_POINTERS
            ),
        )?;
        ensure(
            !self
                .pointers
                .iter()
                .any(|existing| existing.id == pointer.id),
            "This pointer already exists.",
        )?;
        pointer.validate()?;
        ensure(
            !pointer.is_behaviour()
                || !self
                    .pointers
                    .iter()
                    .any(|existing| existing.is_behaviour() && existing.from == pointer.from),
            "A source can have at most one behaviour pointer.",
        )?;
        self.pointers.push(pointer);
        Ok(())
    }

    pub(crate) fn update_pointer(
        &mut self,
        pointer_id: Uuid,
        from: Option<PointerSource>,
        to: Option<PointerTarget>,
        comment: Option<String>,
        kind: Option<PointerKind>,
    ) -> Result<()> {
        let mut pointer = self.pointer_mut(pointer_id)?.clone();
        if let Some(from) = from {
            pointer.from = from;
        }
        if let Some(to) = to {
            pointer.to = to;
        }
        if let Some(comment) = comment {
            pointer.comment = comment;
        }
        if let Some(kind) = kind {
            pointer.kind = kind;
        }
        pointer.validate()?;
        ensure(
            !pointer.is_behaviour()
                || !self.pointers.iter().any(|existing| {
                    existing.id != pointer_id
                        && existing.is_behaviour()
                        && existing.from == pointer.from
                }),
            "A source can have at most one behaviour pointer.",
        )?;
        *self.pointer_mut(pointer_id)? = pointer;
        Ok(())
    }
}
