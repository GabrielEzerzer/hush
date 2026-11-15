//! Terminal colours after the Codex CLI style guide: bold headers, dim secondary text, cyan for names the user acts
//! on, green for success, red for errors, magenta for the brand, in ANSI named colours. anstream drops the codes
//! unless the stream is a terminal and NO_COLOR is unset. Secret values never pass through here.

use anstyle::{AnsiColor, Style};

pub const HEADER: Style = Style::new().bold();
pub const DIM: Style = Style::new().dimmed();
pub const ACCENT: Style = AnsiColor::Cyan.on_default();
pub const OK: Style = AnsiColor::Green.on_default();
pub const ERR: Style = AnsiColor::Red.on_default().bold();
pub const BRAND: Style = AnsiColor::Magenta.on_default();

pub fn clap() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .header(HEADER)
        .usage(HEADER)
        .literal(ACCENT)
        .placeholder(DIM)
        .valid(OK)
        .invalid(ERR)
        .error(ERR)
        .context(DIM)
}

pub fn paint(style: Style, text: impl std::fmt::Display) -> String {
    format!("{style}{text}{style:#}")
}
