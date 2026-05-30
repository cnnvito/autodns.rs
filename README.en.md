# autodns

[Simplified Chinese](README.md) | English

A small desktop DNS tool built with Rust and Tauri.

## Background

This project came from a very ordinary company-network use case. On the company network, some private records can only be resolved through the company DNS. At the same time, everyday browsing may still depend on results from the home network or public DNS. Different DNS servers can return different answers for the same domain, and some internal records are not published to the public internet at all.

The usual workaround is to edit the `hosts` file repeatedly or switch system DNS settings by hand. autodns is a desktop experiment built with GPT vibe coding: it puts local DNS, ordered upstreams, query history, and optional system DNS takeover into one interface, so switching contexts does not require constantly editing local files.

## How It Works

autodns starts a local DNS service and resolves domains through configured upstreams in order:

1. The request is sent to the first upstream DNS server.
2. If that upstream returns a valid record, autodns returns the result immediately.
3. If the upstream cannot be reached, or it has no record for the domain, autodns continues to the next upstream.
4. The process repeats in order until a result is found or all upstreams fail.

This makes scenarios like “company DNS first for private records, then home/public DNS for regular records” a stable configuration instead of a recurring `hosts` editing chore. Network switching is also covered by changing upstream order, but it was not the original core problem.

## Features

- Start, stop, and inspect a local DNS service
- Resolve through multiple upstream DNS servers in order
- View upstream health, failure count, and latency
- Run DNS lookups and inspect query history
- Configure routing rules, proxies, and cache behavior
- Optional system DNS takeover, disabled by default
- Chinese and English UI

## Tech Stack

- Rust
- Tauri 2
- React
- Vite
- Ant Design
- SQLite

## Quick Start

Install dependencies:

```bash
make install
```

Start the development desktop app:

```bash
make dev
```

Common checks:

```bash
make check
make test
```

Build an installer for the current platform:

```bash
make build
```

## Data and Safety

Runtime configuration and query history are stored in a local SQLite database. Desktop preferences are stored in a local `preferences.json`.

System DNS takeover is disabled by default. Network adapter DNS settings are modified only after the user enables and confirms the action. Before enabling it, make sure the target DNS server is available and that you know how to restore the original DNS settings through the app or the operating system.

## License

MIT License. See [LICENSE](LICENSE).
