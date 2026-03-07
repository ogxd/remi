# remi skill

Use this skill whenever the user asks anything that requires knowledge of their past development activity — at any level of granularity or time range. This includes but is not limited to:

- Preparing a weekly or monthly report for a manager
- Writing a self-assessment or performance review
- Preparing for a compensation or promotion discussion
- Providing peer feedback or collecting evidence of contributions
- Tracking technical progression over time
- Summarizing work done on a specific project or technology
- Answering "what have I been working on lately?"

## Step 1 — Ensure remi is installed

Check if remi is available:

```sh
which remi
```

If not found, install it:

```sh
brew install ogxd/tap/remi
```

Or using the installer script:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ogxd/remi/releases/latest/download/remi-installer.sh | sh
```

Then run remi once to install the global git hook for future commits:

```sh
remi
```

## Step 2 — Initialize the activity database (first time only)

Check whether `~/.remi/` contains any year directories. If it is empty or missing, the database has never been populated. Ask user permission to scan a directory to backfill from existing repositories:

```sh
remi scan ~/src
```

Replace `~/src` with the root directory where the user keeps their repositories. A start date can be provided to limit scope:

```sh
remi scan ~/src --start 2025-01-01
```

This will discover all git repositories under that path, collect all commits authored by the current git user, and write daily log files. If a model is configured in `~/.remi/config.toml`, it will also generate LLM descriptions for commits that have no git body.

After scanning, generate recaps for past months and years:

```sh
remi recap
```

## How the data is organized

All data lives under `~/.remi/`:

```
~/.remi/
  2025/
    recap.md          ← yearly recap (LLM-generated)
    01/
      recap.md        ← monthly recap (LLM-generated)
      14-01-2025.md   ← daily log
      27-01-2025.md
    ...
  2026/
    03/
      07-03-2026.md
  config.toml
  remi.log
```

Each daily log contains one entry per commit:

```markdown
- [14:32:10] Commit a3f9c12 on repository "my-project"
  - Message: Fix null pointer in auth handler
  - Description: Adds a nil check before dereferencing the user pointer in the auth middleware.
```

## How to query the database efficiently

The data is structured for progressive summarization. Always start from the coarsest level and drill down only as needed — this keeps the context window small.

**For a broad question (e.g. "what did I work on in 2025?"):**
Read the yearly recap first: `~/.remi/2025/recap.md`

**For a monthly question (e.g. "what did I ship in January?"):**
Read the monthly recap: `~/.remi/2025/01/recap.md`

**For a specific week or day:**
Read the relevant daily log files directly, e.g. `~/.remi/2026/03/07-03-2026.md`

**For a question spanning multiple months:**
Read monthly recaps one by one, and only open daily logs if more detail is needed on a specific period.

Never load all daily logs at once — recaps exist precisely to avoid that. Only drill into daily logs when the question requires commit-level detail or when a recap does not exist yet for a period.
