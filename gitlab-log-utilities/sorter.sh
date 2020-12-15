#!/bin/bash
set -o pipefail
if [ -p /dev/stdin ]; then
  while IFS= read line; do
    # json line ?
    if jq -e . >/dev/null 2>&1 <<<"$line"; then
      # this is a json file
      # is junit report or log
      if [ "$(echo "$line" | jq 'has("type")')" == "true" ]; then
        echo $line >> gitlab-log-utilities/output/junit-report.json

      elif [ "$(echo "$line" | jq 'has("spans")')" == "true" ]; then
        filename=$( echo $line | jq -r '.spans[].name' )
        echo "$line" >> "gitlab-log-utilities/output/$filename"
      fi
    fi
  done
fi
