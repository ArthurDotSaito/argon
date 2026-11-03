mod announced;
mod installer;
pub(crate) mod labels;

use std::path::PathBuf;
use std::time::Duration;

use argon_core::DEFAULT_HOST;
use argon_github::{GithubError, Release, ReleaseAsset};
use chrono::{DateTime, Local};
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task};
use semver::Version;

use crate::agents::AgentSessions;
use crate::services::{AppServices, ServiceError};
use crate::workspace::Workspace;

const REPO: &str = "ArthurDotSaito/argon";
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const HTTP_FORBIDDEN: u16 = 403;

pub const CAN_INSTALL: bool = !cfg!(debug_assertions);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading {
        done: u64,
        total: u64,
        release: Release,
    },
    Installing,
    Unavailable(String),
    Failed(String),
}

impl UpdateState {
    pub fn status_text(&self) -> String {
        match self {
            Self::Idle => labels::IDLE.to_string(),
            Self::Checking => labels::CHECKING.to_string(),
            Self::UpToDate => labels::UP_TO_DATE.to_string(),
            Self::Available(release) => labels::available(&release.version.to_string()),
            Self::Downloading { done, total, .. } => {
                labels::downloading(download_percent(*done, *total))
            }
            Self::Installing => labels::INSTALLING.to_string(),
            Self::Unavailable(message) | Self::Failed(message) => message.clone(),
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing
        )
    }
}

pub enum UpdateEvent {
    Available(Release),
}

pub struct UpdateModel {
    state: UpdateState,
    offered: Option<Release>,
    last_checked: Option<DateTime<Local>>,
    task: Option<Task<()>>,
    _schedule: Task<()>,
}

impl EventEmitter<UpdateEvent> for UpdateModel {}

struct UpdateGlobal(Entity<UpdateModel>);

impl Global for UpdateGlobal {}

pub fn init(cx: &mut App) {
    let entity = cx.new(UpdateModel::new);
    cx.set_global(UpdateGlobal(entity));
}

pub fn entity(cx: &App) -> Entity<UpdateModel> {
    cx.global::<UpdateGlobal>().0.clone()
}

impl UpdateModel {
    fn new(cx: &mut Context<Self>) -> Self {
        let schedule = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK_DELAY).await;
            loop {
                if this.update(cx, |this, cx| this.check(cx)).is_err() {
                    return;
                }
                cx.background_executor().timer(CHECK_INTERVAL).await;
            }
        });
        Self {
            state: UpdateState::Idle,
            offered: None,
            last_checked: None,
            task: None,
            _schedule: schedule,
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn offered(&self) -> Option<&Release> {
        self.offered.as_ref()
    }

    pub fn last_checked(&self) -> Option<DateTime<Local>> {
        self.last_checked
    }

    pub fn status_bar_label(&self) -> Option<String> {
        status_bar_label(&self.state, self.offered.as_ref())
    }

    pub fn check(&mut self, cx: &mut Context<Self>) {
        if self.state.is_busy() {
            return;
        }
        self.state = UpdateState::Checking;
        cx.notify();
        let services = cx.global::<AppServices>().0.clone();
        let job = services.spawn_github(cx, DEFAULT_HOST, |client| async move {
            client.latest_release(REPO).await
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| this.finish_check(result, cx))
                .ok();
        }));
    }

    fn finish_check(
        &mut self,
        result: Result<Option<Release>, ServiceError>,
        cx: &mut Context<Self>,
    ) {
        self.state = state_after_check(&current_version(), result);
        match &self.state {
            UpdateState::Available(release) => self.offered = Some(release.clone()),
            UpdateState::UpToDate => self.offered = None,
            _ => {}
        }
        self.last_checked = Some(Local::now());
        self.announce_available(cx);
        cx.notify();
    }

    fn announce_available(&self, cx: &mut Context<Self>) {
        let UpdateState::Available(release) = &self.state else {
            return;
        };
        if !CAN_INSTALL {
            return;
        }
        let file = announced::file();
        if !announced::is_new(&file, &release.version) {
            return;
        }
        if let Err(error) = announced::write(&file, &release.version) {
            tracing::warn!(%error, "failed to remember the announced update");
        }
        cx.emit(UpdateEvent::Available(release.clone()));
    }

    pub fn install(&mut self, cx: &mut Context<Self>) {
        if !CAN_INSTALL || self.state.is_busy() {
            return;
        }
        let Some(release) = self.offered.clone() else {
            return;
        };
        let asset = match installable_asset(&release) {
            Ok(asset) => asset,
            Err(message) => {
                self.state = UpdateState::Failed(message.to_string());
                cx.notify();
                return;
            }
        };
        let total = asset.size;
        let installer = installer::path_for(&release.version);
        self.state = UpdateState::Downloading {
            done: 0,
            total,
            release,
        };
        cx.notify();

        let services = cx.global::<AppServices>().0.clone();
        let (progress_sender, mut progress) = unbounded::<u64>();
        let target = installer.clone();
        let job = services.spawn_github(cx, DEFAULT_HOST, move |client| async move {
            installer::reset_dir()?;
            client
                .download_asset(REPO, &asset, &target, move |done| {
                    progress_sender.unbounded_send(done).ok();
                })
                .await
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let track = async {
                let mut last_percent = 0;
                while let Some(done) = progress.next().await {
                    let percent = download_percent(done, total);
                    if percent == last_percent {
                        continue;
                    }
                    last_percent = percent;
                    if this
                        .update(cx, |this, cx| this.record_progress(done, cx))
                        .is_err()
                    {
                        return;
                    }
                }
            };
            let (result, ()) = futures::join!(job, track);
            let verified = this
                .update(cx, |this, cx| this.finish_download(result, cx))
                .unwrap_or(false);
            if verified {
                cx.update(|cx| quit_into_installer(installer, cx));
            }
        }));
    }

    fn record_progress(&mut self, received: u64, cx: &mut Context<Self>) {
        if let UpdateState::Downloading { done, .. } = &mut self.state {
            *done = received;
            cx.notify();
        }
    }

    fn finish_download(
        &mut self,
        downloaded: Result<(), ServiceError>,
        cx: &mut Context<Self>,
    ) -> bool {
        match downloaded {
            Ok(()) => {
                self.state = UpdateState::Installing;
                cx.notify();
                true
            }
            Err(error) => {
                self.install_failed(&error.to_string(), cx);
                false
            }
        }
    }

    fn install_failed(&mut self, error: &str, cx: &mut Context<Self>) {
        tracing::warn!(error, "update install failed");
        self.state = UpdateState::Failed(labels::install_failed(error));
        cx.notify();
    }
}

