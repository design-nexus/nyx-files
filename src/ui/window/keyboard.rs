// SPDX-License-Identifier: MIT





pub(crate) mod chooser;
pub(crate) mod chords;
mod commands;
mod escape;
pub(crate) mod files;
mod focus;
mod items;
mod preview;
mod prompts;
mod sidebar;

// None tries the next Files stage; Some(Proceed) gives the event to GTK instead.






// Key controllers retain the dispatcher and its window; unrealize breaks the cycle.





















