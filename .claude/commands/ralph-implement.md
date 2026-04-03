Use the ralph-loop plugin to implement tasks from design doc $1 and progress tracker $2 via an iterative implement-review loop.

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

**Loop A — Correctness validation**
1. Run `/codex:review` to cross-validate $2:
   - **No over-marking**: every `- [x]` task in $2 is actually implemented
   - **No under-marking**: every implemented change has its corresponding task marked in $2
   - **No skips**: no unchecked tasks remain that should have been done
2. Evaluate the feedback — for each item, decide: **incorporate** (correct, valuable) or **discard** (wrong, out-of-scope).
3. If you incorporated any feedback and made changes → go back to A.1.
4. If no changes needed → proceed to Loop B.

**Loop B — Code quality review**
1. Run `/codex:review` on all uncommitted or recently committed changes.
2. Evaluate the feedback — for each item, decide: **incorporate** or **discard**.
3. If you incorporated any feedback and made changes → go back to B.1.
4. If no changes needed → validation complete.

- If both loops converge (no new changes), output: `<promise>IMPLEMENTATION COMPLETE</promise>`

## Rules
- ONE task per iteration — do not batch multiple tasks
- ALWAYS update $2 after completing a task — this is how the next iteration knows where to resume
- If a task is blocked by a prior task that isn't done, do the blocker first
- Follow commit conventions from CLAUDE.md: `<type>(<scope>): <subject>`

## How to invoke ralph-loop

The ralph-loop skill runs a shell setup script that cannot handle backticks, special characters, or long multi-line arguments passed directly. To work around this:

1. **Clean up stale loop files first:**
   Remove any leftover files from previous runs so the user isn't prompted for permission on files that already exist:
   ```
   rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
   ```

2. **Write the prompt to a file:**
   Write the full workflow description (the "Workflow per iteration" and "Rules" sections above, plus the resolved values of $1 and $2) to `.claude/ralph-loop-prompt.local.md` using the Write tool.

3. **Invoke ralph-loop with a short, shell-safe argument:**
   Run the setup script directly via Bash tool: `CLAUDE_CODE_SESSION_ID="${CLAUDE_CODE_SESSION_ID:-}" /root/.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/scripts/setup-ralph-loop.sh "See .claude/ralph-loop-prompt.local.md"`
   Do NOT pass the raw $1 or $2 text to ralph-loop — it will break if the text contains backticks, quotes, or other shell metacharacters.

## When the loop ends

After the implement-review cycle completes, clean up the loop files:
```
rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
```

**Important:** Do NOT implement tasks directly. Write the prompt file, then invoke `/ralph-loop` and let it drive the implement-review cycle.
