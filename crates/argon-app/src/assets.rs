use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        AtSign,
        Brain,
        Check,
        CheckCheck,
        Circle,
        CircleDashed,
        CircleDot,
        Clock,
        CornerDownLeft,
        Crosshair,
        FileCode,
        FileDiff,
        GitBranch,
        GitCommitHorizontal,
        GitMerge,
        GitPullRequest,
        GitPullRequestClosed,
        GitPullRequestDraft,
        Hand,
        ListChecks,
        Lock,
        MessageSquare,
        MessageSquarePlus,
        MessageSquareReply,
        Minus,
        Pencil,
        RotateCcw,
        Send,
        ShieldAlert,
        Sparkles,
        Tag,
        Terminal,
        Trash,
        UserPlus,
        Users,
        Wrench,
        X
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
