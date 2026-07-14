use std::path::{Path, PathBuf};

use url::Url;
use zhuangsheng_core::application::{ApplicationError, plugin::InspectGitPluginCommand};

pub(super) struct NormalizedSource {
    pub url: String,
    pub git_url: String,
    pub source_ref: Option<String>,
    pub credential_secret_id: Option<String>,
    pub credential_username: Option<String>,
}

pub(super) fn normalize_source(
    command: InspectGitPluginCommand,
) -> Result<NormalizedSource, ApplicationError> {
    let mut url = Url::parse(command.source_url.trim()).map_err(|_| {
        invalid(
            "plugin_git_url",
            "plugin source must be a valid HTTPS Git URL",
        )
    })?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid(
            "plugin_git_url",
            "plugin source must use HTTPS without embedded credentials",
        ));
    }
    url.set_fragment(None);
    let source_ref = command
        .source_ref
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if source_ref.as_ref().is_some_and(|value| !valid_ref(value)) {
        return Err(invalid("plugin_git_ref", "plugin Git ref is unsafe"));
    }
    let mut git_url = url.clone();
    if command.credential_secret_id.is_some() {
        let username = command.credential_username.as_deref().unwrap_or("git");
        if username.is_empty()
            || username.len() > 128
            || username
                .chars()
                .any(|value| matches!(value, '/' | '@' | ':'))
        {
            return Err(invalid(
                "plugin_git_username",
                "Git credential username is unsafe",
            ));
        }
        git_url
            .set_username(username)
            .map_err(|_| invalid("plugin_git_username", "Git credential username is unsafe"))?;
    }
    Ok(NormalizedSource {
        url: url.to_string(),
        git_url: git_url.to_string(),
        source_ref,
        credential_secret_id: command.credential_secret_id,
        credential_username: command.credential_username,
    })
}

pub(super) fn candidate_root(root: &Path, id: &str) -> PathBuf {
    root.join("staging").join(id)
}

pub(super) fn version_root(root: &Path, plugin_id: &str, version_id: &str) -> PathBuf {
    root.join("versions").join(plugin_id).join(version_id)
}

fn valid_ref(value: &str) -> bool {
    value.len() <= 200
        && !value.starts_with('-')
        && !(value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        && !value.contains("..")
        && !value.contains("@{")
        && !value.contains(['\\', ':', '~', '^', '?', '*', '['])
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
}

pub(super) fn invalid(code: &'static str, message: &'static str) -> ApplicationError {
    ApplicationError::InvalidArgument {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(source_ref: Option<&str>) -> InspectGitPluginCommand {
        InspectGitPluginCommand {
            source_url: "https://example.test/plugin.git".into(),
            source_ref: source_ref.map(str::to_owned),
            credential_secret_id: None,
            credential_username: None,
        }
    }

    #[test]
    fn source_ref_accepts_names_and_rejects_raw_object_ids() {
        assert_eq!(
            normalize_source(command(Some("release/v1")))
                .unwrap()
                .source_ref
                .as_deref(),
            Some("release/v1")
        );
        assert!(normalize_source(command(Some(&"a".repeat(40)))).is_err());
    }
}
