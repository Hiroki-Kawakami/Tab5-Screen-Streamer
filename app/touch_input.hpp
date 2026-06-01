#pragma once

// Single owner of the capacitive touch controller. A dedicated task polls all
// contacts and routes them two ways:
//   - while the settings overlay is hidden  -> forwarded to the PC over USB
//     (vendor IN) as touch reports (remote-control mode);
//   - while the overlay is visible           -> the primary contact feeds the
//     LVGL pointer indev (settings UI).
// A 3+-finger touch toggles the overlay. See PROTOCOL.md for the wire format.
namespace touch_input {

// Start the touch task. Call once, after pf_port::init() and lvgl setup.
void start();

// Latest primary contact in native panel coordinates, for the LVGL indev.
// Returns true if a finger is currently down (and writes x/y), false otherwise.
bool primary(int *x, int *y);

}  // namespace touch_input
