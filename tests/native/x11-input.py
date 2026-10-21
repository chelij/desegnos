# SPDX-License-Identifier: GPL-3.0-only

import ctypes as c
import ctypes.util
import json
import math
import os
from pathlib import Path
import re
import sys
import time


class WindowAttributes(c.Structure):
    _fields_ = [
        ("x", c.c_int), ("y", c.c_int), ("width", c.c_int), ("height", c.c_int),
        ("border_width", c.c_int), ("depth", c.c_int), ("visual", c.c_void_p),
        ("root", c.c_ulong), ("window_class", c.c_int), ("bit_gravity", c.c_int),
        ("win_gravity", c.c_int), ("backing_store", c.c_int), ("backing_planes", c.c_ulong),
        ("backing_pixel", c.c_ulong), ("save_under", c.c_int), ("colormap", c.c_ulong),
        ("map_installed", c.c_int), ("map_state", c.c_int), ("all_event_masks", c.c_long),
        ("your_event_mask", c.c_long), ("do_not_propagate_mask", c.c_long),
        ("override_redirect", c.c_int), ("screen", c.c_void_p),
    ]


class ClientData(c.Union):
    _fields_ = [("bytes", c.c_char * 20), ("shorts", c.c_short * 10), ("longs", c.c_long * 5)]


class ClientMessage(c.Structure):
    _fields_ = [("type", c.c_int), ("serial", c.c_ulong), ("send_event", c.c_int),
        ("display", c.c_void_p), ("window", c.c_ulong), ("message_type", c.c_ulong),
        ("format", c.c_int), ("data", ClientData)]


class XEvent(c.Union):
    _fields_ = [("client", ClientMessage), ("padding", c.c_long * 24)]


def bind(library, name, result, arguments):
    fn = getattr(library, name)
    fn.restype = result
    fn.argtypes = arguments
    return fn


