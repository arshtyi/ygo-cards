use anyhow::{Result, bail};

fn main() -> Result<()> {
    let options = Options::parse()?;

    if options.refresh_resources {
        for resource in ygo_cards::resources::download_all()? {
            println!(
                "downloaded {:>8} bytes -> {}",
                resource.bytes,
                resource.path.display()
            );
        }
    } else {
        ygo_cards::resources::ensure_all()?;
    }

    let report = ygo_cards::cards::ot::write_json(ygo_cards::cards::ot::BuildOptions {
        check_images: options.check_images,
    })?;
    println!(
        "wrote {} cards -> {}",
        report.cards_written,
        report.path.display()
    );

    let report = ygo_cards::cards::rd::write_json(ygo_cards::cards::rd::BuildOptions {
        check_images: options.check_images,
    })?;
    println!(
        "wrote {} cards -> {}",
        report.cards_written,
        report.path.display()
    );

    Ok(())
}

#[derive(Debug, Default)]
struct Options {
    refresh_resources: bool,
    check_images: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self::default();

        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--refresh-resources" => options.refresh_resources = true,
                "--check-images" => options.check_images = true,
                _ => bail!("unknown option: {arg}"),
            }
        }

        Ok(options)
    }
}
