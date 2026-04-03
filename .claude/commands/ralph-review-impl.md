Use the ralph-loop plugin to review and fix implementation issues via an iterative review-fix loop.

Unlike `/ralph-fix` (which starts from a known problem), this command starts by asking Codex to review the implementation, then fixes whatever it finds.

## Review scope

$ARGUMENTS

Interpret the scope argument to determine what Codex should review. Examples:
- `uncommitted` or empty → review uncommitted changes (`git diff` + `git diff --cached`)
- `last 3 commits` → review the last 3 commits (`git diff HEAD~3...HEAD`)
- `branch vs master` → review current branch against master (`git diff master...HEAD`)
- `<commit-sha>` → review a specific commit (`git show <sha>`)
- `<file-path>` → review changes in a specific file
- Any other description → use judgment to construct the right `git diff` invocation

Before starting the loop, resolve the scope into a concrete diff command and run it so you (and Codex) know exactly what code is under review.

## Loop behavior

1. **Review** — Run `/codex:review` on the resolved scope plus any fixes made in prior iterations. On the first iteration this is just the original scope; on subsequent iterations include the fix changes too (e.g. widen the diff range or pass the updated file list).
   - If Codex finds no issues → output `<promise>ALL CLEAN</promise>` and STOP.
2. **Evaluate** — Judge the review feedback. For each item, classify it as:
   - **Incorporate** — correct and valuable → fix it.
   - **Discard** — wrong, out-of-scope, or low-priority → dismiss it.
3. **Decide:**
   - If nothing to incorporate (all discarded or no actionable feedback) → done, STOP.
   - Otherwise → proceed to step 4.
4. **Fix** — Apply the changes you decided to incorporate.
5. **Re-review** — Go back to step 1.

## How to invoke ralph-loop

The ralph-loop skill runs a shell setup script that cannot handle backticks, special characters, or long multi-line arguments passed directly. To work around this:

1. **Clean up stale loop files first:**
   Remove any leftover files from previous runs so the user isn't prompted for permission on files that already exist:
   ```
   rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
   ```

2. **Write the prompt to a file:**
   Write the resolved loop behavior (with the review scope description and the concrete diff command) to `.claude/ralph-loop-prompt.local.md` using the Write tool. Do NOT include the "How to invoke" section — only the loop behavior and resolved scope.

3. **Invoke ralph-loop with a short, shell-safe argument:**
   Run the setup script directly via Bash tool: `CLAUDE_CODE_SESSION_ID="${CLAUDE_CODE_SESSION_ID:-}" /root/.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/scripts/setup-ralph-loop.sh "See .claude/ralph-loop-prompt.local.md"`
   Do NOT pass the raw review scope text to ralph-loop — it will break if the text contains backticks, quotes, or other shell metacharacters.

## When the loop ends

After the review-fix cycle converges, clean up the loop files:
```
rm -f .claude/ralph-loop-prompt.local.md .claude/ralph-loop.local.md
```

**Important:** Do NOT start fixing directly. Write the prompt file, then invoke `/ralph-loop` and let it drive the review-fix cycle.
