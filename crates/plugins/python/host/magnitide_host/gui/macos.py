"""macOS: the host as an accessory app (no Dock icon), plugin windows in
front of Magnitide, and the focus back to Magnitide afterwards."""

import os
import sys
import traceback


def objc(receiver, selector, arg=None):
    """Send an Objective-C message (receiver: a class name or an object; one
    integer argument at most) through the runtime."""
    import ctypes
    import ctypes.util

    lib = ctypes.cdll.LoadLibrary(ctypes.util.find_library('objc'))
    lib.objc_getClass.restype = ctypes.c_void_p
    lib.sel_registerName.restype = ctypes.c_void_p
    if isinstance(receiver, bytes):
        receiver = lib.objc_getClass(receiver)
    args = () if arg is None else (arg,)
    # objc_msgSend must be called through a prototype matching the method.
    send = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, *(ctypes.c_long for _ in args))(
        ('objc_msgSend', lib)
    )
    return send(receiver, lib.sel_registerName(selector), *args)


def ns_app():
    return objc(b'NSApplication', b'sharedApplication')


def macos_accessory():
    """No Dock icon or menu bar for the host: its windows are plugins'
    dialogs, which belong with Magnitide (NSApplicationActivationPolicyAccessory)."""
    if sys.platform == 'darwin':
        try:
            objc(ns_app(), b'setActivationPolicy:', 1)
        except Exception:
            traceback.print_exc()


def bring_to_front(window):
    """A plugin opened a window: show it in front of Magnitide, whose menu
    item the user just chose."""
    if sys.platform == 'darwin':
        try:
            objc(ns_app(), b'activateIgnoringOtherApps:', 1)
        except Exception:
            traceback.print_exc()
    window.raise_()
    window.activateWindow()


def return_focus():
    """The plugin is done: if its windows had the focus, give it back to
    Magnitide (which started this host and says who it is)."""
    if sys.platform != 'darwin':
        return
    try:
        if not (objc(ns_app(), b'isActive') or 0) & 0xFF:
            return
        pid = int(os.environ.get('MAGNITIDE_PID') or os.getppid())
        parent = objc(b'NSRunningApplication', b'runningApplicationWithProcessIdentifier:', pid)
        if parent:
            objc(parent, b'activateWithOptions:', 2)  # NSApplicationActivateIgnoringOtherApps
    except Exception:
        traceback.print_exc()
