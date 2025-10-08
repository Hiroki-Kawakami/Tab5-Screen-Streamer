import usb.core, usb.util, time, struct

VID, PID = 0x303a, 0x4020
dev = usb.core.find(idVendor=VID, idProduct=PID)
assert dev is not None

if dev.is_kernel_driver_active(0):
    dev.detach_kernel_driver(0)

dev.set_configuration()
cfg = dev.get_active_configuration()
intf = cfg[(0,0)]
ep_out = usb.util.find_descriptor(intf, custom_match=lambda e: usb.util.endpoint_direction(e.bEndpointAddress)==usb.util.ENDPOINT_OUT)

with open("image.jpg", "rb") as f:
    data = bytearray(f.read())
data = struct.pack("<I", len(data) + 4) + data
ep_out.write(data, timeout=1000)
