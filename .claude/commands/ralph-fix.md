Help me write a `ralph-loop` the problem below.

## Problem

$ARGUMENTS

## Loop behavior

1. **Fix** — Apply a fix for the problem.
2. **Review** — Use the `codex-review-code` skill to get a code review of all changes (same review prompt style as `.github/workflows/codex-review.yml`).
3. **Evaluate** — Judge the review feedback:
   - If Codex flags real issues → go back to step 1 and fix them.
   - If Codex approves (no actionable feedback) → done.
   - If the feedback is wrong, out-of-scope, or low-priority → dismiss it and finish.
4. **Repeat** until converged.

**Important:** Do NOT fix the problem directly. Instead, invoke the ralph-loop skill (`/ralph-loop`) and let it drive the fix-review cycle.
