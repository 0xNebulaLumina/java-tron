Help me write a `ralph-loop:ralph-loop` to fix the problem below.

## Problem

$ARGUMENTS

## Loop behavior

1. **Fix** — Apply a fix for the problem.
2. **Review** — Use the `codex-review-code` skill to get a code review of all changes (same review prompt style as `.github/workflows/codex-review.yml`).
3. **Evaluate** — Judge the review feedback. For each item, classify it as:
   - **Incorporate** — correct and valuable → fix it.
   - **Discard** — wrong, out-of-scope, or low-priority → dismiss it.
4. **Decide whether to loop again:**
   - If you incorporated any feedback and made new changes → go back to step 2 (re-review the updated code).
   - If Codex approves (no actionable feedback) → done.
   - If all remaining feedback was discarded (no new changes) → done.
5. **Repeat** until converged (no new changes made in a round).

**Important:** Do NOT fix the problem directly. Instead, invoke the ralph-loop skill (`/ralph-loop`) and let it drive the fix-review cycle.
