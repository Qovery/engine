#!/usr/bin/env bash

# Just a test
#set -x

awk=awk
sed=sed
grep=grep
if [ "$(uname)" == "Darwin" ] ; then
  grep='ggrep'
  awk='gawk'
  sed='gsed'
fi
all_labels="test-aws-all"

function variable_not_found() {
  echo "Required variable not found: $1"
  exit 1
}

function release() {
  test -z $GITLAB_PROJECT_ID && variable_not_found "GITLAB_PROJECT_ID"
  test -z $GITLAB_TOKEN && variable_not_found "GITLAB_TOKEN"
  test -z $GITLAB_PERSONAL_TOKEN && variable_not_found "GITLAB_PERSONAL_TOKEN"
  test -z $GITHUB_BRANCH && variable_not_found "GITHUB_BRANCH"
  GITLAB_REF="main"

  echo "Requesting Gitlab pipeline"
  pipeline_id=$(curl -s -X POST -F "token=$GITLAB_TOKEN" -F "ref=$GITLAB_REF" -F "variables[GITHUB_COMMIT_ID]=$GITHUB_COMMIT_ID" -F "variables[GITHUB_ENGINE_BRANCH_NAME]=$GITHUB_BRANCH" -F "variables[TESTS_TYPE]=$TESTS_TYPE" https://gitlab.com/api/v4/projects/$GITLAB_PROJECT_ID/trigger/pipeline | jq --raw-output '.id')
  if [ $(echo $pipeline_id | egrep -c '^[0-9]+$') -eq 0 ] ; then
    echo "Pipeline ID is not correct, we expected a number and got: $pipeline_id"
    exit 1
  fi
  echo "Pipeline ID: $pipeline_id"
}

function gh_tags_selector_for_gitlab() {
  gh_json=$(curl -s -H "Accept: application/vnd.github.v3+json" "https://api.github.com/repos/Qovery/engine/pulls?state=open")
  gh_pr=$(echo $gh_json | jq --compact-output '.[] | {labels, ref: .head.ref}' | grep "$GITHUB_BRANCH")
  num_labels=$(echo $gh_pr | jq '.labels | length')

  if [ "$num_labels" != "0" ] ; then
    all_labels=""
    for i in $(echo $gh_pr | jq -r '.labels[].name' | grep 'test-') ; do
      all_labels="$all_labels,$i"
    done
    all_labels=$(echo $all_labels | sed 's/^,//')
  fi

  echo $all_labels
}

function run_tests() {
  TESTS_TYPE=$1
  echo "Requested tests: $TESTS_TYPE"
  test -z $GITLAB_PROJECT_ID && variable_not_found "GITLAB_PROJECT_ID"
  test -z $GITLAB_TOKEN && variable_not_found "GITLAB_TOKEN"
  test -z $GITLAB_PERSONAL_TOKEN && variable_not_found "GITLAB_PERSONAL_TOKEN"
  test -z $GITHUB_BRANCH && variable_not_found "GITHUB_BRANCH"
  GITLAB_REF="dev"
  FORCE_CHECKOUT_CUSTOM_BRANCH='false'
  TESTS_TO_RUN="-F \"variables[GITHUB_COMMIT_ID]=$GITHUB_COMMIT_ID\""

  if [ $(curl -s --header "PRIVATE-TOKEN: $GITLAB_PERSONAL_TOKEN" "https://gitlab.com/api/v4/projects/$GITLAB_PROJECT_ID/repository/branches/$GITHUB_BRANCH" | grep -c '404 Branch Not Found') -eq 0 ] ; then
    echo "Same branch name detected on gitlab, requesting to use it instead of dev branch"
    FORCE_CHECKOUT_CUSTOM_BRANCH='true'
  fi

  echo "Requesting Gitlab pipeline"
  pipeline_id=$(curl -s -X POST -F "token=$GITLAB_TOKEN" -F "ref=$GITLAB_REF" -F "variables[GITHUB_COMMIT_ID]=$GITHUB_COMMIT_ID" -F "variables[GITHUB_ENGINE_BRANCH_NAME]=$GITHUB_BRANCH" -F "variables[TESTS_TO_RUN]=$TESTS_TYPE" -F "variables[FORCE_CHECKOUT_CUSTOM_BRANCH]=$FORCE_CHECKOUT_CUSTOM_BRANCH" https://gitlab.com/api/v4/projects/$GITLAB_PROJECT_ID/trigger/pipeline | jq --raw-output '.id')
  if [ $(echo $pipeline_id | egrep -c '^[0-9]+$') -eq 0 ] ; then
    echo "Pipeline ID is not correct, we expected a number and got: $pipeline_id"
    exit 1
  fi
  sleep 2

  pipeline_status=''
  counter=0
  max_unexpected_status=5
  while [ $counter -le $max_unexpected_status ] ; do
    current_status=$(curl -s -H "PRIVATE-TOKEN: $GITLAB_PERSONAL_TOKEN" https://gitlab.com/api/v4/projects/$GITLAB_PROJECT_ID/pipelines/$pipeline_id | jq --raw-output '.detailed_status.text')
    echo "Current pipeline id $pipeline_id status: $current_status"
    case $current_status in
      "created")
        ((counter=$counter+1))
      ;;
      "waiting_for_resource")
        ((counter=$counter+1))
      ;;
      "preparing")
        ((counter=$counter+1))
      ;;
      "pending")
        ((counter=$counter+1))
      ;;
      "running")
        counter=0
      ;;
      "passed")
        echo "Results: Congrats, functional tests succeeded!!!"
        exit 0
      ;;
      "success")
        echo "Results: Congrats, functional tests succeeded!!!"
        exit 0
      ;;
      "failed")
        echo "Results: Functional $TESTS_TYPE tests failed"
        exit 1
      ;;
      "canceled")
        exit 1
      ;;
      "skipped")
        exit 1
      ;;
      "manual")
        exit 1
      ;;
      "scheduled")
        ((counter=$counter+1))
      ;;
      "null")
        ((counter=$counter+1))
      ;;
    esac

    sleep 10
  done

  echo "Results: functional tests failed due to a too high number ($max_unexpected_status) of unexpected status."
  exit 1
}

#set -u

case $1 in
full_tests)
  run_tests full
  ;;
release)
  release
  ;;
autodetect)
  tags=$(gh_tags_selector_for_gitlab)
  run_tests $tags
  ;;
check_gh_tags)
  if [ "$(gh_tags_selector_for_gitlab)" == "$all_labels" ] ; then
    echo "All tests have been enabled"
    exit 0
  fi
  echo "You need to enable all the tests to validate this PR"
  exit 1
  ;;
*)
  echo "Usage:"
  echo "$0 autodetect: autodetect tests to run based on tags"
  echo "$0 full_tests: run full tests (with cloud providers check)"
  echo "$0 check_gh_tags: get defined tags (only working if branch is a PR)"
  ;;
esac
