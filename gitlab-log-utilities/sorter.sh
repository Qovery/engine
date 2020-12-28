#!/bin/bash
set -o pipefail

OUTPUT_DIR_TESTS_FILES="/builds/qovery/qovery-engine/gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"
touch JUNIT_REPORT
echo "-------------------"
echo "Reading $1 file"
echo "-------------------"

while IFS= read -r line; do
if jq -e . >/dev/null 2>&1 <<<"$line"; then
  echo "json======"
    # this is a json line
    if [ "$(echo "$line" | jq 'has("type")')" == "true" ]; then
      # it's junit report file
      echo $line >> "$JUNIT_REPORT"
      echo -e "\e[31$line"
    elif [ "$(echo "$line" | jq 'has("spans")')" == "true" ]; then
        # it's a test log line
        filename=$( echo $line | jq -r '.spans[].name' )
        echo "test log file found will be written into $filename"
        echo "$line" >> "$OUTPUT_DIR_TESTS_FILES/$filename"
    fi
else
    echo "not jsoin======"
    # test are not in json format ? print them all anyway
    echo $line
fi
done < $1

ls $OUTPUT_DIR_TESTS_FILES