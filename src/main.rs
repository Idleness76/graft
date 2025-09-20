use anyhow::Result;
use graft::run_demo3;

#[tokio::main]
async fn main() -> Result<()> {
    // run_demo1::run_demo1().await?;
    // run_demo2::run_demo2().await
    run_demo3::run_demo3().await
}
