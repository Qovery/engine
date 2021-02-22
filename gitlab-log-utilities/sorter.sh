#!/bin/bash

OUTPUT_DIR_TESTS_FILES="/builds/qovery/qovery-engine/gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"
OUTPUT_FILE=$1

echo "[+] Sorting logs output to files:"

# generate junit file
grep '^{ "type": "' $OUTPUT_FILE > $JUNIT_REPORT
# generate logs files
for test_name in $(grep '"spans":' $OUTPUT_FILE | sed -r 's/^(.+),"name":"test"(.+)$/\1\2/g' | jq -r '.spans[].name' | sort | uniq) ; do
    echo "-> processing $test_name"
    grep $test_name $OUTPUT_FILE | grep '^{"timestamp"' > $OUTPUT_DIR_TESTS_FILES/$test_name
done
