#!/bin/bash
set -o pipefail

OUTPUT_DIR_TESTS_FILES="/builds/qovery/qovery-engine/gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"

echo -e "\e[33m****************************************************"
echo -e "\e[33mLog Sorter"
echo -e "\e[33m****************************************************"

touch JUNIT_REPORT
while IFS= read -r line; do
if jq -e . >/dev/null 2>&1 <<<"$line"; then
    # this is a json line
    if [ "$(echo "$line" | jq 'has("type")')" == "true" ]; then
      # it's junit report file
      echo $line >> "$JUNIT_REPORT"
    elif [ "$(echo "$line" | jq 'has("spans")')" == "true" ]; then
        # it's a test log line
        filename=$( echo $line | jq -r '.spans[].name' )
        echo "$line" >> "$OUTPUT_DIR_TESTS_FILES/$filename"
    fi
fi
done < $1

echo -e "\e[33m****************************************************"
echo -e "\e[33mGenerated Files"
echo -e "\e[33m****************************************************"
ls $OUTPUT_DIR_TESTS_FILES