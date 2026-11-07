# HF propagation toolbox

This document records the tools identified in the shared “HF Propagation
Reentry Effects” discussion and the information needed to use them later for
the Julian, California 10 m VARA HF / Winlink experiment.

The tools have different evidentiary roles. VOACAP is a statistical HF
prediction model; Winlink supplies the actual gateway/channel inventory;
PSK Reporter and DX Maps provide reports of observed activity; GIRO provides
ionosonde measurements; and KeepTrack provides orbital/reentry information.
None of these tools, by itself, predicts a sporadic-E opening caused by a
reentry.

## Research reference: measured Falcon 9 lithium plume

Wing et al. report a directly measured upper-atmospheric lithium plume
associated with the uncontrolled re-entry of a SpaceX Falcon 9 upper stage on
19 February 2025. A resonance lidar at Kühlungsborn, Germany, detected a
roughly tenfold lithium enhancement near 96 km altitude about 20 hours after
the re-entry. The observed layer spanned 94.5–96.8 km and was detected for
about 40 minutes. An ensemble of wind-informed back trajectories connected the
sampled air mass to the Falcon 9 re-entry path west of Ireland. The study also
examined ionosonde, meteor-radar wind, and geomagnetic observations; these did
not support a natural sporadic-E-related explanation for this particular
lithium enhancement.

This is evidence that rocket re-entry can inject a detectable anthropogenic
metal signature into the mesosphere/lower thermosphere, and that remote
sensing plus atmospheric transport modelling can associate an observation
with a particular event. It is not evidence that the event caused an HF or
10 m propagation opening. The lidar measured lithium, not radio propagation;
the reported case is one event and one observed plume. It therefore does not
validate a reentry-to-sporadic-E or reentry-to-link-success relationship, nor
provide a propagation forecast or a basis for live channel ranking.

For Linkwise, retain re-entry predictions/observations as a separately
provenanced research covariate. A propagation study would still need repeated
event and control windows, independent ionosonde and 10 m observations, and
timestamped successes and failures for relevant paths. The article's
ionosonde/radar context is a useful model for recording independent evidence
that can test alternative explanations. See the authoritative article:
Wing, R., Gerding, M., Plane, J. M. C., et al., “Measurement of a lithium
plume from the uncontrolled re-entry of a Falcon 9 rocket,”
\emph{Communications Earth \& Environment} 7, 161 (2026),
doi:10.1038/s43247-025-03154-8.
<https://doi.org/10.1038/s43247-025-03154-8>

## Quick reference

