# Conflux-Rust

Conflux-rust is a rust-based implementation of Conflux protocol, it is fast and reliable.

Build Instructions:

1. Install rust
2. rustup update
3. Make sure you are using the stable branch

Test Instruction:

1. Install solc to compile solidity
2. Run dev-support/dep_pip3.sh to install extra python3 packages for running test
3. Run the python scripts in test directory.

Note that there is another sha3 package which does not contain necessary function. Do not install that package! Install pysha3 instead.
