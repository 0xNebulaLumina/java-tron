#!/usr/bin/env bash
set -euo pipefail

prompt_file="PROMPT.txt"
conformance_script="./scripts/ci/run_fixture_conformance.sh"

if [[ ! -f "$prompt_file" ]]; then
    echo "Missing $prompt_file" >&2
    exit 1
fi

if [[ ! -f "$conformance_script" ]]; then
    echo "Missing $conformance_script" >&2
    exit 1
fi

get_excluded_contracts() {
    awk '
        /case "\$1" in/ { in_block=1; next }
        in_block && /^[[:space:]]*\*\)/ { in_block=0; next }
        in_block {
            if ($0 ~ /^[[:space:]]*return[[:space:]]+0/ || $0 ~ /^[[:space:]]*;;/) {
                next
            }
            gsub(/^[[:space:]]+/, "", $0)
            gsub(/\|\\$/, "", $0)
            gsub(/\)$/, "", $0)
            if ($0 != "") {
                print $0
            }
        }
    ' "$conformance_script"
}

update_conformance_list() {
    local -a remaining=("$@")
    local new_list
    local tmp

    if (( ${#remaining[@]} > 0 )); then
        new_list="$(printf "%s\n" "${remaining[@]}")"
    else
        new_list=""
    fi

    tmp="$(mktemp)"

    awk -v new_list="$new_list" '
        BEGIN {
            n = split(new_list, items, "\n")
        }
        /case "\$1" in/ { print; in_block=1; next }
        in_block {
            if ($0 ~ /^[[:space:]]*\*\)[[:space:]]*$/) {
                if (!printed) {
                    if (n > 0) {
                        if (indent_list == "") {
                            indent_list = ""
                        }
                        for (i = 1; i <= n; i++) {
                            if (i < n) {
                                print indent_list items[i] "|\\"
                            } else {
                                print indent_list items[i] ")"
                            }
                        }
                        if (indent_return == "") {
                            indent_return = "                    "
                        }
                        if (indent_end == "") {
                            indent_end = "                    "
                        }
                        print indent_return "return 0"
                        print indent_end ";;"
                    }
                    printed = 1
                }
                in_block = 0
                print $0
                next
            }
            if (indent_list == "" && $0 !~ /^[[:space:]]*$/) {
                match($0, /^[[:space:]]*/)
                indent_list = substr($0, RSTART, RLENGTH)
            }
            if ($0 ~ /^[[:space:]]*return[[:space:]]+0/) {
                match($0, /^[[:space:]]*/)
                indent_return = substr($0, RSTART, RLENGTH)
                next
            }
            if ($0 ~ /^[[:space:]]*;;/) {
                if (indent_end == "") {
                    match($0, /^[[:space:]]*/)
                    indent_end = substr($0, RSTART, RLENGTH)
                }
                next
            }
            next
        }
        { print }
    ' "$conformance_script" > "$tmp"

    mv "$tmp" "$conformance_script"
}

replace_prompt_contract() {
    local contract="$1"
    local current

    current="$(perl -ne 'if (/\b([a-z0-9_]+_contract)\b/) { print $1; exit }' "$prompt_file")"
    if [[ -z "$current" ]]; then
        echo "No contract placeholder found in $prompt_file" >&2
        exit 1
    fi

    perl -0pi -e "s/\\b\\Q${current}\\E\\b/${contract}/g" "$prompt_file"
}

prev_hash="$(git rev-parse HEAD)"

while true; do
    mapfile -t contracts < <(get_excluded_contracts)
    if (( ${#contracts[@]} == 0 )); then
        echo "No excluded contracts left to unskip."
        break
    fi

    contract="${contracts[0]}"
    remaining=("${contracts[@]:1}")

    update_conformance_list "${remaining[@]}"
    replace_prompt_contract "$contract"

    codex exec \
        --model gpt-5.2 \
        --config model_reasoning_effort="xhigh" \
        --full-auto \
        "$(cat "$prompt_file")"

    curr_hash="$(git rev-parse HEAD)"

    if [[ "$curr_hash" == "$prev_hash" ]]; then
        break
    fi

    prev_hash="$curr_hash"
done
