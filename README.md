# Stack Up (Rust Implementation)

A Rust implementation of Stack Up - a simple deployment tool that performs given set of commands on multiple hosts in parallel. It reads Supfile, a YAML configuration file, which defines networks (groups of hosts), commands and targets.

## Installation

```bash
cargo install --path .
```

## Usage

```bash
sup-rs [OPTIONS] NETWORK COMMAND [...]
```

### Options

| Option            | Description                      |
|-------------------|----------------------------------|
| `-f Supfile`      | Custom path to Supfile           |
| `-e`, `--env=[]`  | Set environment variables        |
| `--only REGEXP`   | Filter hosts matching regexp     |
| `--except REGEXP` | Filter out hosts matching regexp |
| `--debug`, `-D`   | Enable debug/verbose mode        |
| `--disable-prefix`| Disable hostname prefix          |
| `--help`, `-h`    | Show help/usage                  |
| `--version`, `-v` | Print version                    |

## Features

- Execute commands on multiple hosts in parallel
- Interactive SSH sessions
- File uploads using tar
- Serial execution (rolling updates)
- Once-only execution
- Local command execution
- Script execution
- Environment variables
- Host filtering
- Target aliases

## Environment Variables

The following environment variables are automatically available in your Supfile:

- `$SUP_HOST` - Current host
- `$SUP_NETWORK` - Current network
- `$SUP_USER` - User who invoked sup command
- `$SUP_TIME` - Date/time of sup command invocation

## Examples

See [example_simple.yml](./example_simple.yml) for a basic example and [example_full.yml](./example_full.yml) for a comprehensive example with all features.

### Basic Usage

1. Run an interactive bash session:
```bash
sup-rs -f example_simple.yml dev bash
```

2. Run a command on all hosts:
```bash
sup-rs -f example_simple.yml dev ping
```

3. Upload files:
```bash
sup-rs -f example_simple.yml dev upload
```

### Advanced Usage

1. Run a rolling update (2 hosts at a time):
```bash
sup-rs -f example_full.yml prod rolling-update
```

2. Run on specific hosts only:
```bash
sup-rs -f example_full.yml prod ping --only "api.*"
```

3. Run with environment variables:
```bash
sup-rs -f example_full.yml prod deploy -e VERSION=v1.2.3,ENV=prod
```

## Common SSH Issues

If you encounter SSH connection issues:

1. Ensure your SSH agent is running:
```bash
eval $(ssh-agent)
```

2. Add your SSH key:
```bash
ssh-add ~/.ssh/id_rsa
```

3. Test direct SSH connection:
```bash
ssh user@host
```

## Development

1. Clone the repository
2. Run tests:
```bash
cargo test
```

3. Build:
```bash
cargo build --release
```

## License

Licensed under the MIT License. 