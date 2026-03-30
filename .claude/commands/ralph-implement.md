Help me write a `ralph-loop:ralph-loop` to implement tasks from design doc $1 and progress tracker $2.

## Workflow per iteration

### Step 1: Read planning & progress
- Read $1 (design planning) and $2 (TODO/progress tracker)
- Identify the FIRST unchecked `- [ ]` task in $2 — that is your target for this iteration
- If ALL tasks are checked `- [x]`, proceed to Step 4 (final validation)

### Step 2: Implement the target task
- Implement ONLY the single target task identified in Step 1
- Follow the design guidance in $1 for how to implement it
- After implementation, run `cargo check` (for Rust changes) or the relevant build command to verify compilation
- If the task involves tests, run the specific test(s) to verify they pass
- On success: mark the task as `- [x]` in $2
- On failure: fix the issue within this iteration; do NOT move on to the next task until the current one compiles and passes

### Step 3: Exit iteration
- After completing one task, output: `Done with this iteration.`
- The Ralph Loop will restart you at Step 1 with the next unchecked task

### Step 4: Final validation (when all tasks are checked)

**Loop A — Correctness validation (`/codex-ask`):**
1. Run `/codex-ask` to cross-validate $2:
   - **No over-marking**: every `- [x]` task in $2 is actually implemented
   - **No under-marking**: every implemented change has its corresponding task marked in $2
   - **No skips**: no unchecked tasks remain that should have been done
2. Evaluate the feedback — for each item, decide: **incorporate** (correct, valuable) or **discard** (wrong, out-of-scope).
3. If you incorporated any feedback and made changes → go back to A.1.
4. If no changes needed → proceed to Loop B.

**Loop B — Code quality review (`/codex-review-code`):**
1. Run `/codex-review-code` on all uncommitted or recently committed changes.
2. Evaluate the feedback — for each item, decide: **incorporate** or **discard**.
3. If you incorporated any feedback and made changes → go back to B.1.
4. If no changes needed → validation complete.

- If both loops converge (no new changes), output: `<promise>IMPLEMENTATION COMPLETE</promise>`

## Rules
- ONE task per iteration — do not batch multiple tasks
- ALWAYS update $2 after completing a task — this is how the next iteration knows where to resume
- If a task is blocked by a prior task that isn't done, do the blocker first
- Follow commit conventions from CLAUDE.md: `<type>(<scope>): <subject>`

**Important:** Do NOT fix the problem directly. Instead, invoke the ralph-loop skill (`/ralph-loop`) and let it drive the fix-review cycle.
