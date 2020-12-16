#!/bin/bash

OUTPUT_DIR_TESTS_FILES="../gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"
echo "===================================================="
echo "OUTPUT TESTS GENERATED"
echo "===================================================="
    for entry in "$OUTPUT_DIR_TESTS_FILES"/*
    do
      f="$(basename $entry)"
      echo $f
    done

echo "===================================================="
echo "FAILED TESTS"
echo "===================================================="
while IFS= read -r line
do
 test_status=$(echo $line | jq .event)
 case $test_status in
  "\"ok\"")
    test_name=$(echo $line | jq .name)
    echo -e "\e[32mPassed test : $test_name" ;;
  "\"failed\"")
    # check if a log file exist
    echo -e "\e[31mFailed test $test_name"
    test_name=$(echo $line | jq .name)

    for entry in "$OUTPUT_DIR_TESTS_FILES"/*
    do
      f="$(basename $entry)"
      if [[ $test_name =~ $f ]]; then
        echo "****************************************************"
        echo -e "\e[31m LOGS FOR TEST $test_name"
        echo "****************************************************"
        jq -c ' "\(.timestamp) ===> \(.fields.message)"' $entry
      fi
    done
  ;;
 esac
done < "$JUNIT_REPORT"