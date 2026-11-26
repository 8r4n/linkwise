# Modeled HF antenna profiles

The shared catalog describes per-band transmit and receive gain in dBi. The
band-tuned dipole and rotatable 20 m Yagi support the same-band comparison.
The isotropic reference has exactly zero gain. The Kiwi-inspired classes come
from reported receiver configurations. The catalog gives each profile its own
source and distinguishes modeled pattern values from operator specifications.

- The inventory covers reported dipoles, terminated folded dipoles, verticals,
  active loops, Beverages and TCI arrays among other installations. Its Kiwi
  score measures waterfall spectral contrast. The score is excluded from gain,
  interference and noise calculations. [KiwiSDR antenna study](../../reference/kiwisdr-antenna-study/2026-11-18-summary.json)
- KFS publishes nominal gain, band coverage, beam width and bearing for its
  receive-only TCI antennas. The TCI elevation shapes in this catalog are
  approximations. The TCI 527B starts at 6.2 MHz and omits 80 m. [KFS antenna details](https://www.kfsdr.org/about)

A heading points along the peak of a directional profile or across the wire
of a bidirectional profile. Bearings use the great-circle path at each
station. The elevation model splits the path into equal hops no longer than
2,400 km, uses a 300 km apex and clamps the resulting endpoint elevation at
5°. This simplified geometry does not select measured ionospheric modes.
The angular model applies bounded azimuth and elevation losses. A horizontal
to vertical pairing adds a modeled 3 dB polarization loss.

The existing isotropic path model supplies the reference SNR and the noise
level. The receiver scales the wanted samples by the TX and RX pattern gains,
their separately declared feedline losses, the polarization term and any
conducted power change. It scales each cochannel signal with the interferer's
own TX pattern and the victim's frozen RX pattern. The noise waveform stays
fixed for antenna comparisons. Directional EIRP multiplies conducted TX watts
by the linear factor for TX gain minus TX feedline loss along the modeled path.

## Native simulator

Use `--antenna-preset kiwi-inspired` for an illustrative lineup or
`--antenna-config PATH` for fixed mounts. For an explicit setup:

```json
{
  "schema": 1,
  "stations": {
    "SIM-022": {
      "tx": {"profile_id": "yagi-20m", "heading_deg": 323.95},
      "power_w": 100
    },
    "SIM-038": {
      "rx": {"profile_id": "yagi-20m", "heading_deg": 139.73}
    }
  }
}
```

Unspecified ports use the isotropic reference. Unspecified power inherits the
matched P.533 cache's reference power or the 100 W heuristic reference. A
receive-only profile cannot be configured for TX. A route lacking a common band
is counted as an unsupported path. Omitting the antenna options keeps the
existing simulator configuration.

Each antenna-enabled attempt includes its mounted TX and RX profiles, gains,
bearings and elevation, conducted power, and directional EIRP. Completed bursts
with overlaps also include the interferers' evaluated antenna contexts. Capture
PCM remains the samples used by the decoder.

## Replacing an illustrative pattern with NEC results

In [4nec2 for Windows](https://www.qsl.net/4nec2/Home.htm), create or open a
`.nec` deck for the installed antenna. Record wire dimensions and height,
feedpoint, ground conductivity and permittivity, and mount orientation.
Generate a full 3D far-field pattern in total gain (dBi) at each modeled band
frequency. Retain the deck, solver version, and output grid. Convert the
solver's angular convention to elevation above the horizon and azimuth
relative to the mounted heading. Record whether matching or feedline losses
are already included before applying any separate catalog loss.
[4nec2 feature list](https://www.qsl.net/4nec2/features.htm)

Compare front, side, and rear cuts with the model before fitting a catalog
profile. Full tabulated pattern import would need a new catalog schema. An
independently checked 4nec2 Type 13 or Type 14 export could also feed a future
P.533 prediction build with explicit TX and RX antennas. Regenerate a baseline
for those patterns and apply each gain and loss once. [4nec2 export notes](https://www.qsl.net/4nec2/_ReleaseNotes.htm)
