#!/usr/bin/env python3
from base64 import b64encode
from binascii import hexlify, unhexlify
from decimal import Decimal, ROUND_DOWN
import hashlib
import inspect
import json
import logging
import os
import random
import re
from subprocess import CalledProcessError
import time

import coverage
from authproxy import AuthServiceProxy, JSONRPCException

logger = logging.getLogger("TestFramework.utils")

# If a cookie file exists in the given datadir, delete it.
def delete_cookie_file(datadir):
    if os.path.isfile(os.path.join(datadir, "regtest", ".cookie")):
        logger.debug("Deleting leftover cookie file")
        os.remove(os.path.join(datadir, "regtest", ".cookie"))

def get_rpc_proxy(url, node_number, timeout=None, coveragedir=None):
    """
    Args:
        url (str): URL of the RPC server to call
        node_number (int): the node number (or id) that this calls to

    Kwargs:
        timeout (int): HTTP timeout in seconds

    Returns:
        AuthServiceProxy. convenience object for making RPC calls.

    """
    proxy_kwargs = {}
    if timeout is not None:
        proxy_kwargs['timeout'] = timeout

    proxy = AuthServiceProxy(url, **proxy_kwargs)
    proxy.url = url  # store URL on proxy for info

    coverage_logfile = coverage.get_filename(
        coveragedir, node_number) if coveragedir else None

    return coverage.AuthServiceProxyWrapper(proxy, coverage_logfile)

def rpc_url(datadir, i, rpchost=None, rpcport=None):
    if rpchost == None or rpcport == None:
        return "http://localhost:32325"
    else:
        return "http://" + rpchost + ":" + str(rpcport)

