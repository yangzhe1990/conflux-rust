# Conflux-Rust

Conflux-rust is a rust-based implementation of Conflux protocol, it is fast and reliable.

Build Instructions:

1. Install rust
2. rustup update
3. Make sure you are using the stable branch

Extra python3 packages for running test:

pip3 install eth-utils
pip3 install rlp
pip3 install py_ecc
pip3 install coincurve
pip3 install pysha3
pip3 install coincurve
pip3 install trie

4. Install above packages before running the python scripts in test directory.
   Note that there is another sha3 package which does not contain necessary
   function. Do not install that package! Install pysha3 instead.
