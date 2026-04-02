---
name: codex-ask
description: This skill should be used when the user asks to "ask codex", "get codex's opinion", "ask codex to plan", "ask codex to debug", "codex help me", "what does codex think", or wants a second opinion from OpenAI Codex CLI on a question, implementation plan, or debugging session. Supports three modes - question, plan, and debug.
allowed-tools:
  - Bash
  - Read
  - Glob
  - Grep
  - Edit
  - Write
  - Agent
  - AskUserQuestion
---

Get an independent answer, implementation plan, or debugging analysis from OpenAI's Codex CLI using GPT-5.4 with maximum reasoning effort. Codex acts as a principal engineer providing a second opinion. You have more context than Codex — use your own judgment to decide what to incorporate.

## Invoking Codex

Use a **subagent** to run Codex CLI so that Codex's stdout noise (progress indicators, metadata, thinking output) stays out of your main context. You construct the prompt, the subagent executes Codex and returns only the clean response text, and you process the results.

### Spawning the subagent

Use the **Agent tool** to run Codex. Pass the subagent a prompt like this (substitute your actual `$PROMPT` and `$SANDBOX`):

```
Run OpenAI Codex CLI to answer a question. Execute this command:

TMPFILE=$(mktemp /tmp/codex-ask.XXXXXXXX)
ERRFILE=$(mktemp /tmp/codex-ask-err.XXXXXXXX)
PROMPT_FILE=$(mktemp /tmp/codex-prompt.XXXXXXXX)
trap 'rm -f "$TMPFILE" "$ERRFILE" "$PROMPT_FILE"' EXIT

cat > "$PROMPT_FILE" <<'CODEX_PROMPT_EOF'
<THE CODEX PROMPT>
CODEX_PROMPT_EOF

[ -f "$HOME/.codex/.env" ] && . "$HOME/.codex/.env"
codex exec \
  -m gpt-5.4 \
  -c 'model_reasoning_effort="xhigh"' \
  -s $SANDBOX \
  --ephemeral \
  -o "$TMPFILE" \
  "$(cat "$PROMPT_FILE")" 2> "$ERRFILE"

After the command completes:
- If exit code is 0: read $TMPFILE and return its FULL contents verbatim.
- If exit code is non-zero: read $ERRFILE and return its exact contents. Do not retry.

The trap handles cleanup in both cases — do not manually delete temp files.

Critical: use mktemp exactly as shown — do NOT add a file extension after the X's.
Return ONLY the response content or the error. No commentary, no summary, no wrapping.
```

**Important notes on the subagent call:**
- Use `description: "Run Codex ask"` (or similar short description)
- Select the sandbox level (`$SANDBOX`) based on the mode (see below)
- If the subagent returns an error, report it to the user and do not retry — there may be an auth or config issue they need to resolve

## Three Modes

Determine the mode from the user's request. Each mode uses a different sandbox level:

| Mode | Sandbox | Why |
|------|---------|-----|
| Question | `-s danger-full-access` | May need to run tests, execute build commands |
| Plan | `-s danger-full-access` | May need to run tests, execute build commands |
| Debug | `-s danger-full-access` | May need to run tests, try fixes, execute build commands |

### Choosing the mode
- If the user explicitly says "plan", "design", or "implement" → Plan
- If the user mentions a bug, failure, error, test failing, or unexpected behavior → Debug
- Otherwise → Question (the safe default)
- When ambiguous, briefly ask the user which mode they prefer.

### Mode 1: Question (ask)

For general questions about the codebase, architecture, or implementation approach.
**Sandbox:** `danger-full-access`

### Mode 2: Plan

For implementation planning — designing a feature, migration strategy, or refactoring approach.
**Sandbox:** `danger-full-access`

### Mode 3: Debug

For debugging — diagnosing failures, unexpected behavior, or test failures. Codex gets full access so it can run tests, try compilation, execute the failing code, and actively investigate.
**Sandbox:** `danger-full-access`

**Note:** The other 2 sandbox options `read-only` & `workspace-write` (read broadly, write within workspace) is available but not used by default. Override manually if a question or plan mode needs limited write access.

## Constructing the Prompt

Codex has shell access — it can run git commands and read files itself. Don't bloat the prompt by embedding file contents. Tell Codex *what* to look at and let it retrieve the code.

Every prompt must include:

### 1. Role and context (required)

Set the role and provide brief context about the codebase and the current situation.

### 2. The question or task (required)

State clearly what Codex should answer, plan, or debug.

### 3. Scope pointers (required)

Tell Codex which files, directories, or git commands to examine. Examples:
- "Read `src/auth/` to understand the current auth implementation"
- "Run `git log --oneline -20` to see recent changes"
- "Read `config.toml` and `src/main.rs` for the relevant context"

