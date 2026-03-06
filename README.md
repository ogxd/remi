# remi

Your personal commit journal. Remi automatically logs every git commit you make across all repositories into a structured daily log.

## What it does

On first run, remi installs a global `post-commit` git hook. After that, every commit you make is appended to a daily markdown file:

```
~/.remi/
  2026/
    03/
      06-03-2026.md
```

Each entry looks like:

```markdown
- [14:32:10] [my-project] [a3f9c12] Fix null pointer in auth handler
- [16:05:44] [another-repo] [b81e203] Add dark mode support
```

## Installation

### Homebrew (macOS / Linux)

```sh
brew install ogxd/tap/remi
```

### Pre-built binary

**macOS / Linux:**
```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ogxd/remi/releases/latest/download/remi-installer.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://github.com/ogxd/remi/releases/latest/download/remi-installer.ps1 | iex
```

Or download a binary directly from the [releases page](https://github.com/ogxd/remi/releases).

### Via cargo

```sh
cargo install remi
```

## Setup

Run remi once to install the global git hook:

```sh
remi
```

That's it. All subsequent commits across all repositories will be logged automatically.
