#!/bin/bash

set -e

function install() {
	if [ "`pip3 show $1`" =  "" ]; then
		pip3 install $1
	fi
}

install eth-utils
install rlp
install py_ecc
install coincurve
install pysha3
install trie
install web3
install easysolc
