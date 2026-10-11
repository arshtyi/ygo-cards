use std::num::NonZeroUsize;

use clap::Parser;

#[derive(Debug, Default, Parser, PartialEq, Eq)]
#[command(
    version,
    about = "Generate normalized Yu-Gi-Oh! card data for OT and RD environments"
)]
pub(crate) struct Options {
    /// Test at most N input cards per dataset; skip release comparison and diffs
    #[arg(long, value_name = "N")]
    pub(crate) limit: Option<NonZeroUsize>,

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
                "--limit",
                "10",
            ])
            .unwrap(),
            Options {
                limit: NonZeroUsize::new(10),
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
        assert!(help.contains("--limit <N>"));
        assert!(help.contains("--refresh-resources"));
        assert!(help.contains("--check-images"));
        assert!(help.contains("--skip-image-failures"));
        assert!(!help.contains("--publish"));
    }

    #[test]
    fn rejects_invalid_limits() {
        for value in ["0", "-1", "1.5", "all"] {
            assert!(Options::try_parse_from(["ygo-cards", "--limit", value]).is_err());
        }
    }

    #[test]
    fn provides_version() {
        let error = Options::try_parse_from(["ygo-cards", "--version"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
    }
}
