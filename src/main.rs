use graft::{run_demo1, run_demo2, run_demo3};
use miette::Result;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

fn init_tracing() {
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        // Log when spans are created/closed so we see instrumented async boundaries
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info,graft=debug"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();
}

fn init_miette() {
    // Pretty panic reports
    miette::set_panic_hook();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    init_miette();

    // Very small CLI: cargo run -- [demo]
    // Examples: cargo run -- demo1 | demo2 | demo3
    let which = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo3".to_string());
    match which.as_str() {
        "demo1" => run_demo1::run_demo1().await?,
        "demo2" => run_demo2::run_demo2().await?,
        "demo3" | _ => run_demo3::run_demo3().await?,
    }
    Ok(())
}
