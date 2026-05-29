use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

pub fn init_logging(verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("hypercore=debug")
    } else {
        EnvFilter::new("hypercore=info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        .with_target(false)
        .init();
}
