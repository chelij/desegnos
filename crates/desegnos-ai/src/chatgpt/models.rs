//! Account-bound catalogue refresh and stale-result checks.
use super::*;

impl ChatgptManager {
    pub async fn discover_models(
        &self,
        account_id: Uuid,
        client: &OpenAiClient,
        cancellation: &CancellationToken,
    ) -> ChatgptResult<ChatgptView> {
        let epoch = self
            .inner
            .state
            .lock()
            .await
            .epochs
            .get(&account_id)
            .copied();
        let bearer = match self.bearer(account_id, cancellation).await {
            Ok(bearer) => bearer,
            Err(error) => {
                let mut state = self.inner.state.lock().await;
                state
                    .model_errors
                    .insert(account_id, error.message().into());
                state.model_provider_errors.remove(&account_id);
                return Err(error);
            }
        };
        let result = client.list_chatgpt_models(&bearer, cancellation).await;
        let mut state = self.inner.state.lock().await;
        if state.epochs.get(&account_id).copied() != epoch
            || self
                .inner
                .store
                .read()?
                .registration(account_id)?
                .reconnect_required
        {
            drop(state);
            return Ok(self.view().await);
        }
        match result {
            Ok(models) => {
                state.models.insert(account_id, models);
                state.model_errors.remove(&account_id);
                state.model_provider_errors.remove(&account_id);
            }
            Err(error) => {
                state.models.remove(&account_id);
                state.model_errors.insert(account_id, error.message());
                state.model_provider_errors.insert(account_id, error);
            }
        }
        drop(state);
        Ok(self.view().await)
    }
}
