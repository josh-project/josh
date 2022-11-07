#!/usr/bin/env python3

import fcntl
import pty
import os

ALL_FLAGS = {
    "O_ACCMODE":    os.O_ACCMODE,
    "O_CLOEXEC":    os.O_CLOEXEC,
    "O_DSYNC":      os.O_DSYNC,
    "O_NDELAY":     os.O_NDELAY,
    "O_NONBLOCK":   os.O_NONBLOCK,
    "O_WRONLY":     os.O_WRONLY,
    "O_APPEND":     os.O_APPEND,
    "O_CREAT":      os.O_CREAT,
    "O_EXCL":       os.O_EXCL,
    "O_NOCTTY":     os.O_NOCTTY,
    "O_RDONLY":     os.O_RDONLY,
    "O_SYNC":       os.O_SYNC,
    "O_ASYNC":      os.O_ASYNC,
    "O_DIRECTORY":  os.O_DIRECTORY,
    "O_NOFOLLOW":   os.O_NOFOLLOW,
    "O_RDWR":       os.O_RDWR,
    "O_TRUNC":      os.O_TRUNC,
};

def _print_flags():
    stdin_flags = fcntl.fcntl(pty.STDIN_FILENO, fcntl.F_GETFL)
    print(f"Flags: {stdin_flags:x}")
    for flag, mask in ALL_FLAGS.items():
        if (stdin_flags & mask) != 0:
            print(flag)


def _set_nonblock():
    stdin_flags = fcntl.fcntl(pty.STDIN_FILENO, fcntl.F_GETFL)
    fcntl.fcntl(pty.STDIN_FILENO, fcntl.F_SETFL, stdin_flags | os.O_NONBLOCK)


def _main():
    _print_flags()
    _set_nonblock()
    _print_flags()


if __name__ == '__main__':
    _main()
