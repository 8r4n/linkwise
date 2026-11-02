# Winlink link inventory and tracking design

**Document ID:** LINKWISE-DES-WL-001  
**Version:** 0.1, proposed  
**Status:** Research-based design proposal; no Winlink tracker implemented

**Documentation review date:** 2026-11-01. Winlink pages and client workflows
can change; verify sources and adapter capabilities during implementation.

## 1. Purpose

Define how Linkwise will keep accurate counts of Winlink gateway choices,
channels, observed status, radio-session attempts, and message-transfer results
for an AI link manager. The design separates network inventory from live RF
reachability and message delivery. The model consumes summaries and requests
bounded actions; a local Linkwise service performs synchronization, counting,
history, and safety-critical monitoring.

## 2. Winlink concepts and limits of available information

The Winlink Book of Knowledge is a curated index of Winlink references; it is
not itself a machine API specification. Winlink describes RMS (Radio Message
Server) gateways as stations that bridge client stations to one or more CMS
(Common Message Server) systems. An RMS gateway can expose one or more
radio/channel combinations. Therefore the design counts unique gateway
stations separately from protocol/frequency channel records.

The Winlink Live System Information page distinguishes gateway report freshness
from actual client reachability: green means reporting normally, yellow means
more than two hours since report, red means more than four hours, and stations
disappear from the display after six hours without a report. These are public
website presentation thresholds, not an assurance that an RF path will work
or that an offline radio channel has been detected. The site also says its
traffic information is a generalized indication, not an exact count of message
traffic. Service codes are access categories; private service-code lists are
not distributed by the Winlink Team. ([Live System Information](https://winlink.org/RMSChannels))

Winlink Express's documented channel selector updates its available list via
the Internet and uses the configured station locator to find nearby stations.
The Quick Setup guide recommends regular refresh (at least monthly in that
guide), and the Winlink FAQ says channel lists can be updated through the
client. This establishes a maintained client inventory, but the public
documentation reviewed here does not define a universal supported API for
third-party programs to read Winlink Express's private local state or to
automate its UI. ([Winlink FAQ, question 112](https://winlink.org/content/winlink_faq_march_11_2023),
[Winlink Express Quick Setup Guide](https://winlink.org/sites/default/files/RMSE_FORMS/quick_setup_guide_for_winlink_sound_card_packet_for_vhf-uhf_on_windows_v1.3.pdf))

The glossary defines a MID as a message identifier unique to each message
flowing into or out of Winlink. This can support event deduplication where a
client adapter exposes it. A client/session result, CMS acceptance, and
end-recipient delivery are different facts; the tracker must report only the
strongest outcome the integration can actually observe. ([Winlink Glossary](https://winlink.org/glossary))

## 3. What “number of links” means in Linkwise

The user-facing inventory summary must label each count and its timestamp. It
must never display one ambiguous total called “links.” The minimum counters
are:

| Counter | Counts | Does not mean |
| --- | --- | --- |
| `known_gateways` | Unique RMS station identities in the current imported inventory | RF reachable gateways |
| `known_channels` | Unique gateway + protocol + channel/frequency records | Unique gateway stations or concurrent links |
| `eligible_channels` | Channels matching station profile, service access, protocol, band, and policy | Channels that will connect |
| `freshly_reported_gateways` | Unique gateways with source report age in configured fresh band | A verified open RF path |
| `stale_or_unreported_gateways` | Gateways whose source report exceeds freshness policy | Definitively powered-off gateways |
| `attempted_sessions` | Distinct radio-session attempts in a selected time window | Successful connections |
| `connected_sessions` | Attempts whose modem/client confirms a radio link | Message delivery |
| `transfers_accepted` | Message transfers the client confirms accepted by its next Winlink stage | End-recipient receipt unless explicitly confirmed |
| `pending_outbox_count` | Queued outbound messages visible through the supported client adapter | Network links or transmitted messages |

For each summary include `as_of`, `inventory_snapshot_id`, `source_updated_at`,
`source_report_age_buckets`, selected time window, configured filters, data
coverage, and partial/stale warnings. Count channels and gateways separately;
the same gateway may contribute multiple supported channels. Counts are
partitioned by protocol, frequency band, service code, and eligibility so the
AI can answer questions such as “how many eligible 10 m VARA RMS channels are
known?” without implying all are simultaneously active.

## 4. Data model and persistence

The link manager keeps normalized local records rather than relying on the
model's conversation history:

- **Gateway:** stable callsign/endpoint identity, location/grid when present,
  service code, source identifiers, and last reported status/time.
- **GatewayChannel:** parent gateway, protocol/modem, center and/or dial
  frequency as supplied, bandwidth/mode, service eligibility, source/version,
  and observed/ingested times. Preserve source frequency semantics; do not
  silently convert dial and center frequencies.
- **InventorySnapshot:** source, query filters, retrieval time, source update
  time, counts, validation warnings, and a content hash for idempotent ingest.
- **LinkOpportunity:** derived station-specific eligibility and rank for one
  channel, with exclusion reasons and evidence/freshness references.
- **SessionAttempt:** unique local session ID, gateway/channel, modem/client,
  start/end, state transitions, PTT/keying summary, connect result, failure
  reason, and policy correlation ID.
- **TransferEvent:** opaque message MID if available, direction, bytes/status,
  client-reported outcome stage, and event time. Do not store message bodies
  or recipient data for link optimization.
- **QueueSnapshot:** aggregate pending counts/bytes by direction/priority if
  the selected client has a supported interface; never infer queue state from
  gateway inventory.

Persist append-only snapshots and events, deduplicate inventory by source
identity and transfer events by client event ID/MID plus event type, and derive
rolling summaries from the records. Retain failed attempts as well as
successful ones to avoid survivorship bias. If an adapter cannot expose a
field, return `unsupported` rather than zero.

## 5. Synchronization and monitoring architecture

Add a **Winlink Inventory Service** and **Link History/Metric Aggregator** to
the Linkwise Link Manager. The inventory service uses a provider interface:

1. Prefer a documented Winlink channel-list feed or supported client adapter.
2. Support explicit user import of a client-exported list when no supported
   integration exists.
3. Use public RMS List/status pages for operator reference and freshness
   metadata only when their interface and terms permit; do not build a
   correctness-critical integration on undocumented HTML scraping.
4. Preserve source and retrieval timestamps, validate service-code visibility,
   schema/version, frequency, mode, and station identity, then ingest
   idempotently.

The public FAQ describes updating the Winlink Express channel list through
`Open Session → Channel Selection → Update Table Via Internet`. Linkwise should
not assume that this workflow exposes a programmatic API. Client-specific
adapters (for example, a documented Pat interface, if available) must declare
capabilities for inventory, queue summary, session start, and outcome events;
unsupported functions remain disabled rather than falling back to fragile GUI
automation.

A local scheduler refreshes according to configurable source policy, and
forces a refresh before a new attempt if the snapshot is stale. A session
supervisor records connect/disconnect/PTT and transfer events in real time.
Meaningful inventory or session changes update the local summary and can notify
the AI host; if the host is not subscribed, the agent calls summary/history
tools when prompted or when a scheduled agent turn runs. Fast PTT watchdogs,
timeouts, and safety transitions remain in deterministic local code and never
depend on repeated LLM polling.

## 6. MCP tools and resources

Proposed read tools:

| Tool | Purpose |
| --- | --- |
| `winlink_refresh_inventory` | Refresh configured source(s), return snapshot ID, count delta, source time, and warnings; no radio control |
| `winlink_get_inventory_summary` | Return the labeled gateway/channel/eligibility/report-age counters for filters and an `as_of` time |
| `winlink_get_gateway_channels` | Show all imported channels for one gateway, with source and freshness |
| `link_get_history` | Query session attempts and transfer outcomes by time window, gateway, channel, or result |
| `link_get_metrics` | Return counts/rates for attempted, connected, failed, and client-confirmed transfer outcomes, with denominator and coverage |
| `winlink_get_queue_summary` | Return aggregate queue counts/bytes only if a configured client adapter supports it |

Resources may expose `linkwise://winlink/inventory`,
`linkwise://winlink/queue-summary`, and `linkwise://links/metrics`. Resource
data is a snapshot; tools revalidate it when taking an action. The queue tool
must disclose when it is unavailable and must not read or expose message
content.

All mutating workflow tools retain the existing policy boundaries:
`link_prepare_candidate`, request session authorization, `link_start_session`,
PTT-permit/keying tools, and `link_abort_session`. An inventory count alone
never authorizes tuning, PTT, or transmission. The agent's routine loop is:

```text
refresh/incrementally ingest inventory
  → count unique gateways and channels
  → filter by policy, mode, band and service access
  → join freshness and propagation evidence
  → rank eligible opportunities
  → prepare/authorize one selected session
  → record connection and transfer events
  → update summaries and report count deltas
```

## 7. Example summary response

```json
{
  "as_of_utc": "2026-11-01T18:00:00Z",
  "snapshot_id": "snapshot-uuid",
  "source": "configured-winlink-provider",
  "source_updated_at_utc": "2026-11-01T17:45:00Z",
  "filters": {"protocol": "VARA-HF", "band": "10m", "service_codes": ["PUBLIC"]},
  "counts": {
    "known_gateways": 42,
    "known_channels": 61,
    "eligible_channels": 18,
    "freshly_reported_gateways": 35,
    "stale_or_unreported_gateways": 7
  },
  "warnings": ["Reported status is not a live RF reachability test"]
}
```

Counts are illustrative only. A zero eligible count means no matching record
in the imported data under those filters, not proof that no usable station
exists.

## 8. Requirements and acceptance checks

The implementation should satisfy at least these observable properties:

1. Several channels belonging to one station increment `known_gateways` once
   and `known_channels` once per distinct channel.
2. Reimporting the same snapshot is idempotent and does not inflate totals.
3. Service-code filtering never leaks private-list stations to an unauthorized
   profile.
4. Website report-age category, inventory age, and last successful RF session
   appear as separate fields and can disagree without data loss.
5. Session failure, successful modem connection, transfer acceptance, and
   recipient delivery are distinct outcomes; unsupported outcomes stay
   unknown.
6. Duplicate event ingestion does not increment attempt/transfer metrics.
7. Summary metrics include numerator, denominator, time window, and source
   coverage; empty or incomplete history is not presented as a zero failure
   rate.
8. An inventory refresh or new high-ranking candidate never changes radio
   state or starts PTT by itself.
9. Agent/tool disconnection does not stop inventory/event persistence,
   watchdog release, or local abort behavior.

## 9. Research references and interpretation

- [Winlink Book of Knowledge](https://winlink.org/content/winlink_book_knowledge) — documentation index, not an API contract.
- [Live System Information / RMS Channels](https://winlink.org/RMSChannels) — public RMS map/list, report-age display, service-code visibility, and explicit caveat that traffic is not an exact message count.
- [Winlink FAQ (March 2023), Q112/Q11570](https://winlink.org/content/winlink_faq_march_11_2023) — documented client channel refresh and client-side channel availability/update guidance.
- [Winlink Express Quick Setup Guide](https://winlink.org/sites/default/files/RMSE_FORMS/quick_setup_guide_for_winlink_sound_card_packet_for_vhf-uhf_on_windows_v1.3.pdf) — locator/service-code setup, channel selection, and session workflow; this guide is specific to sound-card Packet on Windows.
- [Winlink Glossary](https://winlink.org/glossary) — RMS/CMS terminology and MID definition.
- [Overview of the Winlink Hybrid Network](https://winlink.org/content/overview_winlink_hybrid_network) — user-facing description of Winlink transfer modes and hybrid forwarding.
