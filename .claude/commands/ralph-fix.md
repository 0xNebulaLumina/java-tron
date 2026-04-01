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

1. **Write the prompt to a file first:**
   Write the full problem description (the "Problem" section above plus the "Loop behavior" section) to `.claude/.ralph-loop-prompt.local.md` using the Write tool.

2. **Invoke ralph-loop with a short, shell-safe argument:**
   Call `/ralph-loop:ralph-loop` with a simple argument like: `See .claude/.ralph-loop-prompt.local.md for full task`
   Do NOT pass the raw $ARGUMENTS text to ralph-loop — it will break if the text contains backticks, quotes, or other shell metacharacters.

**Important:** Do NOT fix the problem directly. Write the prompt file, then invoke `/ralph-loop` and let it drive the fix-review cycle.
