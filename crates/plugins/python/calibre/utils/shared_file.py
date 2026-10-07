"""Mirrors calibre's `calibre.utils.shared_file`: opening files shared on
Windows. Python's `open` already shares on every platform Magnitide runs
on."""

import os


def share_open(path, mode='r', buffering=-1, encoding=None, errors=None, newline=None, closefd=True):
    return open(path, mode, buffering, encoding, errors, newline, closefd)


def raise_winerror(pywinerr):
    raise OSError(getattr(pywinerr, 'winerror', 0), str(pywinerr))


def is_eintr_error(e):
    return getattr(e, 'errno', None) in (4,)


__all__ = ['share_open', 'raise_winerror', 'is_eintr_error', 'os']
