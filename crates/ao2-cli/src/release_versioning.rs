use std::cmp::Ordering;

pub(crate) fn compare_versions(left: &str, right: &str) -> Ordering {
    match (parse_semver(left), parse_semver(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => parse_version_tuple(left).cmp(&parse_version_tuple(right)),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedSemver {
    core: (u64, u64, u64),
    prerelease: Option<Vec<SemverIdentifier>>,
}

impl Ord for ParsedSemver {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

impl PartialOrd for ParsedSemver {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SemverIdentifier {
    Numeric(u64),
    Text(String),
}

impl Ord for SemverIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Numeric(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
        }
    }
}

impl PartialOrd for SemverIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_semver(version: &str) -> Option<ParsedSemver> {
    let without_build = version.trim_start_matches('v').split('+').next()?;
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(core, prerelease)| {
            (core, Some(prerelease))
        });
    let mut core_parts = core.split('.');
    let core = (
        core_parts.next()?.parse().ok()?,
        core_parts.next()?.parse().ok()?,
        core_parts.next()?.parse().ok()?,
    );
    if core_parts.next().is_some() {
        return None;
    }
    let prerelease = prerelease.map(|value| {
        value
            .split('.')
            .map(|part| {
                part.parse::<u64>()
                    .map(SemverIdentifier::Numeric)
                    .unwrap_or_else(|_| SemverIdentifier::Text(part.to_string()))
            })
            .collect()
    });
    Some(ParsedSemver { core, prerelease })
}

fn parse_version_tuple(version: &str) -> Vec<u64> {
    version
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod version_ordering_tests {
    use super::compare_versions;
    use std::cmp::Ordering;

    #[test]
    fn semver_prerelease_orders_before_stable() {
        assert_eq!(compare_versions("0.5.0-beta.1", "0.5.0"), Ordering::Less);
    }

    #[test]
    fn semver_prerelease_sequence_orders_numerically() {
        assert_eq!(
            compare_versions("0.5.0-beta.1", "0.5.0-beta.2"),
            Ordering::Less
        );
    }

    #[test]
    fn semver_release_orders_by_core_version() {
        assert_eq!(compare_versions("0.5.0", "0.4.81"), Ordering::Greater);
    }
}
