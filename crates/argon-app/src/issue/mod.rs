mod actions;
pub(crate) mod destination;
mod header;
mod issue_view;
mod labels;
mod loading;
mod tabs;
mod timeline;

pub(crate) use header::{IssueStatus, status_style, type_style};
pub use issue_view::IssueView;
