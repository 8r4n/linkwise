#!/usr/bin/env python3
"""Run the simulated HF network and stream link activity to a local Cesium globe."""

import argparse
import base64
import itertools
import json
import math
import threading
import time
import traceback
from collections import deque
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from antennas import ANTENNA_PRESETS, CATALOG_PATH, load_configuration
from medium import INTERFERENCE_PROFILES
from simulator import PAYLOAD, TICK_HZ, Simulation
from stations import BANDS, MAX_STATIONS, make_stations


ROOT = Path(__file__).resolve().parent
WEB_ROOT = ROOT / "web"
CESIUM_ROOT = ROOT.parent.parent / "globe" / "vendor"
SATELLITE_IMAGE = CESIUM_ROOT.parent / "imagery" / "blue-marble-clear.jpg"
SIGPLOT_ROOT = ROOT.parent.parent / "rust" / "demo-ui" / "public" / "vendor"
SIGPLOT_FILES = {
    "sigplot.js": "sigplot-3.1.7.js",
    "sigplot-3.1.7.js.LEGAL.txt": "sigplot-3.1.7.js.LEGAL.txt",
    "LICENSE.sigplot": "LICENSE.sigplot",
    "NOTICE.sigplot": "NOTICE.sigplot",
}


def event_bytes(kind, data):
    return f"event: {kind}\ndata: {json.dumps(data, separators=(',', ':'))}\n\n".encode()


class EventStream:
    def __init__(self, snapshot):
        self.changed = threading.Condition()
        self.latest = snapshot
        self.captures = {}
        self.recent_captures = {}
        self.events = deque(maxlen=512)
        self.revision = 0

    def publish(self, snapshot, events, captures, previous_captures):
        with self.changed:
            self.latest = snapshot
            self.captures = captures.copy()
            self.recent_captures = {
                (pair.attempt["source"], pair.attempt["id"]): pair for pair in previous_captures
            }
            for kind, event in events:
                self.revision += 1
                self.events.append((self.revision, event_bytes(kind, event)))
            if events:
                self.changed.notify_all()

    def capture(self, station, attempt_id=None):
        with self.changed:
            latest = self.captures.get(station)
            if attempt_id is None or (latest and latest.attempt["id"] == attempt_id):
                return latest
            return self.recent_captures.get((station, attempt_id))

    def messages(self):
        # Reconnects and slow readers receive a consistent recent history and
        # active-link snapshot. The simulator never waits for a browser.
        with self.changed:
            cursor = self.revision
            snapshot = self.latest
        yield event_bytes("snapshot", snapshot)
        while True:
            with self.changed:
                self.changed.wait_for(lambda: self.revision > cursor, timeout=10)
                if self.events and cursor < self.events[0][0] - 1:
                    batch = [event_bytes("snapshot", self.latest)]
                else:
                    batch = [event for revision, event in self.events if revision > cursor]
                cursor = self.revision
            yield from batch or [b": keepalive\n\n"]


def run(simulation, stop, speed, duration, stream=None):
    start = time.monotonic()
    for tick in itertools.count():
        if stop.is_set():
            break
        events = simulation.advance(tick, duration)
        simulation.wall_elapsed = time.monotonic() - start
        if stream is not None:
            lag = simulation.wall_elapsed - (tick + 1) / TICK_HZ / speed
            simulation.peak_tick_lag = max(simulation.peak_tick_lag, lag)
        if stream is not None and (events or tick % TICK_HZ == 0 or not simulation.running):
            if tick % TICK_HZ == 0 or not simulation.running:
                events.append(("status", simulation.status()))
            stream.publish(simulation.snapshot(), events, simulation.waveforms, simulation.previous_waveforms)
        if not simulation.running:
            break
        if stream is not None:
            deadline = start + (tick + 1) / TICK_HZ / speed
            stop.wait(max(0, deadline - time.monotonic()))


