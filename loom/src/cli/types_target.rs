//! `loom target` subcommands: review a move of the target branch loom did not
//! make.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum TargetCommands {
    /// Show the target branch guard: the accepted tip, the current tip, and any hold
    Status,

    /// Accept the current target tip after reviewing a move loom did not make;
    /// --to must name the current tip
    Accept {
        /// The commit to accept; it must be the target's current tip
        #[arg(long)]
        to: String,
    },
}
