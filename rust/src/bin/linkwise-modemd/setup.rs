use linkwise::modem::{
    bw500_waveform::Waveform,
    setup::{LinkSetup, Profile},
};
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Station {
    version: u32,
    pub(super) callsign: String,
    pub(super) input: AudioDevice,
    pub(super) output: AudioDevice,
    pub(super) bandwidth: Option<u16>,
    #[serde(default = "default_command_port")]
    pub(super) command_port: u16,
    #[serde(default = "default_data_port")]
    pub(super) data_port: u16,
    bw500_profile: Option<PathBuf>,
    #[cfg(windows)]
    pub(super) winlink_test_audio_port: Option<u16>,
    #[cfg(windows)]
    pub(super) verify_bw500_fixture: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AudioDevice {
    pub(super) name: String,
    pub(super) channel: u16,
}

fn default_command_port() -> u16 {
    8300
}

fn default_data_port() -> u16 {
    8301
}

pub(super) fn read_text(path: &Path, max_bytes: u64) -> Result<String, String> {
    let mut content = String::new();
    File::open(path)
        .and_then(|file| file.take(max_bytes + 1).read_to_string(&mut content))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if content.len() as u64 > max_bytes {
        return Err(format!("{} exceeds {max_bytes} bytes", path.display()));
    }
    Ok(content)
}

impl Station {
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let content = read_text(path, 16 * 1024)?;
        let mut station: Self = serde_json::from_str(&content)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if station.version != 1 {
            return Err(format!(
                "Unsupported configuration version {}",
                station.version
            ));
        }
        station.callsign = station.callsign.trim().to_ascii_uppercase();
        let setup = LinkSetup {
            caller: station.callsign.clone(),
            requested_level: 4,
            high_speed: false,
        };
        if setup.encode(Profile::Bw500).is_none() {
            return Err("Enter a valid BW500 station callsign".into());
        }
        if station.input.name.trim().is_empty() || station.output.name.trim().is_empty() {
            return Err("Enter the exact input and output names shown by --audio-devices".into());
        }
        if station.command_port == 0
            || station.data_port == 0
            || station.command_port == station.data_port
        {
            return Err("Choose distinct, nonzero command and data ports".into());
        }
        if let Some(bandwidth) = station.bandwidth {
            if !matches!(bandwidth, 500 | 2300 | 2750) {
                return Err("Choose station bandwidth 500, 2300, or 2750 Hz".into());
            }
            if bandwidth == 500 && station.bw500_profile.is_none() {
                return Err("Two-way BW500 requires bw500_profile".into());
            }
        }
        #[cfg(windows)]
        if let Some(port) = station.winlink_test_audio_port {
            if station.bandwidth.is_some() {
                return Err("Choose either native bandwidth or winlink_test_audio_port".into());
            }
            if station.command_port.checked_add(1) != Some(station.data_port) {
                return Err(
                    "Winlink Express requires the DATA port to follow the command port".into(),
                );
            }
            if port == 0 || port == station.command_port || port == station.data_port {
                return Err("Winlink test audio port must be nonzero and distinct".into());
            }
            if station.bw500_profile.is_none() {
                return Err("Winlink test mode requires bw500_profile".into());
            }
        }
        Ok(station)
    }

    pub(super) fn profile_path(&self, station_path: &Path) -> Option<PathBuf> {
        self.bw500_profile
            .as_ref()
            .map(|profile| station_path.parent().unwrap_or(Path::new("")).join(profile))
    }

    pub(super) fn waveform(&self, path: &Path) -> Result<Option<Waveform>, String> {
        self.profile_path(path)
            .map(|profile| {
                let content = read_text(&profile, 128 * 1024)?;
                Waveform::from_json(&content)
                    .map_err(|error| format!("Invalid profile {}: {error}", profile.display()))
            })
            .transpose()
    }
}

pub fn check_config(file: &str) -> Result<(), String> {
    let path = Path::new(file);
    let station = Station::load(path)?;
    station.waveform(path)?;
    println!("Configuration valid for {}.", station.callsign);
    Ok(())
}

pub async fn check_station(file: &str) -> Result<(), String> {
    let path = Path::new(file);
    let station = Station::load(path)?;
    station.waveform(path)?;
    check_audio(&station.input, &station.output)?;

    let command = SocketAddr::from((Ipv4Addr::LOCALHOST, station.command_port));
    let data = SocketAddr::from((Ipv4Addr::LOCALHOST, station.data_port));
    let _listener = linkwise::host::tcp::Listener::bind(command, data, Duration::from_secs(5))
        .await
        .map_err(|error| format!(
            "Could not reserve command port {command} and data port {data}: {error}. Close any modem using these ports or choose a different pair."
        ))?;
    println!("Station checks passed for {}.", station.callsign);
    println!("Input capture and output format checked at 48 kHz.");
    println!("Local modem ports available during check: command {command}, data {data}.");
    Ok(())
}

#[cfg(feature = "audio-device")]
pub fn list_audio_devices() -> Result<(), String> {
    use linkwise::audio::{device, playback};

    let inputs = device::input_devices().map_err(|error| format!("List audio inputs: {error}"))?;
    let outputs =
        playback::output_devices().map_err(|error| format!("List audio outputs: {error}"))?;
    println!("Input devices:");
    for name in inputs {
        println!("  {name}");
    }
    println!("Output devices:");
    for name in outputs {
        println!("  {name}");
    }
    Ok(())
}

#[cfg(feature = "audio-device")]
fn check_audio(input: &AudioDevice, output: &AudioDevice) -> Result<(), String> {
    use linkwise::audio::{device::Capture, playback};

    let capture = Capture::start(Some(&input.name), input.channel).map_err(|error| {
        format!(
            "Open input {:?} channel {}: {error}",
            input.name, input.channel
        )
    })?;
    if capture
        .read(Duration::from_secs(2))
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err(format!(
            "No capture samples arrived from {:?} within two seconds",
            input.name
        ));
    }
    drop(capture);
    playback::check_output(&output.name, output.channel).map_err(|error| {
        format!(
            "Check output {:?} channel {}: {error}",
            output.name, output.channel
        )
    })
}

#[cfg(not(feature = "audio-device"))]
pub fn list_audio_devices() -> Result<(), String> {
    Err("Use a Linkwise build with the audio-device feature".into())
}

#[cfg(not(feature = "audio-device"))]
fn check_audio(input: &AudioDevice, output: &AudioDevice) -> Result<(), String> {
    Err(format!(
        "Use a Linkwise build with the audio-device feature to check input channel {} and output channel {}",
        input.channel, output.channel
    ))
}
