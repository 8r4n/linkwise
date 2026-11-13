use std::{ffi::OsString, path::Path, time::Duration};

use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
    service_dispatcher,
};

const SERVICE: &str = "LinkwiseModem";
define_windows_service!(service_main, run);

pub fn dispatch() -> windows_service::Result<()> {
    service_dispatcher::start(SERVICE, service_main)
}

fn report(
    handle: &ServiceStatusHandle,
    state: ServiceState,
    code: u32,
) -> windows_service::Result<()> {
    handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: ServiceExitCode::Win32(code),
        checkpoint: u32::from(matches!(
            state,
            ServiceState::StartPending | ServiceState::StopPending
        )),
        wait_hint: if matches!(
            state,
            ServiceState::StartPending | ServiceState::StopPending
        ) {
            Duration::from_secs(10)
        } else {
            Duration::ZERO
        },
        process_id: None,
    })
}

fn run(_: Vec<OsString>) {
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let Ok(handle) = service_control_handler::register(SERVICE, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop.send_replace(true);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    }) else {
        return;
    };
    let result = (|| -> Result<(), String> {
        report(&handle, ServiceState::StartPending, 0).map_err(|error| error.to_string())?;
        let path = std::env::args_os()
            .nth(2)
            .ok_or("station config path is required")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let path = Path::new(&path);
        let station = super::setup::Station::load(path)?;
        let stop = async {
            let _ = stopped.wait_for(|value| *value).await;
            let _ = report(&handle, ServiceState::StopPending, 0);
        };
        let running = || report(&handle, ServiceState::Running, 0);
        let result = if let Some(audio_port) = station.winlink_test_audio_port {
            let profile = station.profile_path(path).ok_or("Missing BW500 profile")?;
            let (notify, stopping) = tokio::sync::watch::channel(false);
            runtime.block_on(super::station::shutdown(
                async { stop.await; Ok(()) },
                notify,
                async {
                    super::virtual_host::serve_winlink_test(
                        &station.callsign,
                        &profile,
                        (station.command_port, station.data_port, audio_port),
                        stopping,
                        || {
                            running().map_err(std::io::Error::other)?;
                            log_event(&format!(
                                "BW500 test mode on 127.0.0.1: command {}, DATA {}, waveform {}. Waiting for a waveform peer.",
                                station.command_port, station.data_port, audio_port,
                            ), false);
                            Ok(())
                        },
                    ).await.map_err(|error| error.to_string())
                },
            ))
        } else {
            runtime.block_on(super::station::serve(path, stop, || {
                running().map_err(|error| error.to_string())
            }))
        };
        runtime.shutdown_timeout(Duration::from_millis(250));
        result
    })();
    if let Err(error) = result {
        log_event(&error, true);
        // SCM records the nonzero service exit and applies its failure policy.
        eprintln!("Linkwise station: {error}");
        let _ = report(&handle, ServiceState::Stopped, 1064);
    } else {
        let _ = report(&handle, ServiceState::Stopped, 0);
    }
}

pub(super) fn log_event(message: &str, error: bool) {
    use windows_sys::Win32::System::EventLog::{
        DeregisterEventSource, EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE,
        RegisterEventSourceW, ReportEventW,
    };
    let source: Vec<u16> = SERVICE.encode_utf16().chain([0]).collect();
    let message: Vec<u16> = message.encode_utf16().chain([0]).collect();
    let strings = [message.as_ptr()];
    // The buffers are nul-terminated and remain live until ReportEventW returns.
    unsafe {
        let log = RegisterEventSourceW(std::ptr::null(), source.as_ptr());
        if !log.is_null() {
            let kind = if error {
                EVENTLOG_ERROR_TYPE
            } else {
                EVENTLOG_INFORMATION_TYPE
            };
            ReportEventW(
                log,
                kind,
                0,
                1,
                std::ptr::null_mut(),
                1,
                0,
                strings.as_ptr(),
                std::ptr::null(),
            );
            DeregisterEventSource(log);
        }
    }
}
