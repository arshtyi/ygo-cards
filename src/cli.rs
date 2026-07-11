use anyhow::{Result, bail};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) refresh_resources: bool,
    pub(crate) check_images: bool,
}

impl Options {
    pub(crate) fn parse() -> Result<Self> {
        Self::parse_from(std::env::args().skip(1))
    }

    fn parse_from<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut options = Self::default();

        for arg in args {
            match arg.as_ref() {
                "--refresh-resources" => options.refresh_resources = true,
                "--check-images" => options.check_images = true,
                arg => bail!("unknown option: {arg}"),
            }
        }

        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_arguments() {
        assert_eq!(Options::parse_from::<_, &str>([]).unwrap(), Options::default());
    }

    #[test]
    fn parses_supported_options() {
        assert_eq!(
            Options::parse_from(["--refresh-resources", "--check-images"]).unwrap(),
            Options {
                refresh_resources: true,
                check_images: true,
            }
        );
    }

    #[test]
    fn rejects_unknown_options() {
        let error = Options::parse_from(["--unknown"]).unwrap_err();

        assert_eq!(error.to_string(), "unknown option: --unknown");
    }
}
