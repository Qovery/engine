#!/bin/bash
set -o pipefail

OUTPUT_DIR_TESTS_FILES="gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"

mkdir -p $OUTPUT_DIR_TESTS_FILES

if [ -p /dev/stdin ]; then
  while IFS= read line; do
    # json line ?
    if jq -e . >/dev/null 2>&1 <<<"$line"; then
      # this is a json line
      if [ "$(echo "$line" | jq 'has("type")')" == "true" ]; then
        # it's junit report file
        echo $line >> "$JUNIT_REPORT"
        echo $line
      elif [ "$(echo "$line" | jq 'has("spans")')" == "true" ]; then
        # it's a test log line
        filename=$( echo $line | jq -r '.spans[].name' )
        echo "$line" >> "$OUTPUT_DIR_TESTS_FILES/$filename"
      fi
    elif
      # test are not in json format ? print them all anyway
      echo $line
    fi
  done
fi
