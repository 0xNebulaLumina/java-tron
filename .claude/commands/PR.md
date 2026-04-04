---
allowed-tools: Bash(git diff:*), Bash(git log:*), Bash(git branch:*), Bash(git merge-base:*)
description: help me write the title & desc for PR (branch $1-> $2)
---

## Your task

Write a PR title and description for merging branch **$1** into **$2**.

## First: gather context

Run these commands yourself before writing the PR:

1. `git log --oneline $2..$1` — commits on the source branch
2. `git diff $2...$1 --stat` — file change summary
3. If the stat is large or unclear, run `git diff $2...$1` for the full diff

### Steps

1. Run the context-gathering commands above.
2. Analyze the commits and diff to understand all changes.
3. Output a PR title and description in the format below.

### Output format

**IMPORTANT: Output raw Markdown source text inside a fenced code block (```markdown ... ```), NOT rendered Markdown. The user needs to copy-paste the raw Markdown into a PR form.**

```markdown
## Title
<short title, under 70 chars, in the style of existing PRs>

## Description
### Summary
<1-3 bullet points describing what changed and why>

### Changes
<bullet list of notable file/module changes>
```

### Style guide

- Match the tone of recent commits (check `git log --oneline -20 $2` if needed).
- Title should use conventional commit style if the repo does (e.g. `feat(travel): ...`, `fix(farm): ...`).
- Keep the description concise — reviewers skim, not read.
- Do NOT include Claude Code attribution in the output.



