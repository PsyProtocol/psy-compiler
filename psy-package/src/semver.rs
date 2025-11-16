use semver::{Error, Prerelease, Version};

// Parse a semver compatible version string
pub(crate) fn parse_semver_compatible_version(version: &str) -> Result<Version, Error> {
    let mut version = Version::parse(version)?;
    version.pre = Prerelease::EMPTY;
    Ok(version)
}