| Tool | Primary use | Data type | Automation status |
| --- | --- | --- | --- |
| [VOACAP](https://www.voacap.com/) | Point-to-point HF prediction and MUF/reliability planning | Statistical model output | Use the online service manually; the site prohibits automated access unless separately authorized. Use standalone VOACAPL for local automation. |
| [VOACAPL](https://github.com/jawatson/voacapl) | Local/scriptable VOACAP engine | Fortran model and generated files | Candidate local engine; build and validate before relying on results. |
| [KeepTrack API](https://api.keeptrack.space/) | Space-object catalog and reentry analysis | Orbital/catalog data | API key required; current stable API is v4. Data license is CC BY-NC 4.0. |
| [Winlink RMS Channels](https://winlink.org/RMSChannels) | Find current RMS gateways, frequencies, modes, and status | Live gateway/channel directory | Refresh before operating. Gateway status is time-sensitive. |
| [PSK Reporter](https://pskreporter.info/) | Observe where digital-mode signals are being received | Reception reports | Website use is immediate; software can submit reports through the documented DLL/API. |
| [DX Maps](https://www.dxmaps.com/) | Visualize recent spots and propagation activity | QSO, cluster, and reporter data | Best used interactively; use its filters and historical views for analysis. |
| [GIRO / DIDBase](https://giro.uml.edu/didbase/) | Inspect ionograms and ionospheric characteristics | Ionosonde measurements and derived data | Manual web access, downloads, or SAO Explorer; observe license/acknowledgement rules. |

## 1. VOACAP and VOACAPL

### What it does

VOACAP covers HF propagation planning in roughly the 3–30 MHz range. For a
specified transmitter, receiver, frequency, month, hour, antenna model,
power, noise environment, and path, it can produce quantities such as
reliability, signal-to-noise estimates, MUF guidance, and predicted usable
frequencies.

The online [VOACAP for Ham Radio guide](https://www.voacap.com/) and the
[VOACAP Online manual](https://www.voacap.com/2023/documents/VOACAP_Online_User_Manual_16_July_2024.pdf)
are the starting references. Use separate predictions for each RMS gateway;
an averaged “global” prediction is not sufficient for channel selection.

### Initial station model

For the Julian experiment, preserve these as explicit input fields rather
than hidden assumptions:

- Transmitter: approximately DM13 / Julian, California.
- Frequency: 28 MHz / 10 m.
- Antenna: horizontal dipole, with separate 5 m and 10 m scenarios.
- Orientation: East–West; compare gateway bearing with the dipole’s broadside
  directions.
- Power: begin with measured feed-point power, not automatically the radio’s
  100 W maximum. Estimate feed-line and connector loss separately.
- Path: short path initially; calculate each gateway independently.
- Month and UTC hour: use the actual analysis date and convert displayed
  local times carefully.

Run a normal F-layer baseline first. If the interface exposes an Es option,
run it only as a labeled comparison because the Es result is not equivalent
to a verified real-time Es forecast.

### Automation caution

The VOACAP website states that automated access to its online services is
prohibited unless authorized by the site owner. Do not build a scraper around
the online calculator. For reproducible local work, investigate
[VOACAPL](https://github.com/jawatson/voacapl), the Linux port of the
Fortran engine. Its release history documents command-line operation and
CSV-oriented utilities, but the local build and output conventions still need
to be validated in this repository before integration.

### Required record for every prediction

Store the input parameters, engine/version, run timestamp, gateway
coordinates, output files, and interpretation. A prediction without its
inputs is not reproducible evidence.

## 2. Winlink RMS Channels

The official [RMS Channels page](https://winlink.org/RMSChannels) is the
authoritative starting point for current gateway status and channel data.
Winlink’s page identifies status colors as follows: green means normal
reporting, yellow means more than two hours since the last report, and red
means more than four hours. Gateways disappear after six hours without a
report.

Winlink Express also maintains a channel list that can be updated via the
Internet or radio. The official FAQ recommends refreshing it regularly. For
this project, the channel record should include:

- callsign and service code;
- gateway location/grid, distance, and bearing from Julian;
- exact dial/channel frequency;
- mode and VARA profile;
- reporting status and timestamp;
- whether the channel is PUBLIC and available to the operator;
- the time the list was retrieved.

Do not infer that a gateway supports 10 m VARA HF merely because it appears
on the RMS map. Confirm the advertised band and mode. Before transmitting,
confirm the frequency is clear and that operation is permitted under the
station’s license and local band plan.

## 3. PSK Reporter

[PSK Reporter](https://pskreporter.info/) is an observed-signal network. It
can show where a digital signal was received, but it is not a prediction
engine and it does not establish that a VARA channel is usable.

For the 10 m experiment, use it to record:

- UTC time of report;
- sender and receiver callsigns or grids;
- frequency/band and mode;
- SNR or signal report when available;
- distance and bearing;
- whether reports cluster along a potential gateway path.

The [PSK Reporter DLL documentation](https://www.pskreporter.info/PSKReporter.pdf)
describes the Windows submission API. The basic sequence is to initialize a
reporter session, submit recognized callsigns with `ReporterSeenCallsign`,
periodically call `ReporterTickle`, and uninitialize on shutdown. This is for
submitting observations; it is not a general-purpose historical-query API.

Do not transmit VARA on an FT8 frequency merely to create a report. Use a
properly published Winlink channel and treat nearby FT8/other-mode activity
as an independent propagation indicator.

## 4. DX Maps

The [DX Maps manual](https://moon.dxmaps.com/spots/manual.html) describes
real-time maps/lists of amateur-radio and SWL contacts and reception reports.
Useful features for this project include:

- band and mode filters;
- callsign, continent, grid, and distance filters;
- “only last 15 min” for a live snapshot;
- the PSK Reporter source filter;
- the MUF-Es map for an observed/estimated Es view;
- VOACAP short-path and long-path overlays;
- historical propagation-index data.

Use DX Maps as a situational-awareness and corroboration tool. Export or
manually capture the query time, selected band, filters, map area, and visible
reports. Its VOACAP overlay is still model output, while the spots are
observations from reporting networks; keep those evidence classes separate.

## 5. GIRO, DIDBase, and ionosonde data

[GIRO](https://giro.uml.edu/) provides real-time and retrospective HF
ionospheric sounding data. The [DIDBase portal](https://giro.uml.edu/didbase/)
provides ionogram images and derived characteristics. The [GIRO ionogram
description](https://giro.uml.edu/ionogram-data.html) explains that an
ionogram plots reflected signal behavior against sounding frequency and
travel time, with ARTIST-derived traces and electron-density profiles.

Useful access paths are:

- [DIDBase ionogram portal](https://giro.uml.edu/didbase/) for station/day
  browsing;
- [FastChar](https://giro.uml.edu/didbase/scaled.php) for time-bounded
  ionogram-derived characteristics and downloads;
- [24-hour ionogram movies](https://giro.uml.edu/IonogramMovies/) for rapid
  visual inspection;
- [SAO Explorer information](https://giro.uml.edu/sao-explorer.html) for
  deeper ionogram inspection and numerical data;
- [Drift Explorer information](https://giro.uml.edu/drift-explorer.html) for
  Doppler skymaps and drift data.

For Es research, record the station, URSI code, UTC interval, product type,
data latency, and any quality/confidence indicator. A local ionogram shows
ionospheric conditions near that sounder; it does not directly prove a
Julian-to-gateway radio path. Follow the portal’s “Rules of the Road” and
acknowledgement/license requirements before bulk downloading or redistributing
data.

## 6. KeepTrack

[KeepTrack](https://keeptrack.space/) is a satellite/space-object tracking
application. Its [official analysis-menu documentation](https://docs.keeptrack.space/plugins/analysis-menu/)
describes catalog export, close-object searches, potential-reentry searches,
and pass-time calculations.

For programmatic access, use the [KeepTrack API](https://api.keeptrack.space/).
The current stable API is v4; older v1–v3 APIs are documented as shut down.
An API key is required, and the API is rate-limited. The API data are licensed
CC BY-NC 4.0, so attribution and non-commercial-use constraints must be
reviewed before incorporating the data into a distributed product.

For the propagation experiment, store reentry data separately from radio
observations:

- object identifier and source;
- predicted reentry time and uncertainty, if supplied;
- prediction retrieval time and API version;
- geographic corridor/footprint, if supplied;
- actual observation or revised estimate;
- links or raw response retained for provenance.

KeepTrack is an event-timing source, not evidence that a reentry causes
sporadic-E. The correct test is to compare its events against independently
observed 10 m openings over many events, including non-event control periods.

## Recommended toolbox workflow

1. Refresh Winlink RMS channels and retain a timestamped gateway snapshot.
2. Select candidate 10 m VARA gateways by availability, distance, bearing,
   and service code.
3. Run or retrieve a separately documented VOACAP prediction for each
   candidate gateway.
4. Check PSK Reporter and DX Maps for current 10 m activity along the path.
5. Inspect relevant GIRO stations and record ionogram/derived-data evidence.
6. Query KeepTrack only to add a separately labeled reentry event marker.
7. Attempt a short Winlink connection only when the channel is current, clear,
   legal, and supported by multiple indicators.
8. Log success/failure, connection duration, mode/rate, gateway, UTC time,
   and all input snapshots.

The resulting dataset should allow us to test whether reentry-associated
periods differ from ordinary 10 m propagation, without turning an unverified
causal hypothesis into an operating rule.

## Repository status

This is a research reference, not an implementation. No live gateway list,
VOACAP prediction, reentry forecast, or propagation measurement has been
generated by adding this document. The project’s evidence ledger remains in
[`lab-notes.tex`](../lab-notes/lab-notes.tex).
