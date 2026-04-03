Use the ralph-loop plugin to implement tasks from design doc $1 and progress tracker $2 via an iterative implement-review loop.

## Loop behavior

Each iteration:

1. **Read** — Read $1 (design planning) and $2 (progress tracker). Identify unchecked `- [ ]` tasks.
   - If ALL tasks are `[x]` → output `<promise>IMPLEMENTATION COMPLETE</promise>` and STOP.
2. **Implement** — Work through unchecked tasks. Multiple related tasks per iteration is fine — use judgment on what forms a coherent chunk. Don't force yourself to stop mid-work if the next task is closely related.
3. **Verify** — Run `cargo check` (Rust) or the relevant build command. Run related tests. The goal is that the iteration ends in a compilable, test-passing state — but intermediate non-compilation during implementation is acceptable.
4. **Mark** completed tasks `[x]` in $2.
5. **Review** — Run `/codex:review` to check:
   - No over-marking: every `[x]` task is actually implemented
   - No under-marking: every code change has a corresponding `[x]` task
   - No skips: no doable unchecked tasks remain that should have been done in this chunk
6. **Fix** according to codex's feedback unless codex says all good.
7. **Converge** — If you made changes from the review → go back to step 5 (re-review). If no new changes → STOP. Ralph will restart you at step 1 for the next chunk.

## How to invoke ralph-loop

The ralph-loop skill runs a shell setup script that cannot handle backticks, special characters, or long multi-line arguments passed directly. To work around this:

1. **Clean up stale loop files first:**
   Remove any leftover files from previous runs so the user isn't prompted for permission on files that already exist:
   ```
   rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
   ```

2. **Write the prompt to a file:**
   Write the resolved loop behavior (with actual file paths substituted for $1 and $2) to `.claude/ralph-loop-prompt.local.md` using the Write tool. Do NOT include the "How to invoke" section — only the loop behavior and rules.

3. **Invoke ralph-loop with a short, shell-safe argument:**
   Run the setup script directly via Bash tool: `CLAUDE_CODE_SESSION_ID="${CLAUDE_CODE_SESSION_ID:-}" /root/.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/scripts/setup-ralph-loop.sh "See .claude/ralph-loop-prompt.local.md"`
   Do NOT pass the raw $1 or $2 text to ralph-loop — it will break if the text contains backticks, quotes, or other shell metacharacters.

## When the loop ends

After the implement-review cycle converges, clean up the loop files:
```
rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
```

**Important:** Do NOT implement tasks directly. Write the prompt file, then invoke `/ralph-loop` and let it drive the implement-review cycle.
