//! The default prefix of the terminal title the plugin composes: an emoji for the OS and the
//! short host name (graph @nick/warpify, node #23).

/// The emoji for an `os-release` `ID`; anything unknown is a penguin.
fn emoji_for_id(id: &str) -> &'static str {
    match id {
        "ubuntu" => "🟠",
        "debian" => "🌀",
        "nixos" => "❄️",
        "fedora" => "🎩",
        "arch" => "🔷",
        "alpine" => "🏔",
        _ => "🐧",
    }
}

/// The `ID=` value of an `os-release` file's text, unquoted.
fn os_release_id(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.strip_prefix("ID="))
        .map(|v| v.trim().trim_matches(|c| c == '"' || c == '\''))
}

/// `<emoji> <short hostname>` for this machine: `os` is `std::env::consts::OS`, `os_release` the
/// text of `/etc/os-release` if it could be read, `hostname` as the machine reports it (the part
/// before the first dot is kept). Without a host name, the emoji alone.
#[must_use]
pub fn default_title_prefix(os: &str, os_release: Option<&str>, hostname: &str) -> String {
    let emoji = if os == "macos" {
        "🍎"
    } else {
        emoji_for_id(os_release.and_then(os_release_id).unwrap_or_default())
    };
    let host = hostname.trim().split('.').next().unwrap_or_default();
    if host.is_empty() {
        emoji.to_owned()
    } else {
        format!("{emoji} {host}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_emoji_follows_the_os_release_id() {
        let prefix = |id: &str| default_title_prefix("linux", Some(id), "box");
        assert_eq!(
            prefix("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n"),
            "🟠 box"
        );
        assert_eq!(prefix("ID=debian\n"), "🌀 box");
        assert_eq!(prefix("ID=nixos\n"), "❄️ box");
        assert_eq!(prefix("ID=\"fedora\"\n"), "🎩 box");
        assert_eq!(prefix("ID=arch\n"), "🔷 box");
        assert_eq!(prefix("ID=alpine\n"), "🏔 box");
        assert_eq!(prefix("ID=gentoo\n"), "🐧 box");
        assert_eq!(prefix("NAME=x\n"), "🐧 box");
    }

    #[test]
    fn macos_and_a_missing_file() {
        assert_eq!(default_title_prefix("macos", None, "mac"), "🍎 mac");
        assert_eq!(default_title_prefix("linux", None, "h"), "🐧 h");
    }

    #[test]
    fn the_host_name_is_the_short_one() {
        assert_eq!(
            default_title_prefix("linux", Some("ID=ubuntu"), "web1.example.com\n"),
            "🟠 web1"
        );
        assert_eq!(default_title_prefix("linux", Some("ID=ubuntu"), ""), "🟠");
    }
}
