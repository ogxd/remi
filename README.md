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
- Give me peer feedback on my work, as if your were my colleague.
- How's my productivity going?

**Career & Growth**
- Find key areas to improve my overall productivity.
- Write my self-assessment and performance review.
- Track my technical progression since I joined the company.
- What's my coding style like?

## Quick start

Remi is mainly meant to be used by your agent directly. Just install the [SKILL.md](https://raw.githubusercontent.com/ogxd/remi/master/SKILL.md) and it will take care of everything. First usage may take some time for the database to be initialized.

```sh
curl --create-dirs -o ~/.claude/skills/remi/SKILL.md https://raw.githubusercontent.com/ogxd/remi/master/SKILL.md
```

That's it, you don't need anything else 🪄 ! The skill will install the CLI if missing. To update remi, just update the skill with that same command.

## How it works

Remi uses an agent-driven protocol — it never calls an LLM directly. Instead:

1. A global `post-commit` git hook writes a pending file to `~/.remi/pending/` after every commit, containing the commit metadata and full diff.
2. When your agent runs `remi check`, it reads all pending files and outputs structured items to stdout — commits to summarize and recap periods to write.
3. The agent summarizes each diff and calls `remi record commit <hash> "<summary>"` to write the journal entry and clear the pending file.

Logs are organized under `~/.remi/`:

```
~/.remi/
  pending/
    a3f9c12.md        ← written by the hook, consumed by the agent
  2026/
    02/
      14-02-2026.md
      28-02-2026.md
      recap.md        ← written by the agent at end of month
    03/
      07-03-2026.md
  recap.md            ← written by the agent at end of year
```

Each journal entry looks like:

```markdown
- [14:32:10] Commit a3f9c12 on repository "my-project"
  - Message: Fix null pointer in auth handler
  - Description: Adds a nil check before dereferencing the user pointer in the auth middleware.
```