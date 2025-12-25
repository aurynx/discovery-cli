# Architecture

## Overview

**Aurynx Discovery CLI** is a Rust binary that parses PHP code using tree-sitter and generates optimized attribute cache files.

## Components

### Parser (`src/parser.rs`)

- Uses tree-sitter to parse PHP syntax
- Extracts attributes from AST nodes
- Handles classes, methods, properties, parameters, and enums
- Generates structured metadata

### Scanner (`src/scanner.rs`)

- Walks directory tree to find PHP files
- Uses `ignore` crate for efficient .gitignore support
- Parallel file processing with `rayon`

### Daemon (`src/daemon.rs`)

- File watching with `notify` crate
- Unix socket for IPC commands
- Adaptive caching strategies:
  - **Memory**: < 10MB codebase (fastest, zero SSD wear)
  - **Hybrid**: 10-100MB codebase (memory + incremental disk)
  - **File**: > 100MB codebase (full disk cache)

### Writer (`src/writer.rs`)

- Converts Rust metadata to PHP array syntax
- Optimized for opcache
- Human-readable output

## Data Flow

```
PHP Files → Scanner → Parser → Metadata → Writer → PHP Cache
                ↓
         File Watcher (daemon mode)
                ↓
         Unix Socket (IPC)
```

## Cache Structure

Generated cache is a plain PHP array:

```php
<?php

declare(strict_types=1);

return [
    '\\Namespace\\ClassName' => [
        'file' => 'path/to/file.php',
        'type' => 'class',
        'attributes' => [...],
        'methods' => [...],
        'properties' => [...],
    ],
];
```

## Performance

- **Tree-sitter parsing**: Fast native parsing without runtime overhead
- **Parallel scanning**: Uses all CPU cores
- **Memory-first caching**: Zero SSD wear in development
- **Incremental updates**: Only reparse changed files

## Integration

This binary is designed to be used with the [aurynx/discovery](https://github.com/aurynx/discovery) PHP library for seamless integration with PHP applications.

## Examples

See [examples/](../examples/) directory for usage examples.
