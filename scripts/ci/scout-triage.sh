#!/usr/bin/env bash
# Split a Scout JSON report into NEW findings and ones already reviewed in
# .github/scout-baseline.json, and print a Markdown summary.
#
# A baseline entry covers a finding when the detector matches, the file path
# ends with `file`, the flagged span text contains `match`, and the source
# line at the span (read from the checkout) contains `line`. Line numbers are
# never stored, so entries survive unrelated edits.
# Run from the repository root (Scout's file_path is repo-relative).
#
# Usage: scout-triage.sh REPORT_JSON BASELINE_JSON PACKAGE SUMMARY_MD
# Appends the Markdown summary to SUMMARY_MD and prints a ::warning
# annotation (stdout) per NEW critical finding. Never fails on
# findings (Scout is advisory); exits non-zero only on unreadable input.
set -euo pipefail

report=$1 baseline=$2 pkg=$3 summary=$4

# Source line at each finding's span start, in finding order.
lines=$(jq -r '.findings[] | "\(.file_path)\t\(.span | capture(":(?<l>[0-9]+):").l)"' "$report" |
  while IFS=$'\t' read -r file line; do
    if [ -f "$file" ]; then sed -n "${line}p" "$file"; else echo; fi
  done | jq -R . | jq -sc .)

triaged=$(jq -c --slurpfile b "$baseline" --argjson src "$lines" '
  def norm: gsub("\\s+"; " ");
  ([.categories[].vulnerabilities[] | {(.id): .severity}] | add // {}) as $sev
  | ($b[0].reviewed) as $rev
  | .findings | to_entries[]
  | .value as $f | ($src[.key] // "" | norm) as $line
  | ($rev | map(. as $r | select($r.detector == $f.vulnerability_id
                       and ($f.file_path | endswith($r.file))
                       and (($f.code_snippet | norm) | contains($r.match | norm))
                       and ($line | contains($r.line | norm))))
          | first) as $hit
  | {severity: ($sev[$f.vulnerability_id] // "unknown" | ascii_downcase),
     detector: $f.vulnerability_id, file: $f.file_path, span: $f.span,
     message: $f.error_message, reason: ($hit.reason // null)}
' "$report")

new=$(printf '%s\n' "$triaged" | jq -sc 'map(select(.reason == null))')
rev=$(printf '%s\n' "$triaged" | jq -sc 'map(select(.reason != null))')

{
echo "## Scout: $pkg"
echo
printf '%s' "$new" | jq -r '
  "**New (unreviewed) findings: \(length)**  " ,
  (group_by(.severity) | map("- \(.[0].severity): \(length)") | .[]),
  "",
  if length > 0 then
    "| Severity | Detector | Location | Message |",
    "| --- | --- | --- | --- |",
    (sort_by(.severity) | .[] | "| \(.severity) | `\(.detector)` | `\(.span)` | \(.message | gsub("\\|"; "\\|")) |")
  else empty end'
echo
printf '%s' "$rev" | jq -r '
  "<details><summary>Reviewed / accepted: \(length) (see .github/scout-baseline.json)</summary>",
  "",
  (.[] | "- `\(.detector)` at `\(.span)`: \(.reason)"),
  "</details>"'
} >> "$summary"

printf '%s' "$new" | jq -r '.[] | select(.severity == "critical")
  | "::warning title=Scout critical (unreviewed)::\(.detector) at \(.span): \(.message)"'
