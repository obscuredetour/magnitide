"""Mirrors the part of calibre's `calibre.devices.usbms.driver` plugins
use: `debug_print` (kiwidude's plugins log through it) and a `USBMS` base
class for `isinstance` checks. No device is driven through it."""

import time

from calibre.constants import DEBUG
from calibre.devices.interface import BookList, DevicePlugin

BASE_TIME = None


def debug_print(*args, **kw):
    global BASE_TIME
    if BASE_TIME is None:
        BASE_TIME = time.time()
    if DEBUG:
        from calibre import prints

        prints('DEBUG: %6.1f' % (time.time() - BASE_TIME), *args, **kw)


class USBMS(DevicePlugin):
    name = 'USBMS Base Device Interface'
    booklist_class = BookList

    def books(self, oncard=None, end_session=True):
        return BookList(oncard=oncard)


__all__ = ['debug_print', 'USBMS', 'BookList']
