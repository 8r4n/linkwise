import os
output = os.fdopen(os.dup(1), "wb")
os.dup2(2, 1)
import struct, sys
import numpy as np
from scipy.signal import hilbert
from gnuradio import analog, blocks, channels, gr

snr, delay_ms, spread, echo_db, seed = map(float, sys.argv[1:])
seed = int(seed)
delay = delay_ms * 48
taps = int(delay) + 8
fading = channels.selective_fading_model(16, spread / 48000, True, float(os.environ.get("BENCH_RICIAN_K", "100")), seed,
                                         [0., delay], [1., 10**(echo_db/20)], taps)
noise = analog.noise_source_f(analog.GR_GAUSSIAN, 1, seed+100)
while header := sys.stdin.buffer.read(4):
    count, = struct.unpack('>I', header)
    data = sys.stdin.buffer.read(4*count)
    x = np.frombuffer(data, dtype='<f4')
    source = blocks.vector_source_c(hilbert(x))
    sink = blocks.vector_sink_c()
    noise_sink = blocks.vector_sink_f()
    flow = gr.top_block()
    flow.connect(source, fading, sink)
    flow.connect(noise, blocks.head(gr.sizeof_float, count), noise_sink)
    flow.run()
    y = np.asarray(sink.data()).real
    y = np.pad(y, (0,max(0,count-len(y))))[:count]
    rms = np.sqrt(np.mean(x*x))
    y = y + np.asarray(noise_sink.data())*rms*10**(-snr/20)
    output.write(header+np.clip(y,-1,1).astype('<f4').tobytes())
    output.flush()
