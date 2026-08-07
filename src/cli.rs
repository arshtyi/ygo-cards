use clap::Parser;

#[derive(Debug, Default, Parser, PartialEq, Eq)]
#[command(
    version,
    about = "Generate normalized Yu-Gi-Oh! card data for OT and RD environments"
)]
pub(crate) struct Options {
    /// Download fresh upstream resources before generating card data
    #[arg(long)]
    pub(crate) refresh_resources: bool,

    /// Check primary and alias images; failed checks use image 0 by default
    #[arg(long)]
    pub(crate) check_images: bool,

    /// Skip cards whose primary and alias images both fail (requires --check-images)
    #[arg(long, requires = "check_images")]
    pub(crate) skip_image_failures: bool,
}

impl Options {
    pub(crate) fn try_parse() -> Result<Self, clap::Error> {
        <Self as Parser>::try_parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_arguments() {
        assert_eq!(
            Options::try_parse_from(["ygo-cards"]).unwrap(),
            Options::default()
        );
    }

    #[test]
    fn parses_supported_options() {
        assert_eq!(
            Options::try_parse_from([
                "ygo-cards",
                "--refresh-resources",
                "--check-images",
                "--skip-image-failures",
            ])
            .unwrap(),
            Options {
                refresh_resources: true,
                check_images: true,
                skip_image_failures: true,
            }
        );
    }

    #[test]
    fn requires_image_checks_when_skipping_image_failures() {
        let error = Options::try_parse_from(["ygo-cards", "--skip-image-failures"]).unwrap_err();

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn provides_help() {
        let error = Options::try_parse_from(["ygo-cards", "--help"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
        let help = error.to_string();
        assert!(help.contains("--refresh-resources"));
        assert!(help.contains("--check-images"));
        assert!(help.contains("--skip-image-failures"));
    }

    #[test]
    fn provides_version() {
        let error = Options::try_parse_from(["ygo-cards", "--version"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
    }
}
