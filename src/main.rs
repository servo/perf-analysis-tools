mod analyse;
mod chromium;
mod collect;
mod combined;
mod dom;
mod json;
mod report;
mod servo;
mod shell;
mod study;
mod summary;

use std::env::args;

use jane_eyre::eyre::{self, bail};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> eyre::Result<()> {
    jane_eyre::install()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            EnvFilter::builder()
                .with_default_directive("analyse=info".parse()?)
                .from_env_lossy(),
        )
        .init();

    let mode = args().nth(1).unwrap();
    let args = args().skip(2).collect::<Vec<_>>();

    match &*mode {
        // Usage: collect <studies/example>
        "collect" => crate::collect::main(args),
        // Usage: analyse <studies/example>
        "analyse" => crate::analyse::main(args),
        // Usage: report <studies/example>
        "report" => crate::report::main(args),

        // Usage: analyse servo <trace.html ...>
        "servo" => crate::servo::main(args),
        // Usage: analyse chromium <page url> <chrome.json ...>
        "chromium" => crate::chromium::main(args),
        // Usage: analyse combined servo <trace.html ...> -- chromium <chrome.json ...>
        "combined" => crate::combined::main(args),

        other => bail!("Unknown command: {other}"),
    }
}
