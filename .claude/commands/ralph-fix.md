Use the ralph-loop plugin to fix the problem below via an iterative fix-review loop.

## Problem

$ARGUMENTS

## Loop behavior

1. **Assess** — Understand the problem. Decide what changes are needed and whether each aspect is worth fixing (correct, in-scope, valuable) before writing any code.
2. **Fix** — Apply the changes you decided to make.
3. **Review** — Use the `codex-review-code` skill to get a code review of all changes (same review prompt style as `.github/workflows/codex-review.yml`).
4. **Evaluate** — Judge the review feedback. For each item, classify it as:
   - **Incorporate** — correct and valuable → fix it.
   - **Discard** — wrong, out-of-scope, or low-priority → dismiss it.
5. **Decide whether to loop again:**
   - If you incorporated any feedback and made new changes → go back to step 3 (re-review the updated code).
   - If Codex approves (no actionable feedback) → done.
   - If all remaining feedback was discarded (no new changes) → done.
6. **Repeat** until converged (no new changes made in a round).

## How to invoke ralph-loop

The ralph-loop skill runs a shell setup script that cannot handle backticks, special characters, or long multi-line arguments passed directly. To work around this:

1. **Clean up stale loop files first:**
   Remove any leftover files from previous runs so the user isn't prompted for permission on files that already exist:
   ```
   rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
   ```

2. **Write the prompt to a file:**
   Write the full problem description (the "Problem" section above plus the "Loop behavior" section) to `.claude/ralph-loop-prompt.local.md` using the Write tool.

3. **Invoke ralph-loop with a short, shell-safe argument:**
   Run the setup script directly via Bash tool: `CLAUDE_CODE_SESSION_ID="${CLAUDE_CODE_SESSION_ID:-}" /root/.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/scripts/setup-ralph-loop.sh "See .claude/ralph-loop-prompt.local.md"`
   Do NOT pass the raw $ARGUMENTS text to ralph-loop — it will break if the text contains backticks, quotes, or other shell metacharacters.

## When the loop ends

After the fix-review cycle converges, clean up the loop files:
```
rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
```

**Important:** Do NOT fix the problem directly. Write the prompt file, then invoke `/ralph-loop` and let it drive the fix-review cycle.
