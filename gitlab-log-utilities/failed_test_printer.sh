#!/bin/bash

OUTPUT_DIR_TESTS_FILES="/builds/qovery/qovery-engine/gitlab-log-utilities/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"

echo -e "\e[33m****************************************************"
echo -e "\e[33mLog Printer"
echo -e "\e[33m****************************************************"

cd $OUTPUT_DIR_TESTS_FILES

dirlist=(`ls`)

while IFS= read -r line
do
 test_status=$(echo $line | jq .event)
 case $test_status in
  "\"ok\"")
    test_name=$(echo $line | jq .name)
    echo -e "\e[32mPassed test : $test_name" ;;
  "\"failed\"")
    # check if a log file exist
    test_name=$(echo $line | jq .name)
    echo -e "\e[31mFailed test $test_name"
    for entry in ${dirlist[@]}
    do
      f="$(basename $entry)"
      if [[ $test_name =~ $f ]]; then
        printf "\n****************************************************\n"
        echo -e "\e[31m LOGS FOR FAILED TEST $test_name"
        printf "****************************************************\n"
        jq -c ' "\(.timestamp) : \(.target) ===> \(.fields.message)"' $entry
      fi
    done
  ;;
 esac
done < "$JUNIT_REPORT"