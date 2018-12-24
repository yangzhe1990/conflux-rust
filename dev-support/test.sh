#!/bin/bash

ROOT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )"/.. && pwd )"

function check_build {
    local -n test_reuslt=$1

    #rm -rf $ROOT_DIR/build && mkdir -p $ROOT_DIR/build
    pushd $ROOT_DIR > /dev/null

    local result
    result=`CARGO_TARGET_DIR=$ROOT_DIR/build RUSTFLAGS="-D warnings" cargo build -v`
    local exit_code=$?

    popd > /dev/null

    if [[ $exit_code -ne 0 ]]; then
        result="Build failed."$'\n'"$result"
    else
        result="Build succeeded."
    fi
    test_result=($exit_code "$result")
}

function check_client_tests {
    local -n test_reuslt=$1

    pushd $ROOT_DIR/client > /dev/null
    local result
    result=`CARGO_TARGET_DIR=$ROOT_DIR/build cargo test`
    local exit_code=$?
    popd > /dev/null

    if [[ $exit_code -ne 0 ]]; then
        result="Unit test in client failed."$'\n'"$result"
    fi
    test_result=($exit_code "$result")
}

function check_core_tests {
    local -n test_reuslt=$1

    pushd $ROOT_DIR/core > /dev/null
    local result
    result=`CARGO_TARGET_DIR=$ROOT_DIR/build cargo test`
    local exit_code=$?
    popd > /dev/null

    if [[ $exit_code -ne 0 ]]; then
        result="Unit test in core failed."$'\n'"$result"
    fi
    test_result=($exit_code "$result")
}

function check_integration_tests {
    local -n test_reuslt=$1

    pushd $ROOT_DIR > /dev/null
    local result
    result=$(
        # Make symbolic link for conflux binary to where integration test assumes its existence.
        rm -f target; ln -s build/x86_64-unknown-linux-gnu/ target
        ./test/test_all.py
    )
    local exit_code=$?
    popd > /dev/null

    if [[ $exit_code -ne 0 ]]; then
        result="Integration test failed."$'\n'"$result"
    fi
    test_result=($exit_code "$result")
}

function save_test_result {
    local -n test_reuslt=$1
    local exit_code=${test_result[0]}
    local result=${test_result[1]}
    if [[ $exit_code -ne 0 ]]; then
        printf "%s\n" "$result" >> $ROOT_DIR/.phabricator-comment
        exit 1
    fi
    printf "%s\n" "$result"
}

echo -n "" > $ROOT_DIR/.phabricator-comment
mkdir -p $ROOT_DIR/build

declare -a test_result; check_build test_result; save_test_result test_result
declare -a test_result; check_core_tests test_result; save_test_result test_result
declare -a test_result; check_client_tests test_result; save_test_result test_result
declare -a test_result; check_integration_tests test_result; save_test_result test_result

