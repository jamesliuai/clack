use std::fs::{File, OpenOptions};
use std::io::{self, Write};

use crossterm::cursor::{Hide, SetCursorStyle, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CaretStyle {
    #[default]
    Bar,
    Block,
    Underline,
}

#[derive(Debug, Clone, Copy)]
pub struct SessionOptions {
    pub enhanced_keyboard: bool,
    pub focus_reporting: bool,
    pub caret: CaretStyle,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            enhanced_keyboard: false,
            focus_reporting: true,
            caret: CaretStyle::Bar,
        }
    }
}

/// A controlling-terminal output channel independent of stdin/stdout redirection.
pub fn controlling_terminal() -> io::Result<File> {
    #[cfg(unix)]
    let path = "/dev/tty";
    #[cfg(windows)]
    let path = "CONOUT$";
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("a suitable interactive controlling terminal is required: {error}"),
            )
        })
}

/// Geometry uses the backend's operating-system terminal-size call, not an
/// escape-response capability query.
pub fn terminal_size() -> io::Result<(u16, u16)> {
    #[cfg(unix)]
    {
        let size = crossterm::terminal::window_size()?;
        Ok((size.columns, size.rows))
    }
    #[cfg(windows)]
    crossterm::terminal::size()
}

/// Create this guard before the reader and renderer. Stop/drop the reader before
/// restoring the session, including on unwind. The reader never writes output.
pub struct Session {
    terminal: File,
    options: SessionOptions,
    raw: bool,
    alternate: bool,
    cursor: bool,
    style: bool,
    paste: bool,
    focus: bool,
    enhanced: bool,
}

impl Session {
    pub fn enter(options: SessionOptions) -> io::Result<Self> {
        let terminal = controlling_terminal()?;
        // The signal-only self-pipe is adopted by the reader later; installing
        // it here protects the entire raw-mode setup interval without reading
        // keyboard input or creating another worker.
        #[cfg(unix)]
        crossterm::event::prepare_signals()?;
        // Validate the independently acquired console input before touching modes.
        #[cfg(windows)]
        let _input = OpenOptions::new().read(true).write(true).open("CONIN$")?;
        let mut session = Self {
            terminal,
            options,
            raw: false,
            alternate: false,
            cursor: false,
            style: false,
            paste: false,
            focus: false,
            enhanced: false,
        };
        session.activate()?;
        Ok(session)
    }

    fn activate(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        self.raw = true;
        // A failed write/flush may have emitted a prefix. Track each attempted
        // capability before writing so error cleanup also reverses that case.
        self.alternate = true;
        execute!(self.terminal, EnterAlternateScreen)?;
        self.cursor = true;
        execute!(self.terminal, Show)?;
        self.set_caret(self.options.caret)?;
        self.paste = true;
        execute!(self.terminal, EnableBracketedPaste)?;
        if self.options.focus_reporting {
            self.focus = true;
            execute!(self.terminal, EnableFocusChange)?;
        }
        if self.options.enhanced_keyboard {
            self.set_enhanced_keyboard(true)?;
        }
        self.terminal.flush()
    }

    pub fn writer(&self) -> io::Result<File> {
        self.terminal.try_clone()
    }

    /// Apply an explicit preference while input is disarmed. No capability
    /// query is sent. Resume options change only after successful output.
    pub fn set_enhanced_keyboard(&mut self, enabled: bool) -> io::Result<()> {
        if enabled == self.enhanced {
            self.options.enhanced_keyboard = enabled;
            return Ok(());
        }
        if enabled {
            self.enhanced = true;
            if let Err(error) = execute!(
                self.terminal,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_ASSOCIATED_TEXT,
                )
            ) {
                // A partial write may have changed the terminal. Compensate
                // when possible; otherwise retain cleanup responsibility.
                if execute!(self.terminal, PopKeyboardEnhancementFlags).is_ok() {
                    self.enhanced = false;
                }
                return Err(error);
            }
        } else {
            // On failure keep the old preference and pending-pop obligation.
            execute!(self.terminal, PopKeyboardEnhancementFlags)?;
            self.enhanced = false;
        }
        self.options.enhanced_keyboard = enabled;
        Ok(())
    }

    pub fn set_caret(&mut self, caret: CaretStyle) -> io::Result<()> {
        let style = match caret {
            CaretStyle::Bar => SetCursorStyle::SteadyBar,
            CaretStyle::Block => SetCursorStyle::SteadyBlock,
            CaretStyle::Underline => SetCursorStyle::SteadyUnderScore,
        };
        self.style = true;
        execute!(self.terminal, style)?;
        self.options.caret = caret;
        Ok(())
    }

    pub fn show_cursor(&mut self, visible: bool) -> io::Result<()> {
        self.cursor = true;
        if visible {
            execute!(self.terminal, Show)?;
        } else {
            execute!(self.terminal, Hide)?;
        }
        Ok(())
    }

    /// Restore all successfully changed capabilities, attempting later cleanup
    /// even after an earlier operation fails. No terminal response is awaited.
    pub fn restore(&mut self) -> io::Result<()> {
        let mut first_error = None;
        macro_rules! restore {
            ($field:ident, $operation:expr) => {
                if self.$field {
                    if let Err(error) = $operation {
                        first_error.get_or_insert(error);
                    }
                    self.$field = false;
                }
            };
        }
        restore!(
            enhanced,
            execute!(self.terminal, PopKeyboardEnhancementFlags)
        );
        restore!(focus, execute!(self.terminal, DisableFocusChange));
        restore!(paste, execute!(self.terminal, DisableBracketedPaste));
        // Prior cursor shape is not queried on the normal startup path. Reset to
        // the terminal user's default, as required when it is not known.
        restore!(
            style,
            execute!(self.terminal, SetCursorStyle::DefaultUserShape)
        );
        restore!(cursor, execute!(self.terminal, Show));
        restore!(alternate, execute!(self.terminal, LeaveAlternateScreen));
        restore!(raw, disable_raw_mode());
        if let Err(error) = self.terminal.flush() {
            first_error.get_or_insert(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Call after the reader has been stopped and the active run interrupted.
    /// SIGSTOP is intentional here: SIGTSTP remains safely owned by the reader's
    /// signal source. Returning after SIGCONT reestablishes the session.
    #[cfg(unix)]
    pub fn suspend(&mut self) -> io::Result<()> {
        // Self-directed SIGSTOP delivery can be deferred until a thread returns
        // to the kernel. Wait for an explicit continuation before reactivating
        // modes, rather than assuming kill() resumes only after the stop.
        let mut continuation = signal_hook::iterator::Signals::new([signal_hook::consts::SIGCONT])?;
        self.restore()?;
        nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGSTOP)
            .map_err(io::Error::from)?;
        continuation.forever().next();
        self.activate()
    }

    #[cfg(windows)]
    pub fn suspend(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "console suspension is not supported on Windows",
        ))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
