#pragma once

// UAC speaker audio pipeline for the Tab5 streamer.
//
// PCM arrives from the PC over USB (UAC 2.0) clocked by the host, but it is
// played out through the ES8388 codec clocked by the Tab5's own I2S master.
// Those two clocks are never exactly equal, so a fixed-rate copy slowly
// over- or under-flows the buffer and produces periodic clicks/dropouts.
//
// audio_manager interposes a Catmull-Rom (cubic Hermite) resampler between
// the USB sink and the codec. The resampling step is nudged around 1.0 by a
// proportional controller driven by buffer occupancy, so the playout rate
// continuously tracks the host rate without inserting or dropping samples.

// Configure codec routing, register the streamer_usb audio callbacks, and
// start the resampling consumer task. Call once after streamer_usb_init().
void audio_manager_init();
