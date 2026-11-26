"""Run independently generated station traffic over shared simulated HF bands."""

import random
import struct
from collections import Counter, deque
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from medium import add_overlap, injected_interference
from stations import BANDS, make_stations, path_conditions

if TYPE_CHECKING:
    from hf_channel import EncodedBurst
    from space_weather import WeatherSnapshot


TICK_HZ = 10
MAX_QUEUE = 6
MAX_RETRIES = 2
PATH_VARIATION_SECONDS = 300
BUSY_BAND_REACCESS_MEAN_S = 8.0
PAYLOAD = struct.Struct(">IBBBH")


@dataclass
class Message:
    id: int
    source: int
    target: int
    value: int
    ready: float
    retry: int = 0

    def encode(self):
        return PAYLOAD.pack(self.id, self.source + 1, self.target + 1, 1, self.value)


@dataclass(frozen=True, slots=True)
class Transmission:
    attempt_id: int
    source: int
    encoded: "EncodedBurst"
    start_s: float
    seed: int
    weather: "WeatherSnapshot | None"


@dataclass
class Burst:
    message: Message
    event: dict
    transmission: Transmission
    fading_db: float
    completion: float
    overlaps: list[Transmission] = field(default_factory=list)


@dataclass(frozen=True, slots=True)
class WaveformPair:
    attempt: dict
    encoded: "EncodedBurst"
    receiver: dict | None = None


