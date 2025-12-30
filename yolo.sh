#!/usr/bin/env bash
set -euo pipefail

prompt_file="YOLO_PROMPT.txt"

if [[ ! -f "$prompt_file" ]]; then
    echo "Missing $prompt_file" >&2
    exit 1
fi

prev_hash="$(git rev-parse HEAD)"

while true; do
    codex exec \
        --model gpt-5.2 \
        --config model_reasoning_effort="xhigh" \
        --yolo \
        "$(cat "$prompt_file")"

    curr_hash="$(git rev-parse HEAD)"

    if [[ "$curr_hash" == "$prev_hash" ]]; then
        break
    fi

    prev_hash="$curr_hash"
done
