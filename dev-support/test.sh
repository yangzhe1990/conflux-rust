#!/bin/bash

ROOT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )"/.. && pwd )"

function check_build {
    #rm -rf $ROOT_DIR/build && mkdir -p $ROOT_DIR/build
    pushd $ROOT_DIR > /dev/null
    CARGO_TARGET_DIR=$ROOT_DIR/build cargo build -v
    if [ $? -ne 0 ]; then
        echo "build failed" >> $ROOT_DIR/.phabricator-comment
        exit 1
    fi
    popd > /dev/null
}

function check_test {
    pushd $ROOT_DIR/core > /dev/null
    result=`CARGO_TARGET_DIR=$ROOT_DIR/build cargo test --tests`
    if [ $? -ne 0 ]; then
        echo "test failed" >> $ROOT_DIR/.phabricator-comment
        printf "%s\n" "${result}" >> $ROOT_DIR/.phabricator-comment
        exit 1
    fi
    popd > /dev/null
}

echo -n "" > $ROOT_DIR/.phabricator-comment
mkdir -p $ROOT_DIR/build

check_build
check_test

echo "Build succeed." > $ROOT_DIR/.phabricator-comment