class Simulation:
    def __init__(self, channel, station_count, seed, band=None, weather_feed=None, *,
                 medium="exclusive", interference="none", forecast=None, retain_waveforms=True):
        self.channel = channel
        self.weather_feed = weather_feed
        self.weather = weather_feed.snapshot(0) if weather_feed else None
        self.seed = seed
        self.traffic_rng = random.Random(seed)
        self.policy_rng = random.Random(seed ^ 0xA81B)
        self.rf_rng = random.Random(seed ^ 0xE42D)
        self.access_rng = random.Random(seed ^ 0x5AFC)
        self.stations = make_stations(station_count)
        self.bands = [choice for choice in BANDS if band is None or choice["id"] == band]
        self.medium = medium
        self.interference = interference
        self.forecast = forecast
        self.retain_waveforms = retain_waveforms
        self.next_access = dict.fromkeys((choice["id"] for choice in self.bands), 0)
        self.next_generation = [self.traffic_rng.uniform(0, 4) for _ in self.stations]
        self.queues = [deque() for _ in self.stations]
        self.active = {}
        self.waveforms = {}
        self.previous_waveforms = deque(maxlen=16)
        self.recent = deque(maxlen=32)
        self.counts = Counter()
        self.band_attempts = Counter()
        self.band_successes = Counter()
        self.elapsed = 0.0
        self.wall_elapsed = 0.0
        self.peak_tick_lag = 0.0
        self.running = True
        self.error = None

    def stats(self):
        return {
            **dict.fromkeys(("generated", "attempted", "delivered", "failed",
                             "retried", "dropped", "aborted", "overlapped"), 0),
            **self.counts,
            "active": len(self.active),
            "queued": sum(map(len, self.queues)),
        }

    def snapshot(self):
        return {
            "stations": self.stations,
            "bands": self.bands,
            "active": [burst.event for burst in self.active.values()],
            "recent": list(self.recent),
            "stats": self.stats(),
            "seed": self.seed,
            "running": self.running,
            "error": self.error,
            "elapsed_s": self.elapsed,
            "wall_elapsed_s": self.wall_elapsed,
            "peak_tick_lag_s": self.peak_tick_lag,
            "channel": self.channel.name,
            "channel_model": self.channel.channel_model,
            "medium": self.medium,
            "interference": self.interference,
            "sample_rate_hz": self.channel.sample_rate,
            "weather": self.weather_status(),
            "forecast": self.forecast.status() if self.forecast else None,
        }

    def _generate(self):
        for source, deadline in enumerate(self.next_generation):
            if deadline > self.elapsed:
                continue
            self.next_generation[source] = self.elapsed + self.traffic_rng.uniform(10, 27)
            # Uniformly select one of the other stations.
            target = self.traffic_rng.randrange(len(self.stations) - 1)
            if target >= source:
                target += 1
            self.counts["generated"] += 1
            message = Message(self.counts["generated"], source, target,
                              self.traffic_rng.randrange(65536), self.elapsed)
            queue = self.queues[source]
            if len(queue) < MAX_QUEUE:
                queue.append(message)
            else:
                self.counts["dropped"] += 1

    def _notify(self, event):
        return "attempt", {"attempt": event, "stats": self.stats(), "elapsed_s": self.elapsed}

    def _path(self, source, target, band, start_s, weather, rng):
        slow_offset = None
        if self.channel.channel_model != "flat":
            # A slow station-pair term moves the nominal noise/link budget
            # over minutes. The waveform channel independently handles fades.
            period, phase = divmod(start_s, PATH_VARIATION_SECONDS)
            key = f"{self.seed}:{source}:{target}:{band}"
            now = random.Random(f"{key}:{int(period)}").gauss(0, 5.5)
            later = random.Random(f"{key}:{int(period) + 1}").gauss(0, 5.5)
            slow_offset = now + (later - now) * phase / PATH_VARIATION_SECONDS
        return path_conditions(self.stations[source], self.stations[target],
                               band, start_s, rng, weather, forecast=self.forecast, slow_offset_db=slow_offset)

    def _propagate(self, transmission, target, band, snr_db, fading_db, *, noise):
        return self.channel.propagate(
            transmission.encoded, seed=transmission.seed, snr_db=snr_db,
            fading_db=fading_db, path_key=(self.seed, transmission.source, target, band),
            started_at_s=transmission.start_s, include_noise=noise,
        )

    def _receive(self, burst):
        tx = burst.transmission
        target = burst.message.target
        band = burst.event["band"]
        snr_db = burst.event["snr_db"]
        if not burst.overlaps and self.interference == "none":
            return self.channel.receive(
                tx.encoded, seed=tx.seed, snr_db=snr_db, fading_db=burst.fading_db,
                path_key=(self.seed, tx.source, target, band), started_at_s=tx.start_s,
                capture=True,
            )

        signal, noise = self._propagate(tx, target, band, snr_db, burst.fading_db, noise=True)
        rate = tx.encoded.sample_rate
        noise_power = self.channel.reference_power(tx.encoded.samples) * 10 ** (-snr_db / 10)
        interference = injected_interference(
            self.interference, rate=rate, count=len(signal), start_s=tx.start_s,
            noise_power=noise_power, seed=tx.seed ^ 0x638CFD,
        )
        for other in burst.overlaps:
            other_snr, fading_db, _ = self._path(
                other.source, target, band, other.start_s, other.weather,
                random.Random(other.seed ^ (target + 1)),
            )
            incoming, _ = self._propagate(other, target, band, other_snr, fading_db, noise=False)
            # Use the wanted receiver's noise floor and each interfering path's
            # independent link budget. Scale before fading so fades survive.
            clean_power = self.channel.reference_power(other.encoded.samples)
            gain = (noise_power * 10 ** (other_snr / 10) / clean_power) ** 0.5
            offset = round(other.start_s * rate) - round(tx.start_s * rate)
            add_overlap(interference, incoming, offset, gain)
        return self.channel.decode_received(
            tx.encoded, signal, noise, interference=interference, capture=True,
        )

    def _complete(self):
        for attempt_id, burst in list(self.active.items()):
            if burst.completion > self.elapsed:
                continue
            msg = burst.message
            payload = msg.encode()
            received, receiver = self._receive(burst)
            del self.active[attempt_id]
            delivered = received == payload
            interferers = [other.attempt_id for other in burst.overlaps]
            self.counts["overlapped"] += bool(interferers)
            if delivered:
                self.counts["delivered"] += 1
                self.band_successes[msg.source, burst.event["band"]] += 1
            else:
                self.counts["failed"] += 1
                queue = self.queues[msg.source]
                if msg.retry < MAX_RETRIES and len(queue) < MAX_QUEUE:
                    msg.retry += 1
                    msg.ready = self.elapsed + self.policy_rng.uniform(1.5, 5) * msg.retry
                    queue.appendleft(msg)
                else:
                    self.counts["dropped"] += 1
            event = {
                **burst.event,
                "state": "delivered" if delivered else "failed",
                "decode_status": "delivered" if delivered else "failed",
                "simulated_at_s": self.elapsed,
                "measured_snr_db": receiver.get("measured_snr_db"),
                "measured_sinr_db": receiver.get("measured_sinr_db"),
                "profile": receiver.get("profile"),
                "interferers": interferers,
            }
            if self.retain_waveforms:
                self.waveforms[event["source"]] = WaveformPair(event, burst.transmission.encoded, receiver)
            self.recent.appendleft(event)
            yield self._notify(event)

    def _band_score(self, source, band):
        observations = self.band_attempts[source, band]
        successes = self.band_successes[source, band]
        return (successes + 1) / (observations + 2) + self.policy_rng.uniform(0, 0.4)

    def _start(self):
        busy = {endpoint for burst in self.active.values()
                for endpoint in (burst.message.source, burst.message.target)}
        using = Counter(burst.event["band"] for burst in self.active.values())
        capacity = 2 if self.medium == "shared" else 1
        free_bands = [band["id"] for band in self.bands
                      if using[band["id"]] < capacity
                      and (not using[band["id"]] or self.next_access[band["id"]] <= self.elapsed)]
        pending = sorted((queue[0] for queue in self.queues if queue and queue[0].ready <= self.elapsed),
                         key=lambda msg: (msg.ready, msg.id))
        for msg in pending:
            if not free_bands:
                break
            if msg.source in busy or msg.target in busy:
                continue
            encoded = self.channel.prepare(msg.encode())
            band = max(free_bands, key=lambda choice: self._band_score(msg.source, choice))
            free_bands.remove(band)
            if self.medium == "shared":
                # A busy band becomes eligible for another uncoordinated
                # caller after a seeded random wait. Idle bands stay eligible.
                delay = self.access_rng.expovariate(1 / BUSY_BAND_REACCESS_MEAN_S)
                self.next_access[band] = self.elapsed + 0.2 + delay
            busy.update((msg.source, msg.target))
            self.queues[msg.source].popleft()
            self.counts["attempted"] += 1
            self.counts["retried"] += int(msg.retry > 0)
            self.band_attempts[msg.source, band] += 1
            source = self.stations[msg.source]
            target = self.stations[msg.target]
            snr_db, fading_db, space_weather = self._path(
                msg.source, msg.target, band, self.elapsed, self.weather, self.rf_rng,
            )
            tx = Transmission(
                self.counts["attempted"], msg.source, encoded, self.elapsed,
                self.rf_rng.randrange(1, 2**31), self.weather,
            )
            event = {
                "id": tx.attempt_id,
                "message_id": msg.id,
                "source": source["id"],
                "target": target["id"],
                "band": band,
                "state": "active",
                "decode_status": "pending",
                "retry": msg.retry,
                "simulated_at_s": self.elapsed,
                "started_at_s": self.elapsed,
                "snr_db": round(snr_db, 1),
                "airtime_s": round(encoded.airtime_s, 3),
                "payload_bytes": PAYLOAD.size,
                "channel_model": self.channel.channel_model,
                "interference": self.interference,
            }
            if space_weather is not None:
                event["space_weather"] = space_weather
            if self.retain_waveforms:
                previous = self.waveforms.get(source["id"])
                if previous is not None:
                    self.previous_waveforms.append(previous)
                self.waveforms[source["id"]] = WaveformPair(event, encoded)
            burst = Burst(msg, event, tx, fading_db, self.elapsed + encoded.airtime_s)
            for other in self.active.values():
                if other.event["band"] == band:
                    burst.overlaps.append(other.transmission)
                    other.overlaps.append(tx)
            self.active[tx.attempt_id] = burst
            yield self._notify(event)

    def advance(self, tick, duration=None):
        self.elapsed = tick / TICK_HZ
        if self.weather_feed is not None:
            self.weather = self.weather_feed.snapshot(self.elapsed)
        events = list(self._complete())
        if duration is None or self.elapsed < duration:
            self._generate()
            events.extend(self._start())
        elif not self.active:
            self.running = False
        return events

    def weather_status(self):
        return self.weather.status() if self.weather is not None else {"mode": "scenario"}

    def status(self):
        return {"running": self.running, "error": self.error,
                "stats": self.stats(), "elapsed_s": self.elapsed, "weather": self.weather_status(),
                "medium": self.medium, "interference": self.interference,
                "wall_elapsed_s": self.wall_elapsed, "peak_tick_lag_s": self.peak_tick_lag}

    def stop_with_error(self, error):
        self.running = False
        self.error = str(error) or type(error).__name__
        self.counts["aborted"] += len(self.active)
        for burst in self.active.values():
            if self.retain_waveforms:
                event = {**burst.event, "state": "aborted", "decode_status": "aborted"}
                self.waveforms[event["source"]] = WaveformPair(event, burst.transmission.encoded)
        self.active.clear()
