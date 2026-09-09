//! Explicit, bounded doctor probing. The ordinary application never calls this.
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ProbeReport {
    pub status: &'static str,
    pub elapsed_ms: u64,
    pub keyboard_enhancement_support: Option<bool>,
    /// Currently active protocol flags, not a claim that every flag is supported.
    pub keyboard_enhancement_flags: Option<u8>,
    pub primary_device_attributes_received: bool,
    pub cursor_style: Option<u8>,
    pub interrupted: bool,
}

/// Requires exclusive ownership of the terminal event reader. Run only from
/// explicit `doctor --probe`, never while an interactive Reader exists.
pub fn probe_capabilities() -> Result<ProbeReport, String> {
    #[cfg(unix)]
    {
        unix_probe()
    }
    #[cfg(windows)]
    {
        Ok(ProbeReport {
            status: "unsupported_backend",
            elapsed_ms: 0,
            keyboard_enhancement_support: None,
            keyboard_enhancement_flags: None,
            primary_device_attributes_received: false,
            cursor_style: None,
            interrupted: false,
        })
    }
}

#[cfg(unix)]
fn unix_probe() -> Result<ProbeReport, String> {
    use crossterm::event::{
        Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardProbeEvent, poll_keyboard_probe,
    };
    use std::{
        io::Write,
        time::{Duration, Instant},
    };

    let mut terminal = super::controlling_terminal().map_err(|error| error.to_string())?;
    // A diagnostic query must also fail promptly if terminal output is blocked.
    // This file description is independently opened, so its flags do not alter
    // the event reader's terminal descriptor.
    let flags = nix::fcntl::fcntl(&terminal, nix::fcntl::FcntlArg::F_GETFL)
        .map(nix::fcntl::OFlag::from_bits_truncate)
        .map_err(|_| "cannot configure bounded capability probe output".to_owned())?;
    nix::fcntl::fcntl(
        &terminal,
        nix::fcntl::FcntlArg::F_SETFL(flags | nix::fcntl::OFlag::O_NONBLOCK),
    )
    .map_err(|_| "cannot configure bounded capability probe output".to_owned())?;
    crossterm::event::prepare_signals()
        .map_err(|_| "cannot prepare safe probe signals".to_owned())?;
    let mut raw =
        ProbeRaw::enter().map_err(|_| "cannot enter raw mode for capability probe".to_owned())?;
    let started = Instant::now();
    let deadline = started + Duration::from_millis(400);
    let result = (|| {
        crossterm::event::discard_pending_input()
            .map_err(|_| "cannot initialize capability probe input".to_owned())?;
        terminal
            .write_all(b"\x1b[?u\x1b[c")
            .and_then(|()| terminal.flush())
            .map_err(|_| "cannot write capability probe to controlling terminal".to_owned())?;
        let mut report = ProbeReport {
            status: "timeout",
            elapsed_ms: 0,
            keyboard_enhancement_support: None,
            keyboard_enhancement_flags: None,
            primary_device_attributes_received: false,
            cursor_style: None,
            interrupted: false,
        };
        let mut events = 0_usize;
        while Instant::now() < deadline {
            let event = poll_keyboard_probe(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| "malformed or unavailable capability probe response".to_owned())?;
            let Some(event) = event else {
                continue;
            };
            events += 1;
            if events > 4096 {
                return Err("capability probe input limit exceeded".into());
            }
            match event {
                KeyboardProbeEvent::Flags(flags) => {
                    report.keyboard_enhancement_support = Some(true);
                    report.keyboard_enhancement_flags = Some(flags.bits());
                    report.status = "supported";
                }
                KeyboardProbeEvent::PrimaryDeviceAttributes => {
                    report.primary_device_attributes_received = true;
                    if report.keyboard_enhancement_support.is_none() {
                        report.keyboard_enhancement_support = Some(false);
                        report.status = "unsupported";
                    }
                    break;
                }
                KeyboardProbeEvent::Input(Event::Signal(
                    signal_hook::consts::SIGINT
                    | signal_hook::consts::SIGTERM
                    | signal_hook::consts::SIGHUP
                    | signal_hook::consts::SIGTSTP,
                )) => {
                    report.interrupted = true;
                    report.status = "interrupted";
                    break;
                }
                KeyboardProbeEvent::Input(event) => {
                    if let Some(key) = event.as_key_event()
                        && key.kind != KeyEventKind::Release
                        && (key.code == KeyCode::Esc
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers == KeyModifiers::CONTROL))
                    {
                        report.interrupted = true;
                        report.status = "interrupted";
                        break;
                    }
                }
                KeyboardProbeEvent::Other => {}
            }
        }
        report.elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        Ok(report)
    })();
    // Restore before exposing either diagnostic output or an error to main.
    raw.restore()
        .map_err(|_| "cannot restore terminal after capability probe".to_owned())?;
    result
}

#[cfg(unix)]
struct ProbeRaw(bool);

#[cfg(unix)]
impl ProbeRaw {
    fn enter() -> std::io::Result<Self> {
        if crossterm::terminal::is_raw_mode_enabled()? {
            return Ok(Self(false));
        }
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self(true))
    }

    fn restore(&mut self) -> std::io::Result<()> {
        if self.0 {
            crossterm::terminal::disable_raw_mode()?;
            self.0 = false;
        }
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for ProbeRaw {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
