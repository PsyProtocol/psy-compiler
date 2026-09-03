use semver::{Error, Prerelease, Version};

// Parse a semver compatible version string
pub(crate) fn parse_semver_compatible_version(version: &str) -> Result<Version, Error> {
    let mut version = Version::parse(version)?;
    version.pre = Prerelease::EMPTY;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_version_discards_prerelease_but_keeps_build_metadata() {
        let version = parse_semver_compatible_version("1.2.3-beta.4+build.9").unwrap();
        assert_eq!(version.to_string(), "1.2.3+build.9");
    }

    #[test]
    fn compatible_version_rejects_invalid_semver() {
        assert!(parse_semver_compatible_version("1.2").is_err());
        assert!(parse_semver_compatible_version("not-a-version").is_err());
    }

    #[test]
    fn compatible_version_preserves_zero_and_build_metadata() {
        assert_eq!(parse_semver_compatible_version("0.0.0").unwrap().to_string(), "0.0.0");
        assert_eq!(parse_semver_compatible_version("1.0.0+ci.42").unwrap().to_string(), "1.0.0+ci.42");
    }

    #[test]
    fn compatible_version_strips_only_the_prerelease_segment() {
        let version = parse_semver_compatible_version("2.0.0-rc.1").unwrap();
        assert_eq!(version.to_string(), "2.0.0");
        assert!(version.pre.is_empty());

        // Stripping must not touch adjacent build metadata.
        let version = parse_semver_compatible_version("0.1.0-alpha+x").unwrap();
        assert_eq!(version.to_string(), "0.1.0+x");
    }

    #[test]
    fn compatible_version_accepts_large_numeric_parts() {
        let version = parse_semver_compatible_version("18446744073709551615.0.0").unwrap();
        assert_eq!(version.major, u64::MAX);
    }

    #[test]
    fn compatible_version_rejects_leading_zero_numeric_parts() {
        assert!(parse_semver_compatible_version("01.2.3").is_err());
        assert!(parse_semver_compatible_version("1.02.3").is_err());
        assert!(parse_semver_compatible_version("1.2.03").is_err());
    }
}
