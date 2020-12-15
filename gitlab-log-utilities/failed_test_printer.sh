#!/bin/bash
input="gitlab-log-utilities/output/junit-report.json"
directory="gitlab-log-utilities/output/"

mkdir -p $directory

while IFS= read -r line
do
 test_status=$(echo $line | jq .event)
 case $test_status in
  "\"ok\"")
    test_name=$(echo $line | jq .name)
    echo "Passed test $test_name" ;;
  "\"failed\"")
    # check if a log file exist
    echo "Failed test $test_name"
    test_name=$(echo $line | jq .name)
    for entry in "$directory"*
    do
      f="$(basename $entry)"
      if [[ $test_name =~ $f ]]; then
        cat $entry
      fi
    done
  ;;
 esac
done < "$input"