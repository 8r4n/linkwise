# Linkwise modem setup checks

Use these checks to prepare a station for Linkwise. The current build checks
audio and local port availability and can run isolated audio bench sessions.
Over-the-air Winlink sessions and PTT control require the operational
modem runtime.

## Select your radio's audio devices

Use an audio-enabled build of `linkwise-modemd`. On Windows, the executable
is named `linkwise-modemd.exe`.

1. Connect your radio's USB audio interface. In the radio or interface
   settings, disable VOX before connecting or changing audio connections.
2. Run `linkwise-modemd --audio-devices`. Find the radio interface in both
   lists. If several entries look alike, compare the list before and after
   connecting the interface.
3. Save the following as `station.json`. Replace the example callsign and
   device names with your own. Copy each name exactly as listed. Channel
   `0` is the first channel.

```json
{
  "version": 1,
  "callsign": "W1AW",
  "input": { "name": "Your radio input device", "channel": 0 },
  "output": { "name": "Your radio output device", "channel": 0 }
}
```

## Check the configuration and station

Run these commands from the directory containing `station.json`:

```text
linkwise-modemd --check-config station.json
linkwise-modemd --check-station station.json
```

The first command validates the file. The second opens the selected input,
waits for samples, checks the output device's advertised format, and briefly
reserves the two local modem ports. A pass confirms that these checks
succeeded. Capture can pass even if the samples are silent. The output
check does not play audio or verify audio routing to the radio.

The default loopback ports are `8300` for commands and `8301` for data.
If you use different ports, add `command_port` and `data_port` to the
configuration with two distinct port numbers. The check releases both ports
when it exits.

If you already have a Linkwise BW500 level-4 measurement profile, set
`bw500_profile` to its JSON file path to check that it can be loaded. A
relative path is resolved from the directory containing `station.json`.
The profile is optional for these setup checks.

## Start an isolated wide-mode bench

Run one Linkwise daemon per test station on a dedicated virtual audio route.
Choose one bandwidth, 2300 or 2750, for both stations. For example:

```text
linkwise-modemd --serve-virtual-wide W1AW 2300 8300 8301 8302
```

The three ports are the command, DATA, and audio endpoints. Use separate
ports for each station when both run on one computer. Attach the audio
endpoint to your virtual capture and playback devices with
`linkwise-modemd --virtual-audio-device 8302 "Virtual input" "Virtual output"`.
These sessions use level-4 frames and a peer-to-peer host connection.

## Troubleshooting

| What you see | What to check |
| --- | --- |
| Input or output device unavailable | Run `--audio-devices` again. Confirm the interface is connected and that its current name matches the name in `station.json`. |
| Input cannot be opened or no samples arrive | Check OS microphone permissions for the program launching Linkwise. Confirm that the selected device and channel support 48 kHz, then retry. |
| Output format unsupported | Check that the selected device supports 48 kHz on the specified channel. For a single-channel interface, use channel `0`. |
| Local modem ports cannot be reserved | Close the application using either port or configure a distinct available pair, then rerun the station check. |
| Profile cannot be loaded | Confirm the file exists and that the path is relative to `station.json` or absolute. Check the error for invalid profile content. |

Record the output of `linkwise-modemd --version` and the failing check
when reporting a setup problem.
