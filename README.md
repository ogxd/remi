# REMI 🧠

Your agent can now reason over years of your engineering work. Ask it to:


**Engineering**
- Look for optimizations in [repository]. Skip things I've already tried.
- When did that [regression] might have been introduced?
- Summarize my contributions to [project].
- Tell me when I last worked on [project] and for how long.

**Reporting & Admin**
- Prepare my weekly meeting notes.
- Fill my time sheets for the past two weeks.
- Give peer feedback on my colleagues.
- How's my productivity going?

**Career & Growth**
- Write my self-assessment and performance review.
- Prepare me for a future promotion.
- Track my technical progression since I joined the company.
- What's my coding style like?

## Quick start

Remi is mainly meant to be used by your agent directly. Just install the [SKILL.md](https://raw.githubusercontent.com/ogxd/remi/master/SKILL.md) and it will take care of everything. First usage may take some time for the database to be initialized.

```sh
curl --create-dirs -o ~/.claude/skills/remi/SKILL.md https://raw.githubusercontent.com/ogxd/remi/master/SKILL.md
```

That's it... 🤯

## Manual installation

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

### LLM summarization (optional)

To enable automatic commit descriptions, create `~/.remi/config.toml`:

```toml
model = "gemini-2.0-flash"
```

Any model supported by [genai](https://github.com/jeremychone/rust-genai) works: OpenAI, Anthropic, Gemini, and others. The corresponding API key must be available as an environment variable (e.g. `GEMINI_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`).

## Commands

Remi can be used manually, but also by an agent directly.

### `remi scan <path>`

Backfills the journal by scanning a directory tree for git repositories and collecting your past commits.

```sh
remi scan ~/src
remi scan ~/src --start 2026-01-01 --end 2026-03-01
```

### `remi recap`

(Re)generates `recap.md` files for all complete past months and years. Recaps are also auto-generated whenever remi detects a new month or year directory.

```sh
remi recap
remi recap --start 2026-01-01 --end 2026-02-28
```

Only periods that are fully in the past and fully within the given date range are recapped.

## How it works

On first run, remi installs a global `post-commit` git hook. After that, every commit you make is appended and summarized to a daily markdown file.

Logs are organized under `~/.remi/`:

```
~/.remi/
  2026/
    02/
      14-02-2026.md
      28-02-2026.md
      recap.md        ← auto-generated at end of month
    03/
      07-03-2026.md
  recap.md            ← auto-generated at end of year
```

Each entry looks like:

```markdown
- [14:32:10] Commit a3f9c12 on repository "my-project"
  - Message: Fix null pointer in auth handler
  - Description: Adds a nil check before dereferencing the user pointer in the auth middleware.
```

If you provide a git commit body, it is used as the description. Otherwise, if a model is configured, remi calls the LLM to summarize the diff.