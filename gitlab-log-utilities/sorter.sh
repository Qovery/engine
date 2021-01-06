#!/bin/bash
set -o pipefail

OUTPUT_DIR_TESTS_FILES="/builds/qovery/qovery-engine/gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"

touch JUNIT_REPORT
while IFS= read -r line; do
if jq -e . >/dev/null 2>&1 <<<"$line"; then
    # this is a json line
    if [ $(echo "$line" | grep -c '"type":') -eq 1 ]; then
      # it's junit report file
      echo $line >> "$JUNIT_REPORT"
    elif [ $(echo "$line" | grep -c '"spans":') -eq 1 ]; then
        # it's a test log line
        filename=$( echo $line | sed -r 's/^(.+),"name":"test"(.+)$/\1\2/g' | jq -r '.spans[].name' )
        echo "$line" >> "$OUTPUT_DIR_TESTS_FILES/$filename"
    fi
fi
done < $1