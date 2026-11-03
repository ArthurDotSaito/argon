mod entries;
mod ranking;

use argon_core::{PrKey, PrState};
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, Icon, IndexPath, h_flex};
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, EventEmitter, FocusHandle, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Render, SharedString, Styled as _, Window,
    deferred, div, hsla, px,
};

use self::entries::{CommandSpec, PrEntry, command_specs};
use self::ranking::{Ranker, prs_first};
use crate::inbox::InboxModel;
use crate::ui;

pub const PALETTE_CONTEXT: &str = "Palette";

const PALETTE_WIDTH: f32 = 640.;
const TOP_OFFSET: f32 = 64.;
const LIST_MAX_HEIGHT: f32 = 372.;
const BACKDROP_ALPHA: f32 = 0.35;
const EMPTY_QUERY_PR_LIMIT: usize = 20;
const QUERY_PR_LIMIT: usize = 50;
const PLACEHOLDER: &str = "Buscar PRs, repos, comandos…";
const EMPTY_MESSAGE: &str = "Nada encontrado.";
const PRS_HEADING: &str = "PRs";
const COMMANDS_HEADING: &str = "Comandos";

pub enum PaletteEvent {
    Dismiss,
    OpenPr(PrKey),
}

#[derive(Clone, Copy)]
enum Target {
    Pr(usize),
    Command(usize),
}

struct Section {
    heading: &'static str,
    targets: Vec<Target>,
}

pub struct Palette {
    state: Entity<CommandState>,
    scope: FocusHandle,
    prs: Vec<PrEntry>,
    commands: Vec<CommandSpec>,
    ranker: Ranker,
    query: SharedString,
    dirty: bool,
    sections: Vec<Section>,
}

impl EventEmitter<PaletteEvent> for Palette {}

impl Palette {
    pub fn new(model: &Entity<InboxModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prs = model
            .read(cx)
            .items()
            .iter()
            .map(|item| PrEntry::new(&item.pr))
            .collect();
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        let mut this = Self {
            state,
            scope: cx.focus_handle(),
            prs,
            commands: command_specs(),
            ranker: Ranker::new(),
            query: SharedString::default(),
            dirty: false,
            sections: Vec::new(),
        };
        this.refresh();
        this
    }

    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.scope.contains_focused(window, cx)
    }

    fn refresh(&mut self) {
        self.dirty = false;
        let query = self.query.as_ref();
        let mut prs = self
            .ranker
            .rank(query, self.prs.iter().map(PrEntry::candidate));
        let commands = self
            .ranker
            .rank(query, self.commands.iter().map(CommandSpec::candidate));
        let prs_first = prs_first(query, &prs, &commands);
        prs.truncate(if query.trim().is_empty() {
            EMPTY_QUERY_PR_LIMIT
        } else {
            QUERY_PR_LIMIT
        });
        let pr_section = Section {
            heading: PRS_HEADING,
            targets: prs.iter().map(|scored| Target::Pr(scored.index)).collect(),
        };
        let command_section = Section {
            heading: COMMANDS_HEADING,
            targets: commands
                .iter()
                .map(|scored| Target::Command(scored.index))
                .collect(),
        };
        let ordered = if prs_first {
            [pr_section, command_section]
        } else {
            [command_section, pr_section]
        };
        self.sections = ordered
            .into_iter()
            .filter(|section| !section.targets.is_empty())
            .collect();
    }

    fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_string().into();
        self.dirty = true;
        cx.notify();
    }

    fn confirm(&mut self, path: IndexPath, cx: &mut Context<Self>) {
        let target = self
            .sections
            .get(path.section)
            .and_then(|section| section.targets.get(path.row));
        match target {
            Some(Target::Pr(ix)) => cx.emit(PaletteEvent::OpenPr(self.prs[*ix].key.clone())),
            _ => cx.emit(PaletteEvent::Dismiss),
        }
    }

    fn group(&self, section: &Section) -> CommandGroup {
        let items = section.targets.iter().map(|target| match *target {
            Target::Pr(ix) => pr_item(&self.prs[ix]),
            Target::Command(ix) => command_item(&self.commands[ix]),
        });
        CommandGroup::new().label(section.heading).items(items)
    }
}

fn command_item(spec: &CommandSpec) -> CommandItem {
    CommandItem::new()
        .label(spec.title)
        .icon(Icon::new(spec.icon))
        .action(spec.action.boxed_clone())
}

fn pr_item(entry: &PrEntry) -> CommandItem {
    let (title, reference) = (entry.title.clone(), entry.reference.clone());
    let (state, is_draft) = (entry.state, entry.is_draft);
    CommandItem::new()
        .label(title.clone())
        .child(move |_, cx| pr_row(&title, &reference, state, is_draft, cx))
}

fn pr_row(
    title: &SharedString,
    reference: &SharedString,
    state: PrState,
    is_draft: bool,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_2()
        .items_center()
        .child(ui::state_icon_for(state, is_draft, cx))
        .child(div().flex_1().min_w_0().truncate().child(title.clone()))
        .child(
            div()
                .flex_none()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(reference.clone()),
        )
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dirty {
            self.refresh();
        }
        let theme = cx.theme();
        let palette = cx.weak_entity();
        let on_query = palette.clone();
        let command = self.sections.iter().fold(
            Command::new(&self.state)
                .filterable(false)
                .bordered(false)
                .placeholder(PLACEHOLDER)
                .max_h(px(LIST_MAX_HEIGHT))
                .empty(|_, _, cx| {
                    div()
                        .p_3()
                        .text_color(cx.theme().muted_foreground)
                        .child(EMPTY_MESSAGE)
                })
                .on_query(move |query, _, cx| {
                    _ = on_query.update(cx, |this, cx| this.set_query(query, cx));
                })
                .on_confirm(move |path, _, cx| {
                    _ = palette.update(cx, |this, cx| this.confirm(path, cx));
                }),
            |command, section| command.group(self.group(section)),
        );
        let panel = div()
            .w(px(PALETTE_WIDTH))
            .max_w_full()
            .overflow_hidden()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(command);
        deferred(
            div()
                .id("palette")
                .key_context(PALETTE_CONTEXT)
                .track_focus(&self.scope)
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(TOP_OFFSET))
                .bg(hsla(0., 0., 0., BACKDROP_ALPHA))
                .on_action(cx.listener(|_, _: &Cancel, _, cx| cx.emit(PaletteEvent::Dismiss)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Dismiss)),
                )
                .child(panel),
        )
        .with_priority(1)
    }
}
