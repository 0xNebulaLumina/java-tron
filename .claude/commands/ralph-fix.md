Help me write a `ralph-loop:ralph-loop` to fix the problem below.

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

**Important:** Do NOT fix the problem directly. Instead, invoke the ralph-loop skill (`/ralph-loop`) and let it drive the fix-review cycle.
