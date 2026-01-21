#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
prompt_file="$script_dir/YOLO_PROMPT.txt"
inputs_file="$script_dir/YOLO_PROMPT_INPUT.txt"

if [[ ! -f "$prompt_file" ]]; then
    echo "Missing $prompt_file" >&2
    exit 1
fi

if [[ ! -f "$inputs_file" ]]; then
    echo "Missing $inputs_file" >&2
    exit 1
fi

prompt_template="$(cat "$prompt_file")"

while IFS= read -r input || [[ -n "$input" ]]; do
    [[ -z "$input" ]] && continue
    [[ "$input" == \#* ]] && continue

    prompt="${prompt_template//\{PLACE_HOLDER\}/$input}"

    echo "Processing: $input"

    codex exec \
        --model gpt-5.2 \
        --config model_reasoning_effort="xhigh" \
        --yolo \
        "$prompt"
done < "$inputs_file"
