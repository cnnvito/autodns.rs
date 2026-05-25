# autodns desktop

autodns desktop is a Rust + Tauri desktop DNS manager.

It provides a GUI for managing local DNS runtime configuration. System DNS
takeover is optional and disabled by default.

## Requirements

- Rust
- Node.js and npm
- Tauri CLI

## Development

Install dependencies:

```bash
make install
```

Run the desktop app in development mode:

```bash
make dev
```

## Build

Build the current platform bundle:

```bash
make build
```

Run checks:

```bash
make check
```

Format and test Rust code:

```bash
make fmt
make test
```

## Windows MSI

Windows distribution uses an MSI installer.

Build on Windows:

```powershell
make windows-msi
```

Output:

```text
src-tauri\target\release\bundle\msi\*.msi
```

Tauri MSI builds require the WiX Toolset v3 on the Windows build machine.

## Release

GitHub Actions release builds are defined in:

```text
.github/workflows/release-tauri.yml
```

Create a release by pushing a tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow builds desktop bundles and uploads them to a draft GitHub Release.

## Project Layout

```text
frontend/    React + Vite frontend
src-tauri/   Rust + Tauri desktop app
scripts/     Build helper scripts
```

## Notes

- Runtime configuration is stored in a local SQLite database managed by the GUI.
- The user does not need to edit or know about config files.

## Performance TODO

Completed optimizations:

- Replaced the DNS cache's global `Mutex<HashMap<...>>` with `moka::sync::Cache`.
- Kept DNS TTL handling in the resolver: cache hits rewrite response TTLs to the remaining lifetime.
- Limited listener task fan-out with a shared `Semaphore`.
- Reused direct UDP upstream sockets and matched concurrent responses by rewritten DNS IDs.
- Shortened health-status locking with snapshot reads.
- Skipped health probes after recent real query success, staggered probe startup, and limited concurrent probes.
- Added health-check backoff while an upstream remains unhealthy.
- Reduced idle UI polling and removed tray refreshes from regular status polling.
- Used `arc-swap` for lock-free runtime resolver reads during hot reload.
- Reused DoQ endpoints and connections; open a new QUIC stream per query.
- Reused TCP and DoT connections with DNS ID-based pipelining.
- Reused SOCKS5 UDP associations separately from direct UDP reuse.
- Made desktop status updates event-driven with coalesced UI emissions.
- Reused cached system-DNS adapter data for apply/restore operations.
- Moved startup adapter refresh into a background warm-up after first paint.
- Replaced render-time config dirty checks with edit-time signature checks.

## License

MIT
