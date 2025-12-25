# Aurynx | Discovery CLI

<p align="center">
  <img width="256" height="256" alt="Aurynx Mascot" src="https://github.com/user-attachments/assets/80a3ece6-5c50-4b01-9aee-7f086b55a0ef" />
</p>

<p align="center">
    <b>Fast PHP attribute discovery without runtime reflection</b>
</p>
<p align="center">tree-sitter • daemon mode • zero overhead — simple, safe, and blazing fast</p>

<p align="center">
  <a href="#installation">Installation</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#features">Features</a> •
  <a href="#usage">Usage</a> •
  <a href="./docs/architecture.md">Architecture</a>
</p>

---

## Overview

Rust-powered CLI tool for extracting PHP attributes from code using tree-sitter parsing. Zero runtime overhead.

**Backend for:** [aurynx/discovery](https://github.com/aurynx/discovery) PHP library

## Features

- ⚡ Native Rust + tree-sitter parsing
- 🎯 Extracts attributes from classes, methods, properties, parameters, enums
- 🔄 Daemon mode with file watching
- 💾 PHP array cache output (zero deserialization cost)

## Quick Start

```bash
# Scan once
aurynx discovery:scan --path src/ --output cache.php

# Watch mode (daemon)
aurynx discovery:scan --path src/ --output /tmp/cache.php --watch --socket /tmp/discovery.sock --pid /tmp/discovery.pid
```

## Installation

```bash
# macOS
curl -L https://github.com/aurynx/discovery-cli/releases/latest/download/aurynx-macos -o aurynx
chmod +x aurynx
sudo mv aurynx /usr/local/bin/

# Linux
curl -L https://github.com/aurynx/discovery-cli/releases/latest/download/aurynx-linux-x64 -o aurynx
chmod +x aurynx
sudo mv aurynx /usr/local/bin/
```

Or build from source:

```bash
git clone https://github.com/aurynx/discovery-cli.git
cd discovery-cli
cargo build --release
sudo cp target/release/aurynx /usr/local/bin/
```

## Usage

### Scan Once

```bash
aurynx discovery:scan --path src/ --output cache.php
```

### Daemon Mode

```bash
aurynx discovery:scan \
  --path src/ \
  --output /tmp/cache.php \
  --watch \
  --socket /tmp/discovery.sock \
  --pid /tmp/discovery.pid
```

Atomicity guarantee: Only one daemon per cache file. Prevents race conditions from concurrent PHP processes.

### IPC Protocol

**Raw text protocol** (zero overhead):

```bash
# Get PHP code
echo "getCacheCode" | nc -U /tmp/discovery.sock

# Health check
echo "ping" | nc -U /tmp/discovery.sock
```

**PHP integration:**

```php
$socket = stream_socket_client('unix:///tmp/discovery.sock');
fwrite($socket, "getCacheCode\n");
$phpCode = stream_get_contents($socket);
fclose($socket);
```

### CLI Options

```bash
  -p, --path <PATH>...     Directories to scan (required)
  -o, --output <OUTPUT>    Cache file path (required)
  -i, --ignore <PATTERN>   Ignore patterns (e.g. "vendor/*")
  -w, --watch              Daemon mode
  -s, --socket <PATH>      Unix socket (with --watch)
      --pid <PATH>         PID file (with --watch)
      --incremental        Only rescan changed files
      --pretty             Pretty print output
  -v, --verbose            Verbose logging
```

## Output Format

Generated cache is a plain PHP array:

```php
<?php declare(strict_types=1);

return [
    '\\App\\Controller\\UserController' => [
        'file' => 'src/Controller/UserController.php',
        'type' => 'class',
        'attributes' => [
            '\\Aurynx\\Routing\\Attributes\\Route' => [
                ['path' => '/api/users', 'methods' => ['GET', 'POST']],
            ],
        ],
        'methods' => [...],
        'properties' => [...],
    ],
];
```

## Troubleshooting

**Stale lock file:**

```bash
rm /tmp/aurynx-discovery-*.lock
```

**Force restart daemon:**

```bash
aurynx discovery:scan --path src/ --output cache.php --watch --force
```

## Documentation

- [Architecture](docs/architecture.md) — Design decisions and structure
- [File Size Limits](docs/file_size_limit.md) — Memory and security considerations
- [Flaky Tests](docs/flaky_tests.md) — Test stability notes

## License

MIT - see [LICENSE](LICENSE)

---

<p align="center">Crafted by Aurynx 🔮</p>
