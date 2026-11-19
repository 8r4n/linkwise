# Linkwise Modem Robustness Briefing

Narration transcript

Based on the Linkwise Modem Robustness Test Plan, revision 0.11, and the Linkwise research lab notes.

## Study overview · 00:00

The Linkwise modem study has two goals: improve recovery when high frequency signals are weak or interfered with, and preserve interoperability with stock VARA.

I’ll cover the improvements we’ve made, what the experiments establish, and how revision 0.11 of the test plan guides the next round of work.

## Experimental method · 00:24

We use KiwiSDR recordings to capture real variation in propagation, receiver installations, and interference.

On the bench, we can give each modem the same audio and vary one impairment at a time.

We score three outcomes separately: finding the burst, recovering every expected byte, and delivering those bytes in a complete session.

On off-air recordings, the decibel scale compares recorded burst power with the noise we add. Calibrated RF sensitivity requires a known wanted signal and a noise measurement.

## Implemented advances · 01:01

The first fix corrected how Linkwise unpacks the waveform’s seven-bit characters. Across eleven matched receptions, Linkwise and stock VARA produced identical KISS payloads.

The second change lowered the level-two acquisition threshold from 0.35 to 0.20. The decoder still checks the frame CRC before accepting a result.

On one retained recording set at minus eight decibels relative to added noise, exact recovery rose from three out of twenty trials to fifteen out of twenty.

## Interference experiment · 01:38

The experimental receiver tries ordinary decoding first. If that fails and the receiver detects a persistent tone, it reduces the influence of the affected tones.

The fallback requires the same CRC-valid bits on two consecutive full turbo decoder iterations.

On the later holdout, recovery under injected continuous-wave interference rose from eight out of sixty to sixty out of sixty.

Those trials reused six clips representing three distinct payloads. The candidate preserved all fourteen natural receptions and every baseline success in the clean, Gaussian noise, and echo tests.

## Acceptance and interoperability · 02:21

In one hundred new test windows with genuine pilots and erased data, the original interference gate accepted one CRC-valid body. The stricter rule accepted zero.

Stock VARA recovered all six later clean replay clips. It recovered five out of six in an earlier batch, with one miss still unexplained.

Ordinary mixed transfers delivered exact bytes with Linkwise as caller and responder. The open question is whether those sessions still complete under interference when the new level-two fallback actually runs.

## First qualification campaign · 02:57

The paper turns that question into procedure CW-01. Start with fresh, stock-generated level-two audio and independently known binary payloads.

Verify the live interference injection and the level-two telemetry. Then pair baseline and candidate sessions with the same payload, impairment assignment, and modem role.

Score exact ordered delivery, disconnect, and successful reconnect. In each role, the lower confidence bound on the gain under interference must exceed five percentage points.

Separate clean and Gaussian noise tests guard against regressions. The release decision also requires observed fallback use, tests for false acceptance, and real-time performance on the target host.

## Broader test roadmap · 03:47

The plan maps thirty-eight failure mechanisms to experiments, from fading and Doppler through clipping, sample loss, and session faults.

After the interference campaign, the calibrated ITU reference channel will support a recovery curve versus in-band signal-to-noise ratio.

The KiwiSDR field campaign will use known transmissions to measure how those bench findings transfer to real paths.

A separate VOACAP study will compare held-out predictions against a band-and-hour baseline. It will report receiver availability alongside decoding outcomes.

## Engineering decision · 04:28

The evidence supports the payload and acquisition fixes. Interference weighting is ready for the next qualification campaign.

The immediate milestone is a repeatable level-two test in both stock VARA roles, with traceable bytes and verified fallback activation.

That test will help determine whether the candidate is ready for production.
