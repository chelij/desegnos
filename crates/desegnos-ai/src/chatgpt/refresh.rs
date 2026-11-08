//! Token rotation and recovery of accepted credentials.
use super::*;

impl ChatgptManager {
    pub async fn bearer(
        &self,
        account_id: Uuid,
        cancellation: &CancellationToken,
    ) -> ChatgptResult<ApiKey> {
        let _gate = self.inner.gate.lock().await;
        let _lease = self.lease(cancellation).await?;
        let mut metadata = self.inner.store.read()?;
        let registration = metadata.registration(account_id)?.clone();
        discard_stale_rotations(&mut *self.inner.state.lock().await, &metadata);
        if registration.reconnect_required {
            return Err(Kind::Reconnect.into());
        }
        if self
            .inner
            .state
            .lock()
            .await
            .pending_rotations
            .contains_key(&account_id)
        {
            return Err(Kind::VaultUnavailable.into());
        }
        if registration.refresh_pending.is_some() {
            return Err(Kind::Reconnect.into());
        }
        let session = self
            .inner
            .state
            .lock()
            .await
            .sessions
            .get(&account_id)
            .cloned();
        let mut tokens = match session {
            Some(tokens) => tokens,
            None if registration.system_stored => self
                .load_system(&metadata, &registration)
                .await?
                .ok_or(Kind::Reconnect)?,
            None => return Err(Kind::Reconnect.into()),
        };
        tokens.validate_mapping(&metadata, &registration)?;
        if !tokens.plan_enabled() {
            return Err(Kind::PlanPermission.into());
        }
        if tokens.expires_at <= now().saturating_add(60) && now() >= tokens.earliest_refresh_at {
            let rotation_id = Uuid::new_v4();
            metadata = self.set_refresh_pending(&metadata, account_id, Some(rotation_id))?;
            let response = match self
                .inner
                .http
                .refresh(&registration.client_id, &tokens.refresh_token, cancellation)
                .await
            {
                Ok(response) => response,
                Err(error) if error.kind == Kind::Reconnect => {
                    self.disconnect_locked(&metadata, &registration, true)
                        .await?;
                    return Err(error);
                }
                Err(error) => {
                    if matches!(error.kind, Kind::InvalidClient | Kind::TokenRejected) {
                        self.set_refresh_pending(&metadata, account_id, None)?;
                    }
                    return Err(error);
                }
            };
            let identity = self
                .inner
                .http
                .identity(
                    &response.id_token,
                    &registration.client_id,
                    None,
                    cancellation,
                )
                .await?;
            if identity.sub != tokens.subject
                || identity
                    .nonce
                    .as_ref()
                    .is_some_and(|value| value != &tokens.nonce)
            {
                return Err(Kind::IdentityMismatch.into());
            }
            let next = make_record(
                &metadata,
                &registration,
                &response,
                &identity.sub,
                &tokens.nonce,
                tokens.storage,
            );
            if next.storage == CredentialStorage::System {
                if let Err(error) = self.save_system(&next).await {
                    let mut state = self.inner.state.lock().await;
                    state.pending_rotations.insert(
                        account_id,
                        PendingRotation {
                            id: rotation_id,
                            tokens: next,
                        },
                    );
                    state.storage_error = Some(ROTATION_STORAGE_MESSAGE.into());
                    return Err(error);
                }
            } else {
                self.inner
                    .state
                    .lock()
                    .await
                    .sessions
                    .insert(account_id, next.clone());
            }
            if let Err(error) = self.set_refresh_pending(&metadata, account_id, None) {
                let mut state = self.inner.state.lock().await;
                state.pending_rotations.insert(
                    account_id,
                    PendingRotation {
                        id: rotation_id,
                        tokens: next,
                    },
                );
                state.storage_error = Some(ROTATION_STORAGE_MESSAGE.into());
                return Err(error);
            }
            tokens = next;
        }
        if tokens.expires_at <= now() {
            return Err(Kind::RefreshTooEarly.into());
        }
        if !tokens.plan_enabled() {
            return Err(Kind::PlanPermission.into());
        }
        ApiKey::from_chatgpt_token(tokens.access_token.clone())
            .map_err(|_| Kind::InvalidToken.into())
    }

    pub async fn retry_secure_storage(&self, account_id: Uuid) -> ChatgptResult<ChatgptView> {
        let _gate = self.inner.gate.lock().await;
        let cancellation = CancellationToken::new();
        let _lease = self.lease(&cancellation).await?;
        let metadata = self.inner.store.read()?;
        let registration = metadata.registration(account_id)?;
        let mut state = self.inner.state.lock().await;
        discard_stale_rotations(&mut state, &metadata);
        if registration.reconnect_required || registration.refresh_pending.is_none() {
            return Err(Kind::Reconnect.into());
        }
        let tokens = state
            .pending_rotations
            .get(&account_id)
            .filter(|rotation| Some(rotation.id) == registration.refresh_pending)
            .map(|rotation| rotation.tokens.clone())
            .ok_or(Kind::Reconnect)?;
        drop(state);
        tokens.validate_mapping(&metadata, registration)?;
        if tokens.storage == CredentialStorage::System {
            self.save_system(&tokens).await?;
        } else {
            self.inner
                .state
                .lock()
                .await
                .sessions
                .insert(account_id, tokens);
        }
        self.set_refresh_pending(&metadata, account_id, None)?;
        let mut state = self.inner.state.lock().await;
        state.pending_rotations.remove(&account_id);
        state.storage_error = None;
        state.model_errors.remove(&account_id);
        state.model_provider_errors.remove(&account_id);
        state.message = Some("Secure storage repaired.".into());
        drop(state);
        drop(_lease);
        drop(_gate);
        Ok(self.view().await)
    }
}