class Handler(SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=WEB_ROOT, **kwargs)

    def translate_path(self, url):
        path = Path(super().translate_path(url))
        parts = path.relative_to(WEB_ROOT).parts
        if parts == (CATALOG_PATH.name,):
            return str(CATALOG_PATH)
        if parts and parts[0] == "vendor":
            if parts == ("vendor", SATELLITE_IMAGE.name):
                return str(SATELLITE_IMAGE)
            if len(parts) == 2 and parts[1] in SIGPLOT_FILES:
                return str(SIGPLOT_ROOT / SIGPLOT_FILES[parts[1]])
            return str(self.server.cesium_dir.joinpath(*parts[1:]))
        return str(path)

    def do_GET(self):
        url = urlsplit(self.path)
        if url.path.startswith("/waveforms/"):
            self.send_waveform(url.path.removeprefix("/waveforms/"), url.query)
            return
        if url.path != "/events":
            super().do_GET()
            return

        if not self.server.listener_slots.acquire(blocking=False):
            self.send_error(503, "Simulation stream is at capacity")
            return
        try:
            self.connection.settimeout(20)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream; charset=utf-8")
            self.send_header("Cache-Control", "no-cache, no-transform")
            self.send_header("X-Accel-Buffering", "no")
            self.send_header("Connection", "close")
            self.end_headers()
            self.close_connection = True
            for event in self.server.stream.messages():
                self.wfile.write(event)
                self.wfile.flush()
        except (ConnectionError, TimeoutError):
            pass
        finally:
            self.server.listener_slots.release()

    def send_waveform(self, station, query):
        try:
            params = parse_qs(query, keep_blank_values=True, max_num_fields=4)
            if params.keys() - {"after", "attempt", "stage"} or any(len(v) != 1 for v in params.values()):
                raise ValueError("unexpected or duplicated parameter")
            stage = params.get("stage", ["tx"])[0]
            if stage not in ("tx", "rx"):
                raise ValueError("expected tx or rx")
            attempt_id = int(params["attempt"][0]) if "attempt" in params else None
            after = params.get("after", [None])[0]
        except ValueError:
            self.send_error(400, "Invalid waveform query")
            return
        pair = self.server.stream.capture(station, attempt_id)
        if pair is None:
            self.send_error(404, "No matching transmission capture for this station")
            return
        attempt, encoded, receiver = pair.attempt, pair.encoded, pair.receiver
        if stage == "rx" and receiver is None and attempt["decode_status"] != "pending":
            self.send_error(404, "Transmitted burst has no receiver capture")
            return
        if (stage == "rx" and receiver is None) or after == str(attempt["id"]):
            self.send_response(204)
            self.send_header("Cache-Control", "no-store")
            if stage == "rx" and receiver is None:
                self.send_header("X-Waveform-Status", "pending")
            self.end_headers()
            return
        samples = receiver["samples"] if stage == "rx" else encoded.samples
        samples = samples.astype("<f4", copy=False)
        from spectral import analyze

        body = json.dumps({
            "attempt": attempt,
            "stage": stage,
            "channel_model": attempt["channel_model"],
            "decode_status": attempt["decode_status"],
            "profile": attempt.get("profile"),
            "reference_snr_db": attempt.get("reference_snr_db", attempt["snr_db"]),
            "measured_snr_db": attempt.get("measured_snr_db"),
            "measured_sinr_db": attempt.get("measured_sinr_db"),
            "interferers": attempt.get("interferers", []),
            "reference_band_hz": receiver.get("reference_band_hz") if receiver else None,
            "sample_rate_hz": encoded.sample_rate,
            "sample_count": len(samples),
            "waveform_f32le": base64.b64encode(samples.tobytes()).decode("ascii"),
            "analysis": analyze(samples, encoded.sample_rate),
        }, separators=(",", ":"), allow_nan=False).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stations", type=int, default=24, metavar=f"2–{MAX_STATIONS}")
    parser.add_argument("--band", choices=[band["id"] for band in BANDS],
                        help="use a single shared band instead of all three")
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--speed", type=float, default=1, help="simulated seconds per wall-clock second")
    parser.add_argument("--duration", type=float, help="simulated seconds of traffic generation")
    parser.add_argument("--headless", action="store_true", help="run to --duration at full speed and print JSON")
    weather_options = parser.add_mutually_exclusive_group()
    weather_options.add_argument("--weather", choices=("live", "scenario"),
                                 help="current NOAA space weather (web default) or a seeded scenario (headless default)")
    weather_options.add_argument("--weather-replay", type=Path, help="apply recorded NOAA inputs from a local manifest")
    parser.add_argument("--forecast", type=Path, help="cache of pinned P.533 path-hour predictions")
    parser.add_argument("--modem", type=Path, help="Linkwise CLI binary for BW500 level-1 bursts")
    parser.add_argument("--channel-model", choices=("flat", "watterson-moderate"),
                        help="select the channel (default: Watterson for native BW500, flat for BPSK)")
    parser.add_argument("--medium", choices=("exclusive", "shared"), default="exclusive",
                        help="allow up to two uncoordinated same-band transmitters in shared mode")
    parser.add_argument("--interference", choices=INTERFERENCE_PROFILES, default="none",
                        help="inject deterministic receiver interference")
    antenna_options = parser.add_mutually_exclusive_group()
    antenna_options.add_argument("--antenna-preset", choices=ANTENNA_PRESETS,
                                 help="opt in to a modeled station antenna lineup")
    antenna_options.add_argument("--antenna-config", type=Path,
                                 help="station TX/RX antennas, headings and transmit powers from JSON")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8752)
    parser.add_argument("--cesium-dir", type=Path, default=CESIUM_ROOT)
    args = parser.parse_args()
    channel_model = args.channel_model or ("watterson-moderate" if args.modem else "flat")
    if not 2 <= args.stations <= MAX_STATIONS:
        parser.error(f"--stations must be between 2 and {MAX_STATIONS}")
    if not 0 <= args.seed < 2**32:
        parser.error("--seed must be between 0 and 4294967295")
    if not 0 < args.speed <= 25:
        parser.error("--speed must be positive and at most 25")
    if args.duration is not None and (not math.isfinite(args.duration) or args.duration <= 0):
        parser.error("--duration must be finite and positive")
    if args.headless and args.duration is None:
        parser.error("--headless requires --duration")
    if channel_model != "flat" and args.modem is None:
        parser.error("the Watterson channel requires --modem")
    if args.forecast and channel_model != "watterson-moderate":
        parser.error("--forecast requires the 3 kHz SNR reference of --channel-model watterson-moderate")
    if not args.headless and not (args.cesium_dir / "Cesium.js").is_file():
        parser.error("Cesium assets are missing. Run python3 globe/prepare.py from the repository root.")

    try:
        from hf_channel import Channel
    except ModuleNotFoundError as error:
        parser.error(f"{error}. Use the Python environment containing GNU Radio and NumPy.")

    channel = Channel(args.modem, channel_model=channel_model)
    if args.modem is not None:
        channel.airtime_s(PAYLOAD.size)
    forecast = None
    weather = None
    antennas = None
    try:
        if args.antenna_preset or args.antenna_config:
            antennas = load_configuration(make_stations(args.stations),
                                          preset=args.antenna_preset, path=args.antenna_config)
        if args.forecast:
            from path_forecast import PathForecast

            forecast = PathForecast.load(args.forecast)
        if args.weather_replay:
            from space_weather import ReplayWeatherFeed

            weather = ReplayWeatherFeed.load(args.weather_replay)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    live_weather = not args.weather_replay and (args.weather == "live" or (args.weather is None and not args.headless))
    if live_weather:
        from space_weather import WeatherFeed

        weather = WeatherFeed()
        if args.headless:
            weather.refresh()
        else:
            weather.start()

    simulation = Simulation(channel, args.stations, args.seed, args.band, weather,
                            medium=args.medium, interference=args.interference,
                            forecast=forecast, retain_waveforms=not args.headless, antennas=antennas)
    stop = threading.Event()
    if args.headless:
        run(simulation, stop, args.speed, args.duration)
        print(json.dumps(simulation.snapshot()))
        return

    stream = EventStream(simulation.snapshot())

    def simulate():
        try:
            run(simulation, stop, args.speed, args.duration, stream)
        except Exception as error:
            traceback.print_exc()
            simulation.stop_with_error(error)
            snapshot = simulation.snapshot()
            stream.publish(snapshot, [("snapshot", snapshot)], simulation.waveforms, simulation.previous_waveforms)

    with ThreadingHTTPServer((args.host, args.port), Handler) as server:
        server.cesium_dir = args.cesium_dir.resolve()
        server.stream = stream
        server.listener_slots = threading.BoundedSemaphore(16)
        worker = threading.Thread(target=simulate, daemon=True)
        worker.start()
        print(f"Linkwise HF live at http://{args.host}:{server.server_port}/", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            stop.set()
            worker.join()
            if weather is not None:
                weather.stop()


if __name__ == "__main__":
    main()
