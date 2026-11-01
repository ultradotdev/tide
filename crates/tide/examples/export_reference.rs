//! Compile the exact runtime registries into the documentation reference input.
fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--syntax-notices") => {
            print!("{}", tide_services::syntax::embedded_syntax_notices());
            return Ok(());
        }
        None => {}
        Some(_) => anyhow::bail!("Unknown reference export option"),
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&tide_ui::reference_catalog())?
    );
    Ok(())
}
