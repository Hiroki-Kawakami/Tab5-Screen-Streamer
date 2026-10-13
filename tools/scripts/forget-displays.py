#!/usr/bin/env python3
"""Make macOS forget the arrangements of every display not connected now,
virtual displays included, so the next one connects as new.

Stop streamer first, or its virtual display counts as connected and is kept.
Run as the logged-in user (it asks for sudo to write the system file), then
restart: WindowServer keeps the arrangements in memory and writes them back.
Both files are backed up to ~/display-prefs-backup-<time> first.
"""
import ctypes
import glob
import os
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time

SYSTEM = '/Library/Preferences/com.apple.windowserver.displays.plist'
USER_GLOB = '~/Library/Preferences/ByHost/com.apple.windowserver.displays.*.plist'


def connected_uuids():
    cg = ctypes.cdll.LoadLibrary('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
    cs = ctypes.cdll.LoadLibrary('/System/Library/Frameworks/ColorSync.framework/ColorSync')
    cf = ctypes.cdll.LoadLibrary('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
    cg.CGGetOnlineDisplayList.argtypes = [
        ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32), ctypes.POINTER(ctypes.c_uint32)]
    cs.CGDisplayCreateUUIDFromDisplayID.argtypes = [ctypes.c_uint32]
    cs.CGDisplayCreateUUIDFromDisplayID.restype = ctypes.c_void_p
    cf.CFUUIDCreateString.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    cf.CFUUIDCreateString.restype = ctypes.c_void_p
    cf.CFStringGetCString.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_long, ctypes.c_uint32]
    displays = (ctypes.c_uint32 * 32)()
    count = ctypes.c_uint32()
    if cg.CGGetOnlineDisplayList(32, displays, ctypes.byref(count)) != 0:
        sys.exit('cannot list the displays')
    uuids = set()
    for display in displays[:count.value]:
        uuid = cs.CGDisplayCreateUUIDFromDisplayID(display)
        text = ctypes.create_string_buffer(64)
        cf.CFStringGetCString(cf.CFUUIDCreateString(None, uuid), text, 64, 0x08000100)
        uuids.add(text.value.decode())
    if not uuids:
        sys.exit('no display found')
    return uuids


def keep_configs(sets, keep):
    before = len(sets['Configs'])
    sets['Configs'] = [
        c for c in sets['Configs']
        if {d.get('UUID') for d in c.get('DisplayConfig', [])} <= keep
    ]
    return before, len(sets['Configs'])


def forget_system(data, keep):
    print('system: %d -> %d arrangements' % keep_configs(data['DisplayAnyUserSets'], keep))
    mappings = data.get('DisplayUUIDMappings_v3', {})
    if 'UsedUUIDs' in mappings:
        mappings['UsedUUIDs'] = [u for u in mappings['UsedUUIDs'] if u in keep]
    entries = mappings.get('UUIDEntryMap', {})
    # identity -> [[rest of identity, UUID, count], ...]
    for identity in list(entries):
        kept = [e for e in entries[identity] if e[1] in keep]
        if kept:
            entries[identity] = kept
        else:
            del entries[identity]


def forget_user(data, keep):
    print('user: %d -> %d arrangements' % keep_configs(data['DisplaySets'], keep))


def main():
    if os.geteuid() == 0:
        sys.exit('run as the logged-in user, not with sudo')
    users = glob.glob(os.path.expanduser(USER_GLOB))
    if len(users) != 1:
        sys.exit('expected one ByHost display preferences file, found %d' % len(users))
    user = users[0]
    keep = connected_uuids()
    print('keeping %d connected displays' % len(keep))

    backup = time.strftime(os.path.expanduser('~/display-prefs-backup-%Y%m%d-%H%M%S'))
    os.makedirs(backup)
    for path in (SYSTEM, user):
        shutil.copy2(path, backup)
    print('backed up to', backup)

    with open(user, 'rb') as f:
        data = plistlib.load(f)
    forget_user(data, keep)
    with open(user, 'wb') as f:
        plistlib.dump(data, f, fmt=plistlib.FMT_BINARY)

    with open(SYSTEM, 'rb') as f:
        data = plistlib.load(f)
    forget_system(data, keep)
    with tempfile.NamedTemporaryFile(suffix='.plist', delete=False) as f:
        plistlib.dump(data, f, fmt=plistlib.FMT_BINARY)
    subprocess.run(['sudo', 'install', '-m', '644', '-o', 'root', '-g', 'admin', f.name, SYSTEM],
                   check=True)
    os.unlink(f.name)
    print('done; restart to apply')


main()
