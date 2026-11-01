//! Terminal emulation and PTY plumbing for Tide.
//!
//! This crate uses `alacritty_terminal` for emulation and `portable-pty` for
//! process spawning, keeping the product-facing snapshot API small and
//! renderer-independent.

mod emulator;
mod grapheme;
mod keyboard;
mod pty;

pub use emulator::{
    CellSnapshot, CellSymbol, CursorSnapshot, TerminalColor, TerminalEmulator, TerminalModes,
    TerminalSnapshot,
};
pub use pty::{OutputSignal, PtyConfig, PtySession, TerminalError, check_terminal};
