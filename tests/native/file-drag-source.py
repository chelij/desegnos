# SPDX-License-Identifier: GPL-3.0-only
"""A real GTK file drag source. XTEST drives it; the editor receives native Xdnd."""
import json
import os
from pathlib import Path
import re
import sys
import gi

gi.require_version('Gtk', '3.0')
from gi.repository import Gdk, GLib, Gtk

if not re.fullmatch(r':(?:9[0-9]|[1-9][0-9]{2,})(?:\.\d+)?', os.environ.get('DISPLAY', '')) or os.environ.get('GDK_BACKEND') != 'x11':
    raise RuntimeError('File drag sources require the guarded private X11 display')
file = Path(sys.argv[1]).resolve(strict=True)
evidence = Path(os.environ['DESEGNI_NATIVE_TEST_EVIDENCE']).resolve(strict=True)
if not file.is_relative_to(evidence):
    raise RuntimeError('Only files in this run’s evidence folder can be dragged')
window = Gtk.Window(title='Native test picture file')
window.set_default_size(170, 120)
window.move(1410, 80)
button = Gtk.Button(label=file.name)
button.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [Gtk.TargetEntry.new('text/uri-list', 0, 0)], Gdk.DragAction.COPY)
button.connect('drag-data-get', lambda widget, context, selection, info, time: selection.set_uris([file.as_uri()]))
button.connect('drag-end', lambda *args: print(json.dumps({'dragEnded': True}), flush=True))
window.add(button)
window.connect('destroy', Gtk.main_quit)
window.show_all()
GLib.timeout_add(300, lambda: (print(json.dumps({'ready': True, 'pid': os.getpid()}), flush=True), False)[1])
Gtk.main()