fn quit_into_installer(installer: PathBuf, cx: &mut App) {
    let launch = move |cx: &mut App| match installer::launch(&installer) {
        Ok(()) => cx.quit(),
        Err(error) => {
            entity(cx).update(cx, |model, cx| model.install_failed(&error.to_string(), cx));
            cx.defer(|cx| {
                if let Some(sessions) = AgentSessions::global(cx) {
                    sessions.update(cx, |sessions, _| sessions.reopen());
                }
            });
        }
    };
    match Workspace::global(cx).and_then(|workspace| workspace.upgrade()) {
        Some(workspace) => {
            workspace.update(cx, |workspace, cx| workspace.shutdown_then(launch, cx));
        }
        None => launch(cx),
    }
}

pub fn toast_body(release: &Release) -> String {
    labels::toast_body(&release.version.to_string())
}

fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("CARGO_PKG_VERSION is valid semver")
}

fn state_after_check(
    current: &Version,
    result: Result<Option<Release>, ServiceError>,
) -> UpdateState {
    match result {
        Ok(Some(release)) if release.version > *current && release.asset.is_some() => {
            UpdateState::Available(release)
        }
        Ok(Some(_)) => UpdateState::UpToDate,
        Ok(None) => UpdateState::Unavailable(labels::NO_RELEASE.to_string()),
        Err(ServiceError::Github(GithubError::InvalidReleaseTag(tag))) => {
            UpdateState::Unavailable(labels::invalid_version(&tag))
        }
        Err(ServiceError::Github(GithubError::AuthMissing { .. })) => {
            UpdateState::Unavailable(labels::AUTH_MISSING.to_string())
        }
        Err(ServiceError::Github(GithubError::Http(error)))
            if error.status().map(|status| status.as_u16()) == Some(HTTP_FORBIDDEN) =>
        {
            UpdateState::Unavailable(labels::FORBIDDEN.to_string())
        }
        Err(error) => {
            tracing::warn!(%error, "update check failed");
            UpdateState::Failed(labels::check_failed(&error.to_string()))
        }
    }
}

fn installable_asset(release: &Release) -> Result<ReleaseAsset, &'static str> {
    let asset = release.asset.clone().ok_or(labels::NO_INSTALLER)?;
    if asset.sha256.is_none() {
        return Err(labels::NO_CHECKSUM);
    }
    Ok(asset)
}

