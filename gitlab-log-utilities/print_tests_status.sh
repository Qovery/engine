#!/bin/bash

FORMATED_FAILED_TESTS_DIR="/builds/qovery/qovery-engine/gitlab-log-utilities"
OUTPUT_DIR_TESTS_FILES="$FORMATED_FAILED_TESTS_DIR/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"
TESTS_NON_HANDLED_ISSUES="$OUTPUT_DIR_TESTS_FILES/tests_issues"

cd $OUTPUT_DIR_TESTS_FILES

# Pretty print failed tests
if [ $(grep -c '"event": "failed"' $JUNIT_REPORT) -gt 0 ] ; then
  echo -e "\e[33m****************************************************"
  echo "                FAILED TESTS RESULTS"
  echo "****************************************************"
  echo -e "\033[0m"

  while IFS= read -r line ; do
    if [ $(echo $line | grep -c 'failed') -ne 0 ] ; then
      test_name=$(echo $line | jq .name)
      test_file="$(echo $test_name | sed -r 's/.+::(.+)"$/\1/')"
      if [ -f $test_file ] ; then
        echo -e "\n\n\e[36m###"
        echo "### LOGS FOR FAILED TEST: $test_name"
        echo -e "###\n\n\e[0m"

        while read -r line ; do
          if [ $(echo $line | grep -c ERROR) -ne 0 ] ; then
            echo -e "\e[31m$line\e[0m"
            continue
          fi
          echo -e "$line"
        done < <(jq -Mc ' "\(.timestamp) | \(.level) | \(.target) | \(.fields.message)"' $test_file)
        jq -Mc ' "\(.timestamp) | \(.level) | \(.target) | \(.fields.message)"' $test_file > cleaned_${test_file}

      else
        if [ "$test_file" != "null" ] ; then
          echo "File not found: $test_file" >> $TESTS_NON_HANDLED_ISSUES
        fi
      fi
    fi
  done < "$JUNIT_REPORT"

fi

# Show OK tests
if [ $(grep -c '"event": "ok"' $JUNIT_REPORT) -gt 0 ] ; then
  echo -e "\n\n\e[32m****************************************************"
  echo "                      TESTS OK"
  echo -e "****************************************************\n"
  grep '"event": "ok"' $JUNIT_REPORT | grep -v '"type": "suite"' | jq --raw-output '.name' | sort
  echo -en "\e[0m"
fi

# Show Failed tests
if [ $(grep -c '"event": "failed"' $JUNIT_REPORT) -gt 0 ] ; then
  echo -e "\n\n\e[31m****************************************************"
  echo "                    FAILED TESTS"
  echo -e "****************************************************\n"
  grep '"event": "failed"' $JUNIT_REPORT | grep -v '"type": "suite"' | jq --raw-output '.name' | sort
  echo -en "\e[0m"
  echo -e "\nNOTE: See logs above to get failed tests logs output"
fi

# Tests issues
if [ "$(wc -l $TESTS_NON_HANDLED_ISSUES 2>/dev/null | awk '{ print $1 }')" != "0" ] ; then
  echo -e "\n\n\e[31m****************************************************"
  echo "                OTHER NON HANDLED ISSUES"
  echo -e "****************************************************\n"
  cat $TESTS_NON_HANDLED_ISSUES | sort
  echo -en "\e[0m"
  echo -e "\nNOTE: See logs above to get failed tests logs output"
fi