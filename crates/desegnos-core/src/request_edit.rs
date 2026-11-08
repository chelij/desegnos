use crate::{
    Error, Result, limits::MAX_REQUESTS, model::PendingRequest, model::Project, validation::ensure,
};
use std::collections::BTreeSet;
use uuid::Uuid;

impl Project {
    pub(crate) fn add_request(&mut self, request: PendingRequest) -> Result<()> {
        request.validate()?;
        if let Some(layer) = request.sole_layer() {
            self.requests
                .retain(|existing| existing.sole_layer() != Some(layer));
        }
        ensure(
            self.requests.len() < MAX_REQUESTS,
            format!(
                "A project holds at most {} pending requests.",
                crate::limits::MAX_REQUESTS
            ),
        )?;
        ensure(
            !self
                .requests
                .iter()
                .any(|existing| existing.id == request.id),
            "This request already exists.",
        )?;
        self.requests.push(request);
        Ok(())
    }

    pub fn keep_one_request_per_layer(&mut self) {
        let mut seen = BTreeSet::new();
        let mut kept: Vec<_> = self
            .requests
            .drain(..)
            .rev()
            .filter(|request| request.sole_layer().is_none_or(|layer| seen.insert(layer)))
            .collect();
        kept.reverse();
        self.requests = kept;
    }

    pub(crate) fn update_request(&mut self, request: PendingRequest) -> Result<()> {
        request.validate()?;
        let existing = self
            .requests
            .iter_mut()
            .find(|existing| existing.id == request.id)
            .ok_or_else(|| Error("This request no longer exists.".into()))?;
        *existing = request;
        Ok(())
    }

    pub(crate) fn remove_requests(&mut self, ids: &[Uuid]) -> Result<()> {
        ensure(!ids.is_empty(), "Select a request first.")?;
        for id in ids {
            ensure(
                self.requests.iter().any(|request| request.id == *id),
                "This request no longer exists.",
            )?;
        }
        self.requests.retain(|request| !ids.contains(&request.id));
        Ok(())
    }
}