def main():
    display_name = os.environ.get("DISPLAY", "")
    match = re.fullmatch(r":(\d+)(?:\.\d+)?", display_name)
    if not match or int(match[1]) < 90 or display_name == os.environ.get("DESEGNI_DESKTOP_DISPLAY"):
        raise RuntimeError("X11 input requires a separate guarded Xvfb display numbered 90 or higher")
    if os.environ.get("GDK_BACKEND") != "x11" or os.environ.get("WAYLAND_DISPLAY") or os.environ.get("WAYLAND_SOCKET"):
        raise RuntimeError("X11 input is restricted to the isolated X11 test session")
    request = json.loads(sys.stdin.read(32769))
    pid = request.get("pid")
    if type(pid) is not int or pid < 1:
        raise RuntimeError("An owned native application PID is required")
    process_dir = Path(f"/proc/{pid}")
    if process_dir.stat().st_uid != os.getuid():
        raise RuntimeError("The test application does not belong to this user")
    environment = process_dir.joinpath("environ").read_bytes().split(b"\0")
    if f"DISPLAY={display_name}".encode() not in environment or b"GDK_BACKEND=x11" not in environment:
        raise RuntimeError("The native application is not on the isolated display")

    names = {name: ctypes.util.find_library(name) for name in ("X11", "Xtst")}
    if not all(names.values()):
        raise RuntimeError(f"Physical input needs system libX11 and libXtst: {names}")
    x = c.CDLL(names["X11"])
    xt = c.CDLL(names["Xtst"])
    pointer, window, integer, ulong = c.c_void_p, c.c_ulong, c.c_int, c.c_ulong
    open_display = bind(x, "XOpenDisplay", pointer, [c.c_char_p])
    close_display = bind(x, "XCloseDisplay", integer, [pointer])
    root_window = bind(x, "XDefaultRootWindow", window, [pointer])
    default_screen = bind(x, "XDefaultScreen", integer, [pointer])
    display_width = bind(x, "XDisplayWidth", integer, [pointer, integer])
    display_height = bind(x, "XDisplayHeight", integer, [pointer, integer])
    intern_atom = bind(x, "XInternAtom", ulong, [pointer, c.c_char_p, integer])
    get_property = bind(x, "XGetWindowProperty", integer, [pointer, window, ulong, c.c_long, c.c_long, integer, ulong,
        c.POINTER(ulong), c.POINTER(integer), c.POINTER(ulong), c.POINTER(ulong), c.POINTER(c.POINTER(c.c_ubyte))])
    get_attributes = bind(x, "XGetWindowAttributes", integer, [pointer, window, c.POINTER(WindowAttributes)])
    get_transient = bind(x, "XGetTransientForHint", integer, [pointer, window, c.POINTER(window)])
    query_tree = bind(x, "XQueryTree", integer, [pointer, window, c.POINTER(window), c.POINTER(window),
        c.POINTER(c.POINTER(window)), c.POINTER(c.c_uint)])
    translate = bind(x, "XTranslateCoordinates", integer, [pointer, window, window, integer, integer,
        c.POINTER(integer), c.POINTER(integer), c.POINTER(window)])
    free = bind(x, "XFree", integer, [pointer])
    sync = bind(x, "XSync", integer, [pointer, integer])
    raise_window = bind(x, "XRaiseWindow", integer, [pointer, window])
    set_focus = bind(x, "XSetInputFocus", integer, [pointer, window, integer, ulong])
    string_to_keysym = bind(x, "XStringToKeysym", ulong, [c.c_char_p])
    keysym_to_code = bind(x, "XKeysymToKeycode", c.c_ubyte, [pointer, ulong])
    code_to_keysym = bind(x, "XkbKeycodeToKeysym", ulong, [pointer, c.c_ubyte, integer, integer])
    query_extension = bind(xt, "XTestQueryExtension", integer, [pointer, c.POINTER(integer), c.POINTER(integer), c.POINTER(integer), c.POINTER(integer)])
    motion = bind(xt, "XTestFakeMotionEvent", integer, [pointer, integer, integer, integer, ulong])
    button = bind(xt, "XTestFakeButtonEvent", integer, [pointer, c.c_uint, integer, ulong])
    key = bind(xt, "XTestFakeKeyEvent", integer, [pointer, c.c_uint, integer, ulong])
    display = open_display(display_name.encode())
    if not display:
        raise RuntimeError("Cannot connect to the isolated Xvfb display")
    pressed = False
    pressed_button = 1
    modifiers_down = []
    try:
        extension = [integer() for _ in range(4)]
        if not query_extension(display, *[c.byref(value) for value in extension]):
            raise RuntimeError("The isolated Xvfb server does not support XTEST")
        root = root_window(display)
        screen = default_screen(display)
        atom = intern_atom(display, b"_NET_WM_PID", 1)
        if not atom:
            raise RuntimeError("The native application has not published its X11 PID")

        def pid_for(wid):
            actual, fmt, count, after = ulong(), integer(), ulong(), ulong()
            data = c.POINTER(c.c_ubyte)()
            get_property(display, wid, atom, 0, 1, 0, 0, c.byref(actual), c.byref(fmt), c.byref(count), c.byref(after), c.byref(data))
            try:
                return c.cast(data, c.POINTER(ulong))[0] if data and fmt.value == 32 and count.value == 1 else None
            finally:
                if data:
                    free(data)

        def children(wid):
            found_root, parent, values, count = window(), window(), c.POINTER(window)(), c.c_uint()
            if not query_tree(display, wid, c.byref(found_root), c.byref(parent), c.byref(values), c.byref(count)):
                return []
            try:
                return [values[index] for index in range(min(count.value, 512))]
            finally:
                if values:
                    free(values)

        def title_for(wid):
            name_atom = intern_atom(display, b"_NET_WM_NAME", 1) or intern_atom(display, b"WM_NAME", 1)
            actual, fmt, count, after = ulong(), integer(), ulong(), ulong()
            data = c.POINTER(c.c_ubyte)()
            get_property(display, wid, name_atom, 0, 1024, 0, 0, c.byref(actual), c.byref(fmt), c.byref(count), c.byref(after), c.byref(data))
            try:
                return c.string_at(data, count.value).decode("utf8", "replace") if data and fmt.value == 8 else ""
            finally:
                if data:
                    free(data)

        def metadata(wid):
            attributes = WindowAttributes()
            if not get_attributes(display, wid, c.byref(attributes)) or attributes.map_state != 2 or attributes.window_class != 1:
                return None
            px, py, child = integer(), integer(), window()
            if not translate(display, wid, root, 0, 0, c.byref(px), c.byref(py), c.byref(child)):
                return None
            transient = window()
            get_transient(display, wid, c.byref(transient))
            return {"id": wid, "pid": pid_for(wid), "title": title_for(wid), "mapped": True,
                "transientFor": transient.value or None, "x": px.value, "y": py.value,
                "width": attributes.width, "height": attributes.height}

        source_pid = request.get("sourcePid")
        if source_pid is not None:
            source_dir = Path(f"/proc/{source_pid}")
            if type(source_pid) is not int or source_pid < 1 or source_dir.stat().st_uid != os.getuid():
                raise RuntimeError("An owned drag source PID is required")
            source_env = source_dir.joinpath("environ").read_bytes().split(b"\0")
            if f"DISPLAY={display_name}".encode() not in source_env or b"GDK_BACKEND=x11" not in source_env:
                raise RuntimeError("The drag source must use the same isolated X11 display")
            if request.get("action") != "drag":
                raise RuntimeError("A second process is allowed only for file dragging")
        sources = []
        pending = [(wid, 0) for wid in children(root)]
        owned = []
        visited = 0
        while pending and visited < 1024:
            wid, depth = pending.pop()
            visited += 1
            item = metadata(wid)
            if item and item["pid"] == pid and item["width"] > 100 and item["height"] > 100:
                owned.append(item)
            if item and item["pid"] == source_pid and item["width"] > 100 and item["height"] > 100:
                sources.append(item)
            if depth < 6:
                pending.extend((child, depth + 1) for child in children(wid))
        if not owned:
            raise RuntimeError(f"No mapped X11 window belongs to native application {pid}")
        requested_window = request.get("windowId")
        if requested_window is not None and not any(item["id"] == requested_window for item in owned):
            raise RuntimeError("The requested window is not a mapped window owned by the test app")
        target = next(item for item in owned if item["id"] == requested_window) if requested_window is not None else max(owned, key=lambda item: item["width"] * item["height"])
        result = {"display": display_name, "pid": pid, "window": target, "windows": owned, "libraries": names,
            "screen": {"width": display_width(display, screen), "height": display_height(display, screen)},
            "xtest": {"major": extension[2].value, "minor": extension[3].value}}
        action = request.get("action", "inspect")
        if action == "inspect":
            print(json.dumps(result))
            return
        if action == "close":
            # Deliver the same owned-window protocol as its title-bar Close button,
            # without blurring an unfinished DOM edit or needing a window manager.
            event = XEvent()
            event.client.type = 33
            event.client.display = display
            event.client.window = target["id"]
            event.client.message_type = intern_atom(display, b"WM_PROTOCOLS", 0)
            event.client.format = 32
            event.client.data.longs[0] = intern_atom(display, b"WM_DELETE_WINDOW", 0)
            send_event = bind(x, "XSendEvent", integer, [pointer, window, integer, c.c_long, c.POINTER(XEvent)])
            if not send_event(display, target["id"], 0, 0, c.byref(event)):
                raise RuntimeError("The owned native window rejected its Close protocol")
            sync(display, 0)
            print(json.dumps(result))
            return
        if action not in ("move", "click", "drag", "wheel", "key", "text"):
            raise RuntimeError("Unsupported isolated native input action")

        def point(value, bounds=target):
            if not isinstance(value, dict) or not all(type(value.get(axis)) in (int, float) and math.isfinite(value[axis]) for axis in ("x", "y")):
                raise RuntimeError("A finite physical X11 point is required")
            px, py = round(value["x"]), round(value["y"])
            if not (bounds["x"] <= px < bounds["x"] + bounds["width"] and bounds["y"] <= py < bounds["y"] + bounds["height"]):
                raise RuntimeError(f"Refusing input outside the owned test window: {(px, py)}; {target}")
            if not (0 <= px < result["screen"]["width"] and 0 <= py < result["screen"]["height"]):
                raise RuntimeError("Refusing input outside the isolated screen")
            return px, py

        if source_pid is not None and len(sources) != 1:
            raise RuntimeError("File dragging requires one unambiguous mapped source window")
        source_window = sources[0] if source_pid is not None else target
        positions = [point(request["point"])] if action in ("move", "click", "wheel") else [point(request["from"], source_window), point(request["to"])] if action == "drag" else []
        if "path" in request:
            path = request["path"]
            if action != "drag" or source_pid is not None or not isinstance(path, list) or not 3 <= len(path) <= 500:
                raise RuntimeError("A drawn path needs 3 to 500 points in the owned test window")
            positions = [point(value) for value in path]
        raise_window(display, target["id"])
        set_focus(display, target["id"], 2, 0)
        if source_pid is not None:
            raise_window(display, source_window["id"])
            set_focus(display, source_window["id"], 2, 0)
        sync(display, 0)
        if positions:
            if not motion(display, screen, *positions[0], 0):
                raise RuntimeError("XTEST refused pointer motion")
            sync(display, 0)
            time.sleep(0.04)
        modifiers = request.get("modifiers", [])
        if not isinstance(modifiers, list) or len(set(modifiers)) != len(modifiers) or any(item not in ("Control", "Shift", "Alt") for item in modifiers):
            raise RuntimeError("Unsupported keyboard modifier")
        for modifier in modifiers:
            code = keysym_to_code(display, string_to_keysym((modifier + "_L").encode()))
            if not code or not key(display, code, 1, 0):
                raise RuntimeError("XTEST refused the keyboard modifier")
            modifiers_down.append(code)
        if action in ("click", "drag"):
            count = request.get("count", 1) if action == "click" else 1
            if type(count) is not int or not 1 <= count <= 2:
                raise RuntimeError("Only one click or a bounded double-click is allowed")
            pressed_button = request.get("button", 1)
            if pressed_button not in (1, 3) or action == "drag" and pressed_button != 1:
                raise RuntimeError("Only primary dragging or primary/secondary clicks are allowed")
            if type(request.get("cancelDrag", False)) is not bool:
                raise RuntimeError("Invalid drag cancellation flag")
            for index_click in range(count):
                if not button(display, pressed_button, 1, 0):
                    raise RuntimeError("XTEST refused the pointer press")
                pressed = True
                sync(display, 0)
                time.sleep(0.04)
                if action == "drag":
                    drag_steps = 2 if "path" in request else 24 if source_pid is not None else 8
                    for start, end in zip(positions, positions[1:]):
                        for index in range(1, drag_steps + 1):
                            px = round(start[0] + (end[0] - start[0]) * index / drag_steps)
                            py = round(start[1] + (end[1] - start[1]) * index / drag_steps)
                            if not motion(display, screen, px, py, 0):
                                raise RuntimeError("XTEST refused drag motion")
                            sync(display, 0)
                            time.sleep(0.025 if source_pid is not None else 0.012)
                    if request.get("cancelDrag", False):
                        code = keysym_to_code(display, string_to_keysym(b"Escape"))
                        key(display, code, 1, 0); key(display, code, 0, 0); sync(display, 0)
                        time.sleep(0.04)
                button(display, pressed_button, 0, 0)
                pressed = False
                sync(display, 0)
                if index_click + 1 < count:
                    time.sleep(0.06)
        if action == "wheel":
            count, direction = request.get("count", 1), request.get("direction")
            if type(count) is not int or not 1 <= count <= 20 or direction not in ("up", "down"):
                raise RuntimeError("Only bounded owned-window wheel input is allowed")
            for _ in range(count):
                number = 4 if direction == "up" else 5
                button(display, number, 1, 0)
                button(display, number, 0, 0)
                sync(display, 0)
                time.sleep(0.02)
        if action == "key":
            name, count = request.get("key"), request.get("count", 1)
            if name not in ("Left", "Right", "Up", "Down", "Home", "End", "Escape", "Return", "Tab", "L", "K", "A", "C", "X", "V", "R", "Z", "Y", "S", "N", "O", "bracketleft", "bracketright", "equal", "minus", "0", "1", "2", "Page_Down", "Page_Up", "F1", "F2", "F10", "space", "Delete", "BackSpace") or type(count) is not int or not 1 <= count <= 20:
                raise RuntimeError("Only bounded navigation and file-picker key sequences are allowed")
            code = keysym_to_code(display, string_to_keysym(name.encode()))
            if not code:
                raise RuntimeError(f"No X11 keycode for {name}")
            for _ in range(count):
                if not key(display, code, 1, 0) or not key(display, code, 0, 0):
                    raise RuntimeError("XTEST refused the keyboard sequence")
            for code in reversed(modifiers_down):
                key(display, code, 0, 0)
            modifiers_down.clear()
        if action == "text":
            text = request.get("text")
            if not isinstance(text, str) or not 1 <= len(text) <= 4096 or any(ord(char) < 32 or ord(char) > 126 for char in text):
                raise RuntimeError("Only bounded printable ASCII native text input is allowed")
            shift = keysym_to_code(display, string_to_keysym(b"Shift_L"))
            for char in text:
                symbol = ord(char)
                code = keysym_to_code(display, symbol)
                if not code:
                    raise RuntimeError(f"No X11 keycode for {char!r}")
                shifted = code_to_keysym(display, code, 0, 0) != symbol
                if shifted and code_to_keysym(display, code, 0, 1) != symbol:
                    raise RuntimeError(f"The isolated keyboard map cannot type {char!r}")
                if shifted:
                    key(display, shift, 1, 0)
                    modifiers_down.append(shift)
                if not key(display, code, 1, 0) or not key(display, code, 0, 0):
                    raise RuntimeError("XTEST refused native text input")
                if shifted:
                    key(display, shift, 0, 0)
                    modifiers_down.clear()
        sync(display, 0)
        result["action"] = action
        print(json.dumps(result))
    finally:
        if pressed:
            button(display, pressed_button, 0, 0)
            sync(display, 0)
        for code in reversed(modifiers_down):
            key(display, code, 0, 0)
        if modifiers_down:
            sync(display, 0)
        close_display(display)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