fn status_bar_label(state: &UpdateState, offered: Option<&Release>) -> Option<String> {
    match (state, offered) {
        (UpdateState::Downloading { done, total, .. }, _) => {
            Some(labels::downloading(download_percent(*done, *total)))
        }
        (_, Some(release)) => Some(labels::badge_available(&release.version.to_string())),
        _ => None,
    }
}

fn download_percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    (done.min(total).saturating_mul(100) / total) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn release_with(version: &str, asset: Option<ReleaseAsset>) -> Release {
        Release {
            version: Version::parse(version).unwrap(),
            tag: format!("v{version}"),
            html_url: "https://example.test/release".to_string(),
            asset,
        }
    }

    fn asset(sha256: Option<&str>) -> ReleaseAsset {
        ReleaseAsset {
            id: 1,
            name: "argon_x64-setup.exe".to_string(),
            size: 10,
            sha256: sha256.map(str::to_string),
        }
    }

    fn release(version: &str) -> Release {
        release_with(version, Some(asset(Some(DIGEST))))
    }

    fn current() -> Version {
        Version::parse("0.1.1").unwrap()
    }

    #[test]
    fn newer_release_is_available() {
        let state = state_after_check(&current(), Ok(Some(release("0.2.0"))));
        assert_eq!(state, UpdateState::Available(release("0.2.0")));
    }

    #[test]
    fn newer_release_without_an_installer_is_not_offered_yet() {
        let unfinished = release_with("0.2.0", None);
        assert_eq!(
            state_after_check(&current(), Ok(Some(unfinished))),
            UpdateState::UpToDate
        );
    }

    #[test]
    fn equal_or_older_release_is_up_to_date() {
        assert_eq!(
            state_after_check(&current(), Ok(Some(release("0.1.1")))),
            UpdateState::UpToDate
        );
        assert_eq!(
            state_after_check(&current(), Ok(Some(release("0.1.0")))),
            UpdateState::UpToDate
        );
    }

    #[test]
    fn missing_release_is_unavailable_not_up_to_date() {
        assert_eq!(
            state_after_check(&current(), Ok(None)),
            UpdateState::Unavailable(labels::NO_RELEASE.to_string())
        );
    }

    #[test]
    fn missing_auth_is_unavailable() {
        let error = ServiceError::Github(GithubError::AuthMissing {
            host: "github.com".to_string(),
        });
        assert_eq!(
            state_after_check(&current(), Err(error)),
            UpdateState::Unavailable(labels::AUTH_MISSING.to_string())
        );
    }

    #[test]
    fn invalid_release_tag_is_unavailable_and_names_the_tag() {
        let error = ServiceError::Github(GithubError::InvalidReleaseTag("nightly".to_string()));
        assert_eq!(
            state_after_check(&current(), Err(error)),
            UpdateState::Unavailable("Release com versão inválida: nightly".to_string())
        );
    }

    #[test]
    fn other_errors_fail_the_check() {
        let state = state_after_check(&current(), Err(ServiceError::Stopped));
        assert!(matches!(state, UpdateState::Failed(_)));
    }

    #[test]
    fn installing_requires_an_installer_with_a_checksum() {
        assert_eq!(installable_asset(&release("0.2.0")).unwrap().id, 1);
        assert_eq!(
            installable_asset(&release_with("0.2.0", None)),
            Err(labels::NO_INSTALLER)
        );
        assert_eq!(
            installable_asset(&release_with("0.2.0", Some(asset(None)))),
            Err(labels::NO_CHECKSUM)
        );
    }

    #[test]
    fn percent_is_clamped_and_safe_for_empty_totals() {
        assert_eq!(download_percent(0, 0), 0);
        assert_eq!(download_percent(42, 100), 42);
        assert_eq!(download_percent(1, 3), 33);
        assert_eq!(download_percent(500, 100), 100);
    }

    #[test]
    fn status_bar_label_follows_the_offer_and_survives_failures() {
        let offered = release("0.2.0");
        assert_eq!(status_bar_label(&UpdateState::UpToDate, None), None);
        assert_eq!(
            status_bar_label(&UpdateState::Failed("x".to_string()), Some(&offered)),
            Some("v0.2.0 disponível".to_string())
        );
        let downloading = UpdateState::Downloading {
            done: 42,
            total: 100,
            release: offered.clone(),
        };
        assert_eq!(
            status_bar_label(&downloading, Some(&offered)),
            Some("Baixando 42%".to_string())
        );
    }
}
