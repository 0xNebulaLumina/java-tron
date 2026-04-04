---
allowed-tools: Bash(git diff:*), Bash(git log:*), Bash(git branch:*), Bash(git merge-base:*)
description: help me write the title & desc for PR (branch $1-> $2)
---

## Context

- Current branch: !`git branch --show-current`
- Base branch: $2
- Merge base: !`git merge-base HEAD $2`
- Commits on this branch since diverging from $2: !`git log --oneline $2..HEAD`
- Full diff against base: !`git diff $2...HEAD --stat`

## Your task

Write a PR title and description for merging branch **$1** into **$2**.

### Steps

1. Read the commit history and diff stat above to understand all changes.
2. If the stat is large or unclear, read the full diff (`git diff $2...HEAD`) to understand the details.
3. Output a PR title and description in the format below.

### Output format

```
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
