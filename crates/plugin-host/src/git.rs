use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gix::{
    clone::PrepareFetch,
    credentials::{helper::Action, protocol},
    progress::Discard,
    protocol::transport::client::blocking_io::http,
    remote::fetch::{Shallow, Tags},
    sec::identity::Account,
};
use tokio::{task, time::timeout};
use zeroize::Zeroizing;
use zhuangsheng_core::application::ApplicationError;

use crate::source::invalid;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) struct GitCheckout<'a> {
    pub source_url: &'a str,
    pub source_ref: Option<&'a str>,
    pub destination: &'a Path,
    pub credential: Option<Zeroizing<String>>,
}

struct OwnedCheckout {
    source_url: String,
    source_ref: Option<String>,
    destination: PathBuf,
    credential: Option<Zeroizing<String>>,
}

pub(super) async fn checkout(input: GitCheckout<'_>) -> Result<String, ApplicationError> {
    let input = OwnedCheckout {
        source_url: input.source_url.to_owned(),
        source_ref: input.source_ref.map(str::to_owned),
        destination: input.destination.to_owned(),
        credential: input.credential,
    };
    let interrupt = Arc::new(AtomicBool::new(false));
    let worker_interrupt = interrupt.clone();
    let mut worker = task::spawn_blocking(move || checkout_blocking(input, &worker_interrupt));
    match timeout(OPERATION_TIMEOUT, &mut worker).await {
        Ok(result) => result.map_err(|_| ApplicationError::Internal)?,
        Err(_) => {
            interrupt.store(true, Ordering::Relaxed);
            let _ = worker.await;
            Err(invalid("plugin_git_timeout", "Git operation timed out"))
        }
    }
}

fn checkout_blocking(
    input: OwnedCheckout,
    interrupt: &AtomicBool,
) -> Result<String, ApplicationError> {
    let mut prepare = PrepareFetch::new(
        input.source_url.as_str(),
        &input.destination,
        gix::create::Kind::WithWorktree,
        gix::create::Options::default(),
        gix::open::Options::isolated(),
    )
    .map_err(git_failed)?
    .with_shallow(Shallow::DepthAtRemote(NonZeroU32::MIN))
    .configure_remote(|remote| Ok(remote.with_fetch_tags(Tags::None)));
    if let Some(source_ref) = input.source_ref.as_deref() {
        prepare = prepare
            .with_ref_name(Some(source_ref))
            .map_err(|_| invalid("plugin_git_ref", "plugin Git ref must name a branch or tag"))?;
    }
    let credential = input.credential.map(Arc::new);
    prepare = prepare.configure_connection(move |connection| {
        connection.set_transport_options(Box::new(http_options()));
        let credential = credential.clone();
        connection.set_credentials(move |action| credential_action(action, credential.as_deref()));
        Ok(())
    });
    let (mut checkout, _) = prepare
        .fetch_then_checkout(Discard, interrupt)
        .map_err(git_failed)?;
    let (repository, _) = checkout
        .main_worktree(Discard, interrupt)
        .map_err(git_failed)?;
    let commit = repository.head_commit().map_err(git_failed)?.id.to_string();
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(
            "plugin_git_commit",
            "Git returned an unsupported commit identity",
        ));
    }
    Ok(commit)
}

fn credential_action(action: Action, credential: Option<&Zeroizing<String>>) -> protocol::Result {
    let Action::Get(context) = action else {
        return Ok(None);
    };
    let Some(password) = credential else {
        return Ok(None);
    };
    let username = context.username.clone().unwrap_or_else(|| "git".into());
    Ok(Some(protocol::Outcome {
        identity: Account {
            username,
            password: password.as_str().to_owned(),
            oauth_refresh_token: None,
        },
        next: context.into(),
    }))
}

fn http_options() -> http::Options {
    let backend = http::reqwest::Options {
        configure_request: Some(Box::new(|request| {
            *request.timeout_mut() = Some(OPERATION_TIMEOUT);
            Ok(())
        })),
    };
    http::Options {
        connect_timeout: Some(Duration::from_secs(20)),
        low_speed_limit_bytes_per_second: 1,
        low_speed_time_seconds: 30,
        user_agent: Some("zhuangsheng-plugin-host/0.0.1".into()),
        backend: Some(Arc::new(Mutex::new(backend))),
        ..Default::default()
    }
}

fn git_failed(error: impl std::fmt::Display) -> ApplicationError {
    tracing::warn!(%error, "embedded Git operation failed");
    invalid(
        "plugin_git_failed",
        "Git could not fetch this plugin source",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_provided_only_for_get_actions() {
        let mut context = gix::credentials::protocol::Context::default();
        context.username = Some("private-user".into());
        let outcome = credential_action(
            Action::Get(context),
            Some(&Zeroizing::new("private-token".into())),
        )
        .unwrap()
        .unwrap();
        assert_eq!(outcome.identity.username, "private-user");
        assert_eq!(outcome.identity.password, "private-token");
        assert!(
            credential_action(
                Action::Store(gix::bstr::BString::default()),
                Some(&Zeroizing::new("private-token".into())),
            )
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "requires public network access"]
    async fn embedded_git_clones_https_without_a_system_git_process() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("checkout");
        let commit = checkout(GitCheckout {
            source_url: "https://github.com/octocat/Hello-World.git",
            source_ref: Some("master"),
            destination: &destination,
            credential: None,
        })
        .await
        .unwrap();
        assert_eq!(commit.len(), 40);
        assert!(destination.join("README").is_file());
    }
}
