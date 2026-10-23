//! arlang.tools: free Arabic language tools on the web.
//!
//! [`app`] holds the pages, [`tools`] the work each tool does, and [`queue`] the job queue the
//! tools share. The binary in `main.rs` reads the [`config`] and serves [`app::router`].

pub mod app;
pub mod config;
mod i18n;
mod icons;
mod meta;
pub mod queue;
pub mod tools;