### 4. Output format (required)

Use the appropriate format for the mode:

**Question format:**
```
Return your answer as plain text in this format:

## Answer
Direct answer to the question with supporting reasoning.

## Evidence
Specific files, lines, or code patterns that support your answer.

## Caveats
Assumptions made or things you couldn't verify. If none, write "None."

Do NOT include metadata or commentary outside this format.
If you cannot access a file or run a command, say so clearly.
```

**Plan format:**
```
Return your plan as plain text in this format:

## Approach
High-level strategy in 2-3 sentences.

## Steps
Numbered implementation steps. For each step include:
- What to do
- Which files to create or modify
- Key implementation details

## Risks and Trade-offs
Potential issues, edge cases, or alternative approaches considered.

## Open Questions
Decisions that need human input before proceeding. If none, write "None."

Do NOT include metadata or commentary outside this format.
If you cannot access a file or run a command, say so clearly.
```

**Debug format:**
```
You have full shell access (danger-full-access) — you can read files, run builds, execute tests, and try fixes directly.

Return your analysis as plain text in this format:

## Diagnosis
Root cause or most likely cause of the issue.

## Evidence
Specific code paths, log patterns, or state that points to the root cause.
Include any test output, build errors, or runtime logs you collected.

## Fix
Concrete steps to fix the issue. Reference specific files and lines.
If you were able to try a fix, describe what you changed and whether it worked.

## Verification
How to verify the fix works — test commands you ran, expected vs actual output, or assertions.

Do NOT include metadata or commentary outside this format.
If you cannot access a file or run a command, say so clearly.
```

### Example prompts

**Question:**
```
You are a principal engineer examining a git repository. Context:

This is a Java-Rust hybrid blockchain node (java-tron). The storage layer is being migrated from embedded Java RocksDB to a remote Rust gRPC service.

**Question:** How does the StorageSpiFactory decide whether to use embedded or remote storage? What are all the configuration sources it checks?

Read `framework/src/main/java/org/tron/core/storage/spi/StorageSpiFactory.java` and any files it references. If you need more context, run git commands to explore.

[... format instructions ...]
```

**Plan:**
```
You are a principal engineer planning an implementation. Context:

This is a Java-Rust hybrid blockchain node. We need to add support for a new transaction type in the Rust execution engine.

**Task:** Design a plan to implement VoteWitnessContract execution in Rust, maintaining parity with the Java VoteWitnessActuator.

Read `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java` and `rust-backend/execution/src/` to understand both sides. If you need more context, explore freely.

[... format instructions ...]
```

**Debug:**
```
You are a principal engineer debugging an issue. Context:

This is a Java-Rust hybrid blockchain node. After enabling remote execution mode, state digests diverge at block 12345.

**Issue:** The SHA256 state digest from embedded (Java) execution differs from remote (Rust) execution for TransferContract transactions.

Run `git diff` to see recent changes. Read `framework/src/main/java/org/tron/core/execution/spi/RuntimeSpiImpl.java` and `rust-backend/execution/src/transfer.rs`. If you need more context, explore freely.

[... format instructions ...]
```

### Important:
- Always tell Codex it can run git commands if it needs more context.
- Always tell Codex to return an error message (not silently fail) if it cannot access something.

## Processing Codex's Response

### 1. Assess the response yourself

Do NOT blindly relay Codex's answer. You have far more context about:
- The broader codebase and its conventions
- The user's intent and constraints
- What has already been tried or discussed
- Project-specific patterns and trade-offs

For each point, decide:
- **Incorporate** — correct and valuable
- **Adapt** — right spirit, needs adjustment for this codebase
- **Discard** — wrong, irrelevant, or conflicts with known constraints

### 2. Report to the user

Present a clear summary:

**Codex's Take:**
- Key points from Codex's response

**My Assessment:**
- Which points are solid and why
- Which need adjustment and why
- Which to discard and why

**Synthesis:**
- Combined recommendation incorporating both perspectives
- Any points needing the user's judgment

### 3. Act if appropriate

- For **questions**: synthesize the answer, noting where you agree or disagree with Codex
- For **plans**: merge Codex's plan with your own knowledge, flag conflicts
- For **debug**: if the diagnosis is sound and you can fix it, offer to fix it

### 4. If Codex returns errors

Report the error clearly. Do NOT retry automatically. Suggest what the user might check (codex auth, file permissions, etc.).

## What NOT to Do

- Do not treat Codex's response as authoritative — it is a second opinion, not the final word.
- Do not call Codex repeatedly in a loop trying to get different answers.
- Do not embed large file contents in the prompt — let Codex read them itself.
- Do not skip your own assessment — the user relies on your judgment to filter and contextualize.
- Do not run `codex exec` directly in your own context — always use the subagent to contain stdout noise.
