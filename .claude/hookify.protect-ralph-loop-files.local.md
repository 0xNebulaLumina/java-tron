---
name: protect-ralph-loop-files
enabled: true
event: file
action: block
conditions:
  - field: file_path
    operator: regex_match
    pattern: /root/\.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/
---

🚫 **BLOCKED: ralph-loop plugin is read-only**

You are attempting to edit/write a file inside the ralph-loop plugin directory:
`/root/.claude/plugins/marketplaces/claude-plugins-official/plugins/ralph-loop/`

This directory is protected. Do NOT modify any files here.
If changes are needed, discuss with the user first.
