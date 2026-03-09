---
name: remi
description: This skill provides a work history. Always use this skill before answering any question that involves what the user has worked on, built, shipped, or contributed to — at any level of granularity or time range. Do not attempt to answer from memory or context alone; the data lives on disk and must be queried.
---

Trigger on any phrasing that implies knowledge of past work. Here are some examples of requests that require this skill — and anything similar:

**Engineering**
- "Look for optimizations in [repository], skipping anything I've already tried."
- "When might that [regression] have been introduced?"
- "Summarize my contributions to [project]."
- "Tell me when I last worked on [project] and for how long."

**Reporting & Admin**
- "Prepare my weekly meeting notes."
- "Fill my time sheets for the past two weeks."
- "Give me peer feedback on my work, as if your were my colleague."
- "How's my productivity going?"

**Career & Growth**
- "Identify key areas to improve my overall productivity."
- "Write my self-assessment and performance review."
- "Track my technical progression since I joined the company."
- "What's my coding style like?"

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

## Step 2 — Run `remi check`

`remi check` is the single command for the agent workflow. It installs the git hook (if needed), optionally scans repositories, queues recap jobs, and outputs all pending items.

**First time (or backfill):** provide the root directory where the user keeps their repositories:

```sh
remi check ~/src
```

A date range can be provided to limit scope:

```sh
remi check ~/src --start 2025-01-01
remi check ~/src --start 2025-01-01 --end 2025-06-01
```

**Subsequent runs** (after the hook has been recording commits automatically):

```sh
remi check
```

`remi check` outputs all pending items to stdout. Process each item using a subagent (to keep the main context clean):

**For each pending commit:**
1. Read the diff from the `remi check` output
2. Summarize it in one concise sentence
3. Call: `remi record commit <hash> "<summary>"`

**For each pending recap:**
1. Read the log content from the `remi check` output
2. Generate a recap in markdown (bullet points, key themes, notable achievements)
3. Write the recap directly to the output path shown
4. Call: `remi record recap <period>`

Process month recaps before year recaps, since year recaps may draw from monthly recap files.

## How the data is organized

All data lives under `~/.remi/`:

```
~/.remi/
  pending/
    abc1234.md        ← pending commit (written by hook or scan)
    recap-2025-01.md  ← pending month recap
    recap-2025.md     ← pending year recap
  2025/
    01/
      recap.md        ← monthly recap (written by agent)
      14-01-2025.md   ← daily log
      27-01-2025.md
    ...
    recap.md          ← yearly recap (written by agent)
  2026/
    03/
      07-03-2026.md
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
