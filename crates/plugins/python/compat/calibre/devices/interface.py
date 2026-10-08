"""Mirrors the names of calibre's `calibre.devices.interface` that plugins
reference; no device is ever connected through them in Magnitide."""

from calibre.customize import Plugin


class DevicePlugin(Plugin):
    type = 'Device interface'
    FORMATS = []
    CAN_SET_METADATA = []

    def is_usb_connected(self, *a, **k):
        return False, None

    def detect_managed_devices(self, *a, **k):
        return None


class BookList(list):
    def __init__(self, oncard=None, prefix=None, settings=None):
        list.__init__(self)
        self.oncard = oncard
        self.prefix = prefix
        self.settings = settings

    def supports_collections(self):
        return False

    def add_book(self, book, replace_metadata):
        self.append(book)

    def remove_book(self, book):
        self.remove(book)

    def get_collections(self, collection_attributes):
        return {}
