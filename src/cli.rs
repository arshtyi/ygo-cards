use std::path::PathBuf;

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

    /// Compare with ot.json and rd.json from a fixed previous release directory
    #[arg(long, conflicts_with = "no_previous_release")]
    pub(crate) previous_release_dir: Option<PathBuf>,

    /// Link to the fixed previous release in the report
    #[arg(long, requires = "previous_release_dir")]
    pub(crate) previous_release_url: Option<String>,

    /// Generate an initial release without a previous-release comparison
    #[arg(long)]
    pub(crate) no_previous_release: bool,
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
                ..Options::default()
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
    fn requires_an_unambiguous_previous_release_source() {
        assert!(
            Options::try_parse_from([
                "ygo-cards",
                "--previous-release-dir",
                "baseline",
                "--no-previous-release"
            ])
            .is_err()
        );
        assert!(
            Options::try_parse_from([
                "ygo-cards",
                "--previous-release-url",
                "https://example.test/releases/tag/1"
            ])
            .is_err()
        );
        let options = Options::try_parse_from([
            "ygo-cards",
            "--previous-release-dir",
            "baseline",
            "--previous-release-url",
            "https://example.test/releases/tag/1",
        ])
        .unwrap();
        assert_eq!(
            options.previous_release_dir,
            Some(PathBuf::from("baseline"))
        );
        assert!(!options.no_previous_release);
        assert!(
            Options::try_parse_from(["ygo-cards", "--no-previous-release"])
                .unwrap()
                .no_previous_release
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
