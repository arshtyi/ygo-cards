use anyhow::Result;

fn main() -> Result<()> {
    let downloaded = ygo_cards::resources::download_all()?;

    for resource in downloaded {
        println!(
            "downloaded {:>8} bytes -> {}",
            resource.bytes,
            resource.path.display()
        );
    }

    Ok(())
}
