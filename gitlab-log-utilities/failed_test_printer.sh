#!/bin/bash

OUTPUT_DIR_TESTS_FILES="gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"

while IFS= read -r line
do
 test_status=$(echo $line | jq .event)
 case $test_status in
  "\"ok\"")
    test_name=$(echo $line | jq .name)
    echo "\e[32mPassed test : $test_name" ;;
  "\"failed\"")
    # check if a log file exist
    echo "\e[31mFailed test $test_name"
    test_name=$(echo $line | jq .name)
    for entry in "$OUTPUT_DIR_TESTS_FILES"*
    do
      f="$(basename $entry)"
      if [[ $test_name =~ $f ]]; then
        cat $entry
      fi
    done
  ;;
 esac
done < "$input"