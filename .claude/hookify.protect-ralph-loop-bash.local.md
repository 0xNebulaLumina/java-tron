---
name: protect-ralph-loop-bash
enabled: true
event: bash
action: block
conditions:
  - field: command
    operator: regex_match
    pattern: (echo\s.*>|>>|sed\s+-i|awk\s.*>|cp\s|mv\s|rm\s|tee\s|truncate\s|chmod\s|chown\s|touch\s|mkdir\s|cat\s.*>).*ralph-loop
---

🚫 **BLOCKED: ralph-loop plugin is read-only**

You are attempting to use a Bash command to modify files inside the ralph-loop plugin directory.
This is not allowed — not even as a workaround to bypass Edit/Write restrictions.

Reading and executing scripts from this directory is fine.
Writing, deleting, moving, or modifying files is not.

Protected directory:
`/root/.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/`
