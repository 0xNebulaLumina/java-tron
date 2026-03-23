---
name: codex-review-code
description: Get a second-opinion code review from OpenAI Codex CLI (GPT-5.4 xhigh). Use when the user asks for a code review, wants a second pair of eyes, or you want to validate significant changes.
allowed-tools:
  - Bash
  - Read
  - Glob
  - Grep
  - Edit
  - Write
  - Agent
  - AskUserQuestion
---

Get an independent code review from OpenAI's Codex CLI using GPT-5.4 with maximum reasoning effort. Codex acts as a principal engineer providing a second opinion. You have more context than Codex — use your own judgment to decide what feedback to incorporate.

**Stop hook integration:** A Stop hook triggers this skill when uncommitted changes exist. After completing a review (and any fixes), you must save a diff-hash marker (see "After the review is complete") so the hook won't re-trigger for already-reviewed changes. If the hook triggers again after you've saved the marker, it means NEW changes were made — go for a new review round.

## Overview

This skill uses a **subagent** to run Codex CLI so that Codex's stdout noise (progress indicators, metadata, thinking output) stays out of your main context. You construct the prompt, the subagent executes Codex and returns only the clean review text, and you process the results.

## Step 1: Construct the Codex prompt

Build the prompt string yourself — you have the conversation context to do this well. The prompt must include:

1. **Context summary** — a brief description of what the changes are for and the goal
2. **What to review** — tell Codex which git command to run or which files to read (don't embed diffs — Codex has shell access and can retrieve them itself)
3. **Focus areas** (optional) — specific concerns like "check error handling" or "verify SQL safety"
4. **Output format** — always include this block verbatim:

```
Return your review as plain text in this format:

## Critical Issues
Issues that must be fixed — bugs, security vulnerabilities, data loss risks.
List each with: file, line/section, issue description, and what should change.
If none, write "None found."

## Improvements
Code quality, performance, readability, or maintainability suggestions.
Tag each with a severity: [high] (real impact on correctness, performance, or security if left), [medium] (meaningful quality improvement), [low] (nitpick or style preference).
List each with: severity tag, file, line/section, what to improve, and why.

Be token-efficient. Describe changes concisely — don't rewrite large blocks of code.

## Positive Notes
Things done well that should be kept.

## Summary
2-3 sentence overall assessment.

Do NOT include metadata, conversation artifacts, or commentary outside this format.
If you cannot access a file or run a command, say so clearly instead of silently skipping it.
```

### Telling Codex what to review

Codex has shell access — let it retrieve code itself:
- **Uncommitted changes:** tell it to run `git diff` (or `git diff --cached` for staged)
- **Specific commits:** tell it the hash and to run `git show <hash>`
- **Specific files:** tell it the file paths to read
- **Scoped review:** e.g. "Run `git diff -- src/api/routes.py` and review those changes"

Always tell Codex it can run git commands for additional context, and to report errors clearly if it cannot access something.

### Example prompt

```
You are reviewing code changes in a git repository. Here is the context:

**Goal:** Adding rate limiting middleware to the Express API to prevent abuse of the /api/search endpoint.

**Focus areas:** Check that the rate limit configuration is correct, that the middleware ordering is right, and that error responses follow our existing API format.

You have access to git and the filesystem. If you need more context, run git commands to explore. If you cannot access something, say so clearly.

**What to review:** Run `git diff` to see the uncommitted changes, then review them.

Return your review as plain text in this format:
[... format instructions ...]
```

## Step 2: Spawn a subagent to run Codex

Use the **Agent tool** to run Codex in a subagent. This keeps all of Codex's stdout noise (progress, metadata, thinking) contained — only the clean review text comes back to you.

Pass the subagent a prompt like this (substitute your actual `$PROMPT`):

```
Run OpenAI Codex CLI to perform a code review. Execute this command:

TMPFILE=$(mktemp /tmp/codex-review.XXXXXXXX)
ERRFILE=$(mktemp /tmp/codex-review-err.XXXXXXXX)
PROMPT_FILE=$(mktemp /tmp/codex-prompt.XXXXXXXX)
trap 'rm -f "$TMPFILE" "$ERRFILE" "$PROMPT_FILE"' EXIT

cat > "$PROMPT_FILE" <<'CODEX_PROMPT_EOF'
<THE CODEX PROMPT>
CODEX_PROMPT_EOF

[ -f "$HOME/.codex/.env" ] && . "$HOME/.codex/.env"
codex exec \
  -m gpt-5.4 \
  -c 'model_reasoning_effort="xhigh"' \
  -s read-only \
  --ephemeral \
  -o "$TMPFILE" \
  "$(cat "$PROMPT_FILE")" 2> "$ERRFILE"

After the command completes:
- If exit code is 0: read $TMPFILE and return its FULL contents verbatim.
- If exit code is non-zero: read $ERRFILE and return its exact contents. Do not retry.

The trap handles cleanup in both cases — do not manually delete temp files.

Critical: use mktemp exactly as shown — do NOT add a file extension after the X's.
Return ONLY the review content or the error. No commentary, no summary, no wrapping.
```

**Important notes on the subagent call:**
- Use `description: "Run Codex code review"` (or similar short description)
- If the subagent returns an error, report it to the user and do not retry — there may be an auth or config issue they need to resolve

## Step 3: Process the review

When the subagent returns the review text, assess it yourself. Do NOT blindly apply Codex's suggestions — you have far more context about the codebase, the user's intent, and what's already been tried.

For each piece of feedback, decide:
- **Incorporate** — correct and valuable
- **Adapt** — right spirit, needs adjustment for this codebase
- **Discard** — wrong, irrelevant, or conflicts with known constraints

Fix what you can before reporting. Then present to the user:

**Codex Review Summary:**
- Brief overview of what Codex found

**My Assessment:**
- Which points you're incorporating and why
- Which you're discarding and why
- Any points you're unsure about

**Changes Made:**
- What you fixed based on the review

**For Your Decision:**
- Feedback requiring the user's judgment (architectural trade-offs, business logic)

## After the review is complete

Save a diff-hash marker so the Stop hook won't re-trigger for already-reviewed changes:

```bash
(git diff HEAD 2>/dev/null; git ls-files --others --exclude-standard 2>/dev/null) | md5sum | cut -d' ' -f1 > /tmp/.codex-last-reviewed-hash
```

If you make additional fixes after saving the hash, save it again.

## What NOT to do

- Do not treat Codex's feedback as authoritative — it is a second pair of eyes, not the final word.
- Do not call Codex repeatedly in a loop trying to get different answers.
- Do not send the entire repository — keep reviews focused on the relevant changes.
- Do not skip your own assessment — the user relies on your judgment to filter Codex's raw feedback.
- Do not run `codex exec` directly in your own context — always use the subagent to contain stdout noise.
