Use the ralph-loop plugin to review and fix implementation issues via an iterative review-fix loop.

Unlike `/ralph-fix` (which starts from a known problem), this command starts by asking Codex to review the implementation, then fixes whatever it finds.

## Review scope

$ARGUMENTS

Pass the scope description directly to Codex — let Codex resolve it into concrete diff commands. Examples of valid scope descriptions:
- `uncommitted` or empty → uncommitted changes
- `last 3 commits` → last 3 commits
- `branch X vs branch Y` → diff between two branches
- `<commit-sha>` → a specific commit
- `<file-path>` → changes in a specific file

On re-review iterations (after fixes), tell Codex to also review any uncommitted working tree changes alongside the original scope. Codex sees the current state of the repo each time, so fixes are automatically visible.

## Loop behavior

1. **Review** — Run `codex:review` command asking Codex to review the scope. On re-review iterations, tell Codex to include uncommitted changes too.
   - If Codex finds no issues → output `<promise>ALL CLEAN</promise>` and STOP.
2. **Evaluate** — Judge the review feedback. For each item, classify it as:
   - **Incorporate** — correct and valuable → fix it.
   - **Discard** — wrong, out-of-scope, or low-priority → dismiss it.
3. **Decide:**
   - If nothing to incorporate (all discarded or no actionable feedback) → done, STOP.
   - Otherwise → proceed to step 4.
4. **Fix** — Apply the changes you decided to incorporate.
5. **Re-review** — You MUST go back to step 1. Do NOT emit `<promise>ALL CLEAN</promise>` here. Only Codex (in step 1) can declare ALL CLEAN — you cannot self-certify your own fixes.

## How to invoke ralph-loop

The ralph-loop skill runs a shell setup script that cannot handle backticks, special characters, or long multi-line arguments passed directly. To work around this:

1. **Clean up stale loop files first:**
   Remove any leftover files from previous runs so the user isn't prompted for permission on files that already exist:
   ```
   rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
   ```

2. **Write the prompt to a file:**
   Write the loop behavior and the raw scope description to `.claude/ralph-loop-prompt.local.md` using the Write tool. Do NOT include the "How to invoke" section — only the loop behavior and scope description.

3. **Invoke ralph-loop with a short, shell-safe argument:**
   Run the setup script directly via Bash tool: `CLAUDE_CODE_SESSION_ID="${CLAUDE_CODE_SESSION_ID:-}" "$HOME/.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/scripts/setup-ralph-loop.sh" "See .claude/ralph-loop-prompt.local.md"`
   Do NOT pass the raw review scope text to ralph-loop — it will break if the text contains backticks, quotes, or other shell metacharacters.

## When the loop ends

After the review-fix cycle converges, clean up the loop files:
```
rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
```

**Important:** Do NOT start fixing directly. Write the prompt file, then invoke `/ralph-loop` and let it drive the review-fix cycle.
