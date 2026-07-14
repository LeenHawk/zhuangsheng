use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{fs, process::Command, time::timeout};
use zeroize::Zeroizing;
use zhuangsheng_core::application::ApplicationError;

use crate::source::invalid;

pub(super) struct GitCheckout<'a> {
    pub source_url: &'a str,
    pub source_ref: Option<&'a str>,
    pub destination: &'a Path,
    pub isolated_home: &'a Path,
    pub credential: Option<Zeroizing<String>>,
}

pub(super) async fn checkout(input: GitCheckout<'_>) -> Result<String, ApplicationError> {
    fs::create_dir_all(input.destination)
        .await
        .map_err(internal)?;
    fs::create_dir_all(input.isolated_home)
        .await
        .map_err(internal)?;
    let template = input.isolated_home.join("template");
    fs::create_dir_all(&template).await.map_err(internal)?;
    let config = input.isolated_home.join("config");
    if !config.exists() {
        fs::write(&config, b"").await.map_err(internal)?;
    }
    let askpass = create_askpass(input.isolated_home).await?;
    let credential = input.credential.as_ref().map(|value| value.as_str());
    run(
        input.destination,
        input.isolated_home,
        &template,
        &config,
        &askpass,
        credential,
        ["init", "--quiet"],
    )
    .await?;
    run(
        input.destination,
        input.isolated_home,
        &template,
        &config,
        &askpass,
        credential,
        ["remote", "add", "origin", input.source_url],
    )
    .await?;
    let source_ref = input.source_ref.unwrap_or("HEAD");
    run(
        input.destination,
        input.isolated_home,
        &template,
        &config,
        &askpass,
        credential,
        [
            "fetch",
            "--quiet",
            "--depth=1",
            "--no-tags",
            "origin",
            source_ref,
        ],
    )
    .await?;
    run(
        input.destination,
        input.isolated_home,
        &template,
        &config,
        &askpass,
        credential,
        ["checkout", "--quiet", "--detach", "FETCH_HEAD"],
    )
    .await?;
    let commit = run(
        input.destination,
        input.isolated_home,
        &template,
        &config,
        &askpass,
        credential,
        ["rev-parse", "HEAD"],
    )
    .await?;
    let commit = commit.trim().to_ascii_lowercase();
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(
            "plugin_git_commit",
            "Git returned an invalid commit identity",
        ));
    }
    Ok(commit)
}

async fn run<const N: usize>(
    current_dir: &Path,
    home: &Path,
    template: &Path,
    config: &Path,
    askpass: &Path,
    credential: Option<&str>,
    args: [&str; N],
) -> Result<String, ApplicationError> {
    let template_arg = format!("init.templateDir={}", template.display());
    let mut command = Command::new("git");
    command
        .current_dir(current_dir)
        .args([
            "-c",
            "protocol.file.allow=never",
            "-c",
            "protocol.ext.allow=never",
            "-c",
            &template_arg,
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_ASKPASS", askpass)
        .env("HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(value) = credential {
        command.env("ZS_GIT_PASSWORD", value);
    }
    let output = timeout(Duration::from_secs(120), command.output())
        .await
        .map_err(|_| invalid("plugin_git_timeout", "Git operation timed out"))?
        .map_err(|_| invalid("plugin_git_unavailable", "Git executable is unavailable"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(status = ?output.status.code(), error = %detail.trim(), "plugin Git operation failed");
        return Err(invalid(
            "plugin_git_failed",
            "Git could not fetch this plugin source",
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| invalid("plugin_git_output", "Git returned non-UTF-8 output"))
}

async fn create_askpass(home: &Path) -> Result<PathBuf, ApplicationError> {
    #[cfg(windows)]
    let (name, content) = ("askpass.cmd", "@echo %ZS_GIT_PASSWORD%\r\n");
    #[cfg(not(windows))]
    let (name, content) = (
        "askpass.sh",
        "#!/bin/sh\nprintf '%s' \"$ZS_GIT_PASSWORD\"\n",
    );
    let path = home.join(name);
    fs::write(&path, content).await.map_err(internal)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(internal)?;
    }
    Ok(path)
}

fn internal(_: std::io::Error) -> ApplicationError {
    ApplicationError::Internal
}
