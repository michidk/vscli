mod launch;
mod opts;
mod workspace;

use color_eyre::eyre::{eyre, Result};
use log::{debug, error, info};
use structopt::StructOpt;

use crate::{
    launch::LaunchConfig,
    opts::{LaunchBehaviour, Opts},
    workspace::Workspace,
};

/// Entry point for `vscli`.
fn main() -> Result<()> {
    let _ = color_eyre::install()?;

    let opts = Opts::from_args();

    let log_level = opts.verbosity;

    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(log_level.as_str()),
    )
    .init();

    debug!("Parsed Opts:\n{:#?}", opts);

    let ws = Workspace::from_path(opts.path.as_path())?;
    let lc = LaunchConfig::new(ws, opts.behaviour, opts.insiders, opts.args);
    lc.launch()?;

    Ok(())
}
