# 003 — Key the Bluetooth "Connect automatically" switch by device address

- **Status**: DONE
- **Commit**: 514eec5 (with uncommitted working tree)
- **Severity**: LOW
- **Category**: Interruptibility
- **Estimated scope**: 1 line in `crates/rune-ui/src/gpui_settings.rs`

## Problem

The switch's spring state is stored under its element ID, and the ID is the list position:

```rust
// crates/rune-ui/src/gpui_settings.rs:1330 — current
rows.push(self.toggle(
    &format!("bt-trust:{i}"),
    "Connect automatically",
    d.trusted,
    Msg::Bt(BtCommand::Trust(d.path.clone(), !d.trusted)),
));
```

When the device list reorders (a device connects or disconnects), position `i` belongs to another device. Its switch inherits the old animation state and slides by itself. AGENTS.md: "Prefer domain keys over positions when lists can reorder."

## Target

```rust
    &format!("bt-trust:{}", d.address),
```

## Steps

1. Replace `&format!("bt-trust:{i}")` with `&format!("bt-trust:{}", d.address)`. If `i` becomes unused, remove it from the enumerate pattern; clippy will flag it.

## Boundaries

- Only this ID. Do not rename other IDs.

## Verification

- **Mechanical**: fmt, clippy clean, `cargo test -p rune-ui`.
- **Feel check**: with two paired devices, connect the second so the list reorders. No trust switch animates unless you clicked it.
- **Done when**: the ID uses `d.address`.
