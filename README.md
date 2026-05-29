# RasFocus v2.0.0 - Single Binary Build

## Project Structure (Merged)

```
rasfocus/
├── src/
│   ├── main.rs        ← Rust backend (embeds index.html via include_str!)
│   └── index.html     ← Frontend UI (referenced by main.rs at compile time)
├── Cargo.toml
├── build.rs
├── tauri.conf.json
└── icons/             ← Add your icon files here
```

## How the merge works

`main.rs` embeds `src/index.html` at **compile time** using:
```rust
const INDEX_HTML: &str = include_str!("index.html");
```

This means:
- `index.html` is baked into the binary — no separate file needed at runtime
- Tauri still serves `src/index.html` during `tauri dev` (hot reload works)
- The `const INDEX_HTML` can be used for custom protocol serving if needed

## Build

```bash
cargo tauri build
```

## Dev

```bash
cargo tauri dev
```

## Icons Required (place in icons/)
- 32x32.png
- 128x128.png
- 128x128@2x.png
- icon.icns
- icon.ico
