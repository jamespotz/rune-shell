//! Pending package updates (dnf, Flatpak, firmware) and `topgrade` runs.

fn count(items: impl Iterator) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// dnf5 colors its output even when piped and ignores `NO_COLOR`.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI: ESC [ params, ending in a letter.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `dnf check-update` lines shaped `name.arch version repo`; the
/// "Obsoleting packages" section that may follow repeats packages.
fn dnf_pending(stdout: &str) -> u32 {
    count(
        stdout
            .lines()
            .map(strip_ansi)
            .take_while(|line| !line.starts_with("Obsoleting"))
            .filter(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                matches!(fields.as_slice(), [name, _, _] if name.contains('.') && !line.starts_with(' '))
            }),
    )
}

fn flatpak_pending(stdout: &str) -> u32 {
    count(stdout.lines().filter(|line| !line.trim().is_empty()))
}

#[derive(serde::Deserialize)]
struct Fwupd {
    #[serde(rename = "Devices")]
    devices: Vec<FwupdDevice>,
}

#[derive(serde::Deserialize)]
struct FwupdDevice {
    #[serde(rename = "Releases", default)]
    releases: Vec<serde_json::Value>,
}

fn fwupd_pending(json: &str) -> Result<u32, String> {
    let parsed: Fwupd = serde_json::from_str(json)
        .map_err(|e| format!("fwupdmgr get-updates printed unexpected JSON: {e}"))?;
    Ok(count(
        parsed.devices.iter().filter(|d| !d.releases.is_empty()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DNF: &str = "Upgrades (available for \x1b[1m\x1b[32mreinstall\x1b[0m, available for \x1b[1m\x1b[34mupgrade\x1b[0m)\n\
\x1b[1m\x1b[34mImageMagick.x86_64             1:7.1.2.36-1.fc45 updates\x1b[0m\n\
\x1b[1m\x1b[34mffmpeg.x86_64                  9.0.2-4.fc45      rpmfusion-free-updates\x1b[0m\n\
stress-ng.x86_64               0.22.02-1.fc45    updates\n\
\n\
Obsoleting packages\n\
grub2-tools.x86_64             1:2.12-9.fc45     updates\n\
    grub2-tools-efi.x86_64     1:2.12-8.fc45     @System\n";

    #[test]
    fn dnf_counts_package_lines_before_obsoletes() {
        assert_eq!(dnf_pending(DNF), 3);
    }

    #[test]
    fn dnf_ignores_empty_and_garbage() {
        assert_eq!(dnf_pending(""), 0);
        assert_eq!(dnf_pending("Last metadata expiration check: 0:10:00 ago\n\x1b[\x1b[\n"), 0);
    }

    #[test]
    fn flatpak_counts_non_empty_lines() {
        assert_eq!(flatpak_pending("app.zen_browser.zen\n  \norg.gimp.GIMP\n"), 2);
        assert_eq!(flatpak_pending(""), 0);
    }

    #[test]
    fn fwupd_counts_devices_with_releases() {
        assert_eq!(fwupd_pending(r#"{"Devices":[]}"#), Ok(0));
        let json = r#"{"Devices":[{"Name":"UEFI","Releases":[{"Version":"2"}]},{"Name":"SSD","Releases":[]},{"Name":"Pad"}]}"#;
        assert_eq!(fwupd_pending(json), Ok(1));
    }

    #[test]
    fn fwupd_rejects_garbage() {
        assert!(fwupd_pending("No updatable devices").is_err());
        assert!(fwupd_pending(r#"{"Devices":5}"#).is_err());
    }
}
