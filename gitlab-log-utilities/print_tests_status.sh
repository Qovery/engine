#!/bin/bash

FORMATED_FAILED_TESTS_DIR="/builds/qovery/qovery-engine/gitlab-log-utilities"
OUTPUT_DIR_TESTS_FILES="$FORMATED_FAILED_TESTS_DIR/output"
JUNIT_REPORT="$OUTPUT_DIR_TESTS_FILES/junit-report.json"
TESTS_NON_HANDLED_ISSUES="$OUTPUT_DIR_TESTS_FILES/tests_issues"

set -e

function generate_html_file() {
  file=$1
  html_file=${file}.html
  html_file_wip=${html_file}.wip


  jq -Mc ' "\(.timestamp) | \(.level) | \(.target) | \(.fields.message)"' $file >> $html_file_wip
  # replace line return by html ones
  sed -ri 's/\\n/<br \/>/g' $html_file_wip
  # remove " at the begin and end of lines
  sed -ri 's/^"//g' $html_file_wip
  sed -ri 's/"$//g' $html_file_wip
  sed -ri 's/\\"/"/g' $html_file_wip
  # convert ANSI codes color to html
  sed -ri 's/ /\&nbsp\;/g' $html_file_wip
  sed -ri 's/\\u001b\[31m(.*?)\\u001b\[0m/<font style="color:red">\1<\/font>/g' $html_file_wip
  sed -ri 's/\\u001b\[32m(.*?)\\u001b\[0m/<font style="color:green">\1<\/font>/g' $html_file_wip
  sed -ri 's/\\u001b\[1m(.*?)\\u001b\[0m/<b>\1<\/b>/g' $html_file_wip
  # removing non properly handled colors
  sed -ri 's/\\u001b\[31m//g' $html_file_wip
  sed -ri 's/\\u001b\[32m//g' $html_file_wip
  sed -ri 's/\\u001b\[1m//g' $html_file_wip
  sed -ri 's/\\u001b\[0m/\&nbsp\;\&nbsp\;\&nbsp\;/g' $html_file_wip
  # colorize info, warn, and error
  sed -ri 's/\|\&nbsp\;INFO\&nbsp\;\|/| <font style="color:green">INFO<\/font> |/g' $html_file_wip
  sed -ri 's/\|\&nbsp\;WARN\&nbsp\;\|/| <font style="color:orange">WARN<\/font> |/g' $html_file_wip
  sed -ri 's/fail/<font style="color:red">fail<\/font>/g' $html_file_wip
  sed -ri 's/^(.+)\|\&nbsp\;ERROR\&nbsp\;\|(.+)$/<font style="color:red">\1|ERRO| \2<\/font>/g' $html_file_wip
  # line return at the end of lines
  sed -ri 's/$/<br \/>/g' $html_file_wip

  cat << EOF > $html_file
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>$file</title>
  <style>
    body {
        background-color: #262b33;
        color: white;
    }
  </style>
</head>
<body>
<h1>$file</h1>
EOF

  cat $html_file_wip >> $html_file

  cat << EOF >> $html_file
</body>
</html>
EOF

  rm -f $html_file_wip
}

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

        # generate html version to make it more readable
        generate_html_file $test_file
        mv $OUTPUT_DIR_TESTS_FILES/*.html $FORMATED_FAILED_TESTS_DIR

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