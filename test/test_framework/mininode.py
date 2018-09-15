#!/usr/bin/env python3
"""Conflux P2P network half-a-node.

`P2PConnection: A low-level connection object to a node's P2P interface
P2PInterface: A high-level interface object for communicating to a node over P2P
"""

import asyncore
from collections import defaultdict
from io import BytesIO
import rlp
from rlp.sedes import binary, big_endian_int, CountableList
import logging
import socket
import struct
import sys
import threading

logger = logging.getLogger("TestFramework.mininode")

PACKET_HELLO = 0x80
PACKET_DISCONNECT = 0x01
PACKET_PING = 0x02
PACKET_PONG = 0x0
PACKET_PROTOCOL = 0x10


class Capability(rlp.Serializable):
    fields = [
        ("protocol", binary),
        ("version", big_endian_int)
    ]


class Hello(rlp.Serializable):
    fields = [
        ("capabilities", CountableList(Capability)),
        ("junk", big_endian_int)
    ]


class Disconnect(rlp.Serializable):
    fields = [
        ("reason", big_endian_int)
    ]


class P2PConnection(asyncore.dispatcher):
    """A low-level connection object to a node's P2P interface.

    This class is responsible for:

    - opening and closing the TCP connection to the node
    - reading bytes from and writing bytes to the socket
    - deserializing and serializing the P2P message header
    - logging messages as they are sent and received

    This class contains no logic for handling the P2P message payloads. It must be
    sub-classed and the on_message() callback overridden."""

    def __init__(self):
        assert not network_thread_running()

        super().__init__(map=mininode_socket_map)

    def peer_connect(self, dstaddr, dstport):
        self.dstaddr = dstaddr
        self.dstport = dstport
        self.create_socket(socket.AF_INET, socket.SOCK_STREAM)
        self.socket.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sendbuf = b""
        self.recvbuf = b""
        self.state = "connecting"
        self.disconnect = False
        self.had_hello = False

        logger.debug('Connecting to Conflux Node: %s:%d' %
                     (self.dstaddr, self.dstport))

        try:
            self.connect((dstaddr, dstport))
        except:
            self.handle_close()

    def peer_disconnect(self):
        # Connection could have already been closed by other end.
        if self.state == "connected":
            self.disconnect_node()

    # Connection and disconnection methods

    def handle_connect(self):
        """asyncore callback when a connection is opened."""
        if self.state != "connected":
            logger.debug("Connected & Listening: %s:%d" %
                         (self.dstaddr, self.dstport))
            self.state = "connected"
            self.on_open()

    def handle_close(self):
        """asyncore callback when a connection is closed."""
        logger.debug("Closing connection to: %s:%d" %
                     (self.dstaddr, self.dstport))
        self.state = "closed"
        self.recvbuf = b""
        self.sendbuf = b""
        try:
            self.close()
        except:
            pass
        self.on_close()

    def disconnect_node(self):
        """Disconnect the p2p connection.

        Called by the test logic thread. Causes the p2p connection
        to be disconnected on the next iteration of the asyncore loop."""
        self.disconnect = True

    # Socket read methods

    def handle_read(self):
        """asyncore callback when data is read from the socket."""
        buf = self.recv(8192)
        if len(buf) > 0:
            self.recvbuf += buf
            self._on_data()

    def _on_data(self):
        """Try to read P2P messages from the recv buffer.

        This method reads data from the buffer in a loop. It deserializes,
        parses and verifies the P2P header, then passes the P2P payload to
        the on_message callback for processing."""
        try:
            while True:
                if len(self.recvbuf) < 2:
                    return
                packet_size = struct.unpack("<h", self.recvbuf[:2])[0]
                if len(self.recvbuf) < 2 + packet_size:
                    return
                packet_id = self.recvbuf[2]
                if packet_id != PACKET_HELLO and packet_id != PACKET_DISCONNECT and (not self.had_hello):
                    raise ValueError("bad protocol")
                payload = self.recvbuf[3:2+packet_size]
                self.recvbuf = self.recvbuf[2+packet_size:]
                self._log_message("receive", packet_id)
                if packet_id == PACKET_HELLO:
                    hello = rlp.decode(payload, Hello)
                    self.on_hello(hello)
                elif packet_id == PACKET_DISCONNECT:
                    disconnect = rlp.decode(payload, Disconnect)
                    self.on_disconnect(disconnect)
                elif packet_id == PACKET_PING:
                    self.on_ping()
                elif packet_id == PACKET_PONG:
                    pass
                else:
                    assert packet_id == PACKET_PROTOCOL
                    self.on_protocol_packet(payload)
        except Exception as e:
            logger.exception('Error reading message: ' + repr(e))
            raise

    def on_hello(self, hello):
        self.send_packet(PACKET_HELLO, rlp.encode(hello, Hello))
        self.had_hello = True

    def on_disconnect(self, disconnect):
        self.on_close()

    def on_ping(self):
        self.send_packet(PACKET_PONG, b"")

    def on_protocol_packet(self, payload):
        """Callback for processing a protocol-specific P2P payload. Must be overridden by derived class."""
        raise NotImplementedError

    # Socket write methods

    def writable(self):
        """asyncore method to determine whether the handle_write() callback should be called on the next loop."""
        with mininode_lock:
            pre_connection = self.state == "connecting"
            length = len(self.sendbuf)
        return (length > 0 or pre_connection)

    def handle_write(self):
        """asyncore callback when data should be written to the socket."""
        with mininode_lock:
            # asyncore does not expose socket connection, only the first read/write
            # event, thus we must check connection manually here to know when we
            # actually connect
            if self.state == "connecting":
                self.handle_connect()
            if not self.writable():
                return

            try:
                sent = self.send(self.sendbuf)
            except:
                self.handle_close()
                return
            self.sendbuf = self.sendbuf[sent:]

    def send_packet(self, packet_id, payload, pushbuf=False):
        """Send a P2P message over the socket.

        This method takes a P2P payload, builds the P2P header and adds
        the message to the send buffer to be sent over the socket."""
        if self.state != "connected" and not pushbuf:
            raise IOError('Not connected, no pushbuf')
        self._log_message("send", packet_id)
        buf = struct.pack("<h", len(payload) + 3)
        buf += struct.pack("<B", packet_id)
        buf += payload
        with mininode_lock:
            if (len(self.sendbuf) == 0 and not pushbuf):
                try:
                    sent = self.send(buf)
                    self.sendbuf = buf[sent:]
                except BlockingIOError:
                    self.sendbuf = buf
            else:
                self.sendbuf += buf

    # Class utility methods

    def _log_message(self, direction, msg):
        """Logs a message being sent or received over the connection."""
        if direction == "send":
            log_message = "Send message to "
        elif direction == "receive":
            log_message = "Received message from "
        log_message += "%s:%d: %s" % (self.dstaddr,
                                      self.dstport, repr(msg)[:500])
        if len(log_message) > 500:
            log_message += "... (msg truncated)"
        logger.debug(log_message)


class P2PInterface(P2PConnection):
    """A high-level P2P interface class for communicating with a Bitcoin node.

    This class provides high-level callbacks for processing P2P message
    payloads, as well as convenience methods for interacting with the
    node over P2P.

    Individual testcases should subclass this and override the on_* methods
    if they want to alter message handling behaviour."""

    def __init__(self):
        super().__init__()

        # Track number of messages of each type received and the most recent
        # message of each type
        self.message_count = defaultdict(int)
        self.last_message = {}

    def peer_connect(self, *args, **kwargs):
        super().peer_connect(*args, **kwargs)

    # Message receiving methods

    def on_protocol_packet(self, payload):
        """Receive message and dispatch message to appropriate callback.

        We keep a count of how many of each message type has been received
        and the most recent message of each type."""
        with mininode_lock:
            try:
                protocol = payload[0:3]
            except:
                raise

    # Callback methods. Can be overridden by subclasses in individual test
    # cases to provide custom message handling behaviour.

    def on_open(self): pass

    def on_close(self): pass


# Keep our own socket map for asyncore, so that we can track disconnects
# ourselves (to work around an issue with closing an asyncore socket when
# using select)
mininode_socket_map = dict()

# One lock for synchronizing all data access between the networking thread (see
# NetworkThread below) and the thread running the test logic.  For simplicity,
# P2PConnection acquires this lock whenever delivering a message to a P2PInterface,
# and whenever adding anything to the send buffer (in send_message()).  This
# lock should be acquired in the thread running the test logic to synchronize
# access to any data shared with the P2PInterface or P2PConnection.
mininode_lock = threading.RLock()


class NetworkThread(threading.Thread):
    def __init__(self):
        super().__init__(name="NetworkThread")

    def run(self):
        while mininode_socket_map:
            # We check for whether to disconnect outside of the asyncore
            # loop to work around the behavior of asyncore when using
            # select
            disconnected = []
            for fd, obj in mininode_socket_map.items():
                if obj.disconnect:
                    disconnected.append(obj)
            [obj.handle_close() for obj in disconnected]
            asyncore.loop(0.1, use_poll=True, map=mininode_socket_map, count=1)
        logger.debug("Network thread closing")


def network_thread_running():
    """Return whether the network thread is running."""
    return any([thread.name == "NetworkThread" for thread in threading.enumerate()])


def network_thread_start():
    """Start the network thread."""
    assert not network_thread_running()

    NetworkThread().start()


def network_thread_join(timeout=10):
    """Wait timeout seconds for the network thread to terminate.

    Throw if network thread doesn't terminate in timeout seconds."""
    network_threads = [
        thread for thread in threading.enumerate() if thread.name == "NetworkThread"]
    assert len(network_threads) <= 1
    for thread in network_threads:
        thread.join(timeout)
        assert not thread.is_alive()
