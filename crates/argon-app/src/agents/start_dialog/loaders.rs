use std::sync::Arc;
use std::time::Instant;

use argon_agents::detect_all;
use argon_core::{SessionKind, ensure_default_templates, scan_templates};
use argon_github::IssueDetail;
use gpui_kit::{Context, Window};

use super::{
    Gathering, Load, PlanTarget, ReviewTarget, StartDialog, Subject, SummaryTarget, Target,
    app_services,
};
use crate::agents::context::{SummaryGathering, gather, gather_ci, gather_summary};
use crate::agents::detection::AgentOption;
use crate::agents::labels;

impl StartDialog {
    pub(super) fn load_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cached = Self::sessions(cx).and_then(|sessions| {
            sessions
                .read(cx)
                .detection()
                .fresh(Instant::now())
                .map(<[_]>::to_vec)
        });
        if let Some(options) = cached {
            self.set_agents(options, window, cx);
            self.refresh_listed_models(cx);
            self.refresh_usage(cx);
            return;
        }
        let Some(services) = app_services(cx) else {
            self.agents = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let config = self.config.clone();
        let job = services.spawn_runtime(async move { detect_all(&config).await });
        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| match result {
                Ok(detected) => {
                    let options: Vec<AgentOption> = detected
                        .into_iter()
                        .map(|(spec, support, detection)| {
                            AgentOption::new(spec, &support, &detection)
                        })
                        .collect();
                    let options = Self::with_listed_models(options, cx);
                    if let Some(sessions) = Self::sessions(cx) {
                        let cached = options.clone();
                        sessions.update(cx, |sessions, _| {
                            sessions.detection_mut().store(Instant::now(), cached)
                        });
                    }
                    this.set_agents(options, window, cx);
                    this.refresh_listed_models(cx);
                    this.refresh_usage(cx);
                }
                Err(error) => {
                    this.agents = Load::Failed(labels::detection_failed(&error.to_string()).into());
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn load_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((services, dir)) = app_services(cx)
            .zip(Self::sessions(cx).map(|sessions| sessions.read(cx).paths().prompts_dir()))
        else {
            self.templates = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let job = services.spawn_runtime(async move {
            tokio::task::spawn_blocking(move || {
                ensure_default_templates(&dir).and_then(|()| scan_templates(&dir))
            })
            .await
        });
        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| {
                let result = result
                    .map_err(|error| error.to_string())
                    .and_then(|joined| joined.map_err(|error| error.to_string()))
                    .and_then(|loaded| loaded.map_err(|error| error.to_string()));
                match result {
                    Ok(set) => {
                        this.invalid_templates = set.invalid;
                        this.set_templates(set.templates, window, cx);
                    }
                    Err(error) => {
                        this.templates = Load::Failed(labels::templates_failed(&error).into());
                        cx.notify();
                    }
                }
            });
        }));
    }

    pub(super) fn load_context(&mut self, target: Target, cx: &mut Context<Self>) {
        match target {
            Target::Review(target) => self.load_review_context(target, cx),
            Target::Summary(target) => self.load_summary_context(target, cx),
            Target::Plan(target) => self.load_plan_context(target, cx),
        }
    }

    fn load_plan_context(&mut self, target: PlanTarget, cx: &mut Context<Self>) {
        if let Some(detail) = target.detail {
            self.set_plan_detail(detail, cx);
            return;
        }
        let Some(services) = app_services(cx) else {
            self.context = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let id = target.issue.id.clone();
        let task = services.spawn_github(cx, target.issue.host.clone(), move |client| async move {
            client.issue_detail(&id).await
        });
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| match result {
                Ok(detail) => this.set_plan_detail(detail, cx),
                Err(error) => {
                    this.context = Load::Failed(labels::context_failed(&error.to_string()).into());
                    this.rerender(cx);
                }
            });
        }));
    }

    fn set_plan_detail(&mut self, detail: IssueDetail, cx: &mut Context<Self>) {
        self.subject = Subject::Issue(detail.issue.clone());
        self.context = Load::Ready(Gathering::Plan(Arc::new(detail)));
        self.rerender(cx);
    }

    fn load_review_context(&mut self, target: ReviewTarget, cx: &mut Context<Self>) {
        let Some(services) = app_services(cx) else {
            self.context = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let task = gather(&services, &target.pr, target.detail, cx);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(gathered) => {
                        this.subject = Subject::Pr(gathered.detail.pr.clone(), SessionKind::Review);
                        this.context = Load::Ready(Gathering::Review(Arc::new(gathered)));
                        if this.include_ci {
                            this.load_ci(cx);
                        }
                    }
                    Err(error) => {
                        this.context =
                            Load::Failed(labels::context_failed(&error.to_string()).into());
                    }
                }
                this.rerender(cx);
            });
        }));
    }

    fn load_summary_context(&mut self, target: SummaryTarget, cx: &mut Context<Self>) {
        let Some(services) = app_services(cx) else {
            self.context = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let pr = target.pr;
        let fetch = target.detail.is_none().then(|| {
            let id = pr.id.clone();
            services.spawn_github(cx, pr.host.clone(), move |client| async move {
                client.pr_detail(&id).await
            })
        });
        let configured = self
            .config
            .review
            .related_repos(&pr.host, &pr.repo)
            .to_vec();
        let gathering = gather_summary(target.detail, fetch, services.store(), configured);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = gathering.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(gathering) => this.set_summary_gathering(gathering),
                    Err(error) => {
                        this.context = Load::Failed(labels::context_failed(&error).into());
                    }
                }
                this.rerender(cx);
            });
        }));
    }

    fn set_summary_gathering(&mut self, gathering: SummaryGathering) {
        self.subject = Subject::Pr(gathering.detail.pr.clone(), SessionKind::Summary);
        self.refresh_project = gathering.overview.is_none();
        self.context = Load::Ready(Gathering::Summary(Arc::new(gathering)));
    }

    pub(super) fn load_ci(&mut self, cx: &mut Context<Self>) {
        let (Some(services), Some(Gathering::Review(gathered))) =
            (app_services(cx), self.context.ready())
        else {
            return;
        };
        if self.ci.is_some() {
            return;
        }
        self.ci = Some(Load::Loading);
        let task = gather_ci(&services, &gathered.detail, cx);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                this.ci = Some(match result {
                    Ok(text) => Load::Ready(text),
                    Err(error) => Load::Failed(labels::ci_failed(&error.to_string()).into()),
                });
                this.rerender(cx);
            });
        }));
    }
}
