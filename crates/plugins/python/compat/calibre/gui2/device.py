"""Mirrors `calibre.gui2.device` as plugins see it: `device_signals`, which
they connect to for device events. Magnitide manages devices itself, so
these signals never fire; connecting is harmless."""

from qt.core import QObject, pyqtSignal


class DeviceSignals(QObject):
    #: This signal is emitted once, after metadata is downloaded from the connected device.
    device_metadata_available = pyqtSignal()
    #: This signal is emitted once when the device is detected and once when it is disconnected.
    device_connection_changed = pyqtSignal(object)


device_signals = DeviceSignals()


class DeviceJob:
    pass


class DeviceManager(QObject):
    def __init__(self, *a, **k):
        QObject.__init__(self)
        self.connected_device = None
        self.connected_device_kind = None

    def is_device_connected(self):
        return False

    @property
    def device(self):
        return None


class DeviceMixin:
    """What calibre's main window mixes in; Magnitide's stand-in answers for the parts plugins ask."""

    def __init__(self, *a, **k):
        pass
