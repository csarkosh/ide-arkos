//! The route of one update check (telemetry spec, section 2.1): the first check of each UTC day
//! asks the first-party path, which logs one row and redirects to GitHub's manifest, and every
//! other check goes straight to GitHub as before. Pure, so the decision is tested apart from curl.
use ::telemetry::visits_app::Check;

/// The urls one check fetches its manifest from, in order: `first_party` with the check's query
/// first when `check` is the day's and a first-party endpoint is given, GitHub's manifest always
/// last. A check whose query does not build (a value the wire refuses) goes to GitHub alone, so
/// the day's check can never cost an update.
pub fn route(check: Option<Check>, first_party: Option<&str>, github: &str) -> Vec<String> {
    let first = check.zip(first_party).and_then(|(check, url)| Some(format!("{url}?{}", check.to_query().ok()?)));
    first.into_iter().chain([github.to_string()]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::telemetry::visits_app::{Arch, Channel, Envelope, Os};

    const GITHUB: &str = "https://github.com/csarkosh/ide-arkos/releases/latest/download/manifest.json";
    const FIRST: &str = "https://apps.csarko.sh/a/check";

    fn env() -> Envelope {
        Envelope { app: "arkos".into(), ver: "0.5.0".into(), bin: "daemon".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch: Channel::Release }
    }

    #[test]
    fn no_check_goes_to_github_alone() {
        assert_eq!(route(None, Some(FIRST), GITHUB), [GITHUB]);
        assert_eq!(route(None, None, GITHUB), [GITHUB]);
    }

    #[test]
    fn the_days_check_goes_first_party_first_and_github_last() {
        let check = Check { env: env(), rd: 3 };
        let urls = route(Some(check), Some(FIRST), GITHUB);
        assert_eq!(urls, [format!("{FIRST}?v=2&app=arkos&ver=0.5.0&bin=daemon&os=macos&osv=15&arch=arm64&ch=release&k=check&rd=3"), GITHUB.to_string()]);
        assert_eq!(route(Some(Check { env: env(), rd: -1 }), None, GITHUB), [GITHUB], "no first-party endpoint, no first-party request");
    }

    #[test]
    fn a_check_whose_query_does_not_build_goes_to_github_alone() {
        let check = Check { env: Envelope { ver: "not a version".into(), ..env() }, rd: -1 };
        assert!(check.to_query().is_err());
        assert_eq!(route(Some(check), Some(FIRST), GITHUB), [GITHUB]);
    }
}
