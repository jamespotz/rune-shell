//! Match Hyprland connector names to GPUI Wayland display identities.

/// The pinned GPUI Wayland backend derives display UUIDs from wl_output.name
/// using UUID v5 with the DNS namespace. Keep this aligned when updating GPUI.
fn connector_uuid(name: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, name.as_bytes())
}

pub(crate) struct Resolution<Id> {
    pub ready: Vec<(String, Id)>,
    pub pending: Vec<String>,
}

pub(crate) fn resolve<'a, Id: Copy>(
    connectors: impl IntoIterator<Item = &'a str>,
    displays: &[(uuid::Uuid, Id)],
) -> Resolution<Id> {
    let mut result = Resolution {
        ready: Vec::new(),
        pending: Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    for connector in connectors {
        if !seen.insert(connector) {
            continue;
        }
        let identity = connector_uuid(connector);
        if let Some((_, id)) = displays.iter().find(|(uuid, _)| *uuid == identity) {
            result.ready.push((connector.to_owned(), *id));
        } else {
            result.pending.push(connector.to_owned());
        }
    }
    result
}

pub(crate) fn connected(connectors: &[String], cx: &gpui::App) -> Resolution<gpui::DisplayId> {
    let displays: Vec<_> = cx
        .displays()
        .into_iter()
        .filter_map(|display| display.uuid().ok().map(|uuid| (uuid, display.id())))
        .collect();
    resolve(connectors.iter().map(String::as_str), &displays)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_outputs_by_identity_not_enumeration_order() {
        let displays = vec![(connector_uuid("DP-2"), 22), (connector_uuid("DP-1"), 11)];
        let result = resolve(["DP-1", "DP-2"], &displays);
        assert_eq!(result.ready, [("DP-1".into(), 11), ("DP-2".into(), 22)]);
        assert!(result.pending.is_empty());
    }

    #[test]
    fn delayed_or_removed_output_never_falls_back_to_another_display() {
        let requested = ["DP-1", "DP-2"];
        let waiting = resolve(requested, &[] as &[(uuid::Uuid, u32)]);
        assert!(waiting.ready.is_empty());
        assert_eq!(waiting.pending, ["DP-1", "DP-2"]);
        let discovered = resolve(requested, &[(connector_uuid("DP-2"), 22)]);
        assert_eq!(discovered.ready, [("DP-2".into(), 22)]);
        assert_eq!(discovered.pending, ["DP-1"]);
    }

    #[test]
    fn duplicate_connector_requests_open_only_one_bar() {
        let result = resolve(["DP-1", "DP-1"], &[(connector_uuid("DP-1"), 11)]);
        assert_eq!(result.ready, [("DP-1".into(), 11)]);
    }
}
