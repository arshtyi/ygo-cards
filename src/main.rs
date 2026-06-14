use anyhow::Result;

fn main() -> Result<()> {
    ygo_cards::resources::ensure_all()?;

    let report = ygo_cards::cards::ot::write_json()?;
    println!(
        "wrote {} cards -> {}",
        report.cards_written,
        report.path.display()
    );

    Ok(())
}
