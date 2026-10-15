//! Release versions: stable `major.minor.patch`, and the bump policy (routine releases
//! are patches; a minor or major release is a milestone someone agreed on).

use std::{cmp::Ordering, fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionError {
    #[error("use a stable major.minor.patch version, not {0:?}")]
    Format(String),
    #[error("the release version must increase ({current} → {next})")]
    NotNewer { current: Version, next: Version },
    #[error("use {routine} for routine work, or the next minor or major milestone")]
    Skipped { routine: Version },
    #[error(
        "{0} is a milestone: minor and major releases need --milestone and an agreed major upgrade"
    )]
    Milestone(Version),
}

impl FromStr for Version {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || VersionError::Format(s.to_owned());
        let mut parts = s.split('.');
        let mut next = || -> Result<u32, VersionError> {
            let p = parts.next().ok_or_else(bad)?;
            // No signs, no leading zeros (old `releaseVersion`).
            if p.is_empty()
                || !p.bytes().all(|b| b.is_ascii_digit())
                || (p.len() > 1 && p.starts_with('0'))
            {
                return Err(bad());
            }
            p.parse().map_err(|_| bad())
        };
        let v = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        if parts.next().is_some() {
            return Err(bad());
        }
        Ok(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl serde::Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Version {
    /// Whether `next` may follow this version: the next patch, or with `milestone` the
    /// next minor or major (old `assertReleaseBump`).
    pub fn check_bump(self, next: Version, milestone: bool) -> Result<(), VersionError> {
        if next.cmp(&self) != Ordering::Greater {
            return Err(VersionError::NotNewer {
                current: self,
                next,
            });
        }
        let routine = Version {
            patch: self.patch + 1,
            ..self
        };
        if next == routine {
            return Ok(());
        }
        let minor = Version {
            major: self.major,
            minor: self.minor + 1,
            patch: 0,
        };
        let major = Version {
            major: self.major + 1,
            minor: 0,
            patch: 0,
        };
        if next != minor && next != major {
            return Err(VersionError::Skipped { routine });
        }
        if !milestone {
            return Err(VersionError::Milestone(next));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn versions_parse_strictly_and_order_numerically() {
        assert!(v("0.10.0") > v("0.9.9"));
        for bad in [
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "-1.2.3",
            "1.2.3-beta",
            "",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }

    #[test]
    fn bumps_follow_the_milestone_policy() {
        let current = v("0.2.3");
        assert!(current.check_bump(v("0.2.4"), false).is_ok());
        assert!(matches!(
            current.check_bump(v("0.3.0"), false),
            Err(VersionError::Milestone(_))
        ));
        assert!(current.check_bump(v("0.3.0"), true).is_ok());
        assert!(current.check_bump(v("1.0.0"), true).is_ok());
        assert!(matches!(
            current.check_bump(v("0.2.5"), true),
            Err(VersionError::Skipped { .. })
        ));
        assert!(matches!(
            current.check_bump(v("0.2.3"), false),
            Err(VersionError::NotNewer { .. })
        ));
    }
}
