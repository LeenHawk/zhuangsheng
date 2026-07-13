use tauri::State;
use zhuangsheng_core::application::secret::{
    LockSecretStoreCommand, LockSecretStoreResult, SecretMetadataView, SecretStoreSessionView,
    SecretStoreStatusView,
};
use zhuangsheng_tauri_adapter::{
    CommandResult, SensitiveChangePasswordInput, SensitivePutSecretInput, SensitiveSecretInput,
    TauriAdapter,
};

#[tauri::command]
pub async fn get_secret_store_status(
    state: State<'_, TauriAdapter>,
) -> CommandResult<SecretStoreStatusView> {
    state.get_secret_store_status().await
}

#[tauri::command]
pub async fn initialize_secret_store(
    state: State<'_, TauriAdapter>,
    input: SensitiveSecretInput,
) -> CommandResult<SecretStoreSessionView> {
    state.initialize_secret_store(input).await
}

#[tauri::command]
pub async fn unlock_secret_store(
    state: State<'_, TauriAdapter>,
    input: SensitiveSecretInput,
) -> CommandResult<SecretStoreSessionView> {
    state.unlock_secret_store(input).await
}

#[tauri::command]
pub async fn list_secrets(
    state: State<'_, TauriAdapter>,
) -> CommandResult<Vec<SecretMetadataView>> {
    state.list_secrets().await
}

#[tauri::command]
pub async fn put_secret(
    state: State<'_, TauriAdapter>,
    input: SensitivePutSecretInput,
) -> CommandResult<SecretMetadataView> {
    state.put_secret(input).await
}

#[tauri::command]
pub async fn lock_secret_store(
    state: State<'_, TauriAdapter>,
    command: LockSecretStoreCommand,
) -> CommandResult<LockSecretStoreResult> {
    state.lock_secret_store(command).await
}

#[tauri::command]
pub async fn change_master_password(
    state: State<'_, TauriAdapter>,
    input: SensitiveChangePasswordInput,
) -> CommandResult<SecretStoreSessionView> {
    state.change_master_password(input).await
}
