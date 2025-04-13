use clap::Parser;
use color_eyre::eyre::Context as _;

#[derive(clap::Parser)]
#[clap(version, about, long_about = None)]
struct Cli {
    #[clap(flatten)]
    args: stylers::BuildParamsBuilder,
}

fn main() -> color_eyre::Result<()> {
    tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    color_eyre::install()?;

    let args = Cli::parse();
    let build_params: stylers::BuildParams = args.args.finish()?;

    stylers::build(build_params).wrap_err("Failed to build using stylers")?;

    Ok(())
}
