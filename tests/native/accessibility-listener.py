"""Exercise WebKit accessibility on the private native-test bus, never the desktop bus."""
import os
import sys
import gi

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi, GLib

assert int(os.environ['DISPLAY'].split(':')[1].split('.')[0]) >= 90
assert os.environ['DISPLAY'] != os.environ.get('DESEGNOS_DESKTOP_DISPLAY')
target = int(sys.argv[1])
listener = Atspi.EventListener.new(lambda event: None)
listener.register('object:text-selection-changed')
listener.register('object:text-changed')

def activate():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app.get_process_id() != target:
            continue
        pending = [app]
        count = 0
        while pending and count < 10000:
            item = pending.pop()
            count += 1
            try:
                pending.extend(item.get_child_at_index(i) for i in range(item.get_child_count()))
            except GLib.Error:
                pass
        print('Accessibility active', flush=True)
        return False
    return True

GLib.timeout_add(200, activate)
GLib.MainLoop().run()
