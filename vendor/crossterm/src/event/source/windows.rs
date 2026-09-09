use std::time::Duration;

use crossterm_winapi::{Console, Handle, InputRecord};

use crate::event::{
    sys::windows::{parse::MouseButtonsPressed, poll::WinApiPoll},
    Event,
};

#[cfg(any(feature = "event-stream", feature = "poll-waker"))]
use crate::event::sys::Waker;
use crate::event::{
    source::EventSource,
    sys::windows::parse::{handle_key_event, handle_mouse_event},
    timeout::PollTimeout,
    InternalEvent,
};

pub(crate) struct WindowsEventSource {
    console: Console,
    poll: WinApiPoll,
    surrogate_buffer: Option<u16>,
    mouse_buttons_pressed: MouseButtonsPressed,
    pending_repeat: Option<(Event, u16)>,
    #[cfg(feature = "poll-waker")]
    pending_resize: std::collections::VecDeque<Event>,
}

impl WindowsEventSource {
    pub(crate) fn new() -> std::io::Result<WindowsEventSource> {
        let console = Console::from(Handle::current_in_handle()?);
        Ok(WindowsEventSource {
            console,

            poll: WinApiPoll::new()?,

            surrogate_buffer: None,
            mouse_buttons_pressed: MouseButtonsPressed::default(),
            pending_repeat: None,
            #[cfg(feature = "poll-waker")]
            pending_resize: std::collections::VecDeque::new(),
        })
    }
}

impl EventSource for WindowsEventSource {
    fn try_read(&mut self, timeout: Option<Duration>) -> std::io::Result<Option<InternalEvent>> {
        #[cfg(feature = "poll-waker")]
        if let Some(event) = self.pending_resize.pop_front() { return Ok(Some(InternalEvent::Event(event))); }
        if let Some((mut event, remaining)) = self.pending_repeat.take() {
            if let Event::Key(key) = &mut event {
                if key.kind != crate::event::KeyEventKind::Release {
                    key.kind = crate::event::KeyEventKind::Repeat;
                }
            }
            if remaining > 1 { self.pending_repeat = Some((event.clone(), remaining - 1)); }
            return Ok(Some(InternalEvent::Event(event)));
        }
        let poll_timeout = PollTimeout::new(timeout);

        loop {
            if let Some(event_ready) = self.poll.poll(poll_timeout.leftover())? {
                let number = self.console.number_of_console_input_events()?;
                if event_ready && number != 0 {
                    let event = match self.console.read_single_input_event()? {
                        InputRecord::KeyEvent(record) => {
                            let repeat_count = record.repeat_count;
                            let event = handle_key_event(record, &mut self.surrogate_buffer);
                            if repeat_count > 1 {
                                if let Some(event) = &event {
                                    self.pending_repeat = Some((event.clone(), repeat_count - 1));
                                }
                            }
                            event
                        }
                        InputRecord::MouseEvent(record) => {
                            let mouse_event =
                                handle_mouse_event(record, &self.mouse_buttons_pressed);
                            self.mouse_buttons_pressed = MouseButtonsPressed {
                                left: record.button_state.left_button(),
                                right: record.button_state.right_button(),
                                middle: record.button_state.middle_button(),
                            };

                            mouse_event
                        }
                        InputRecord::WindowBufferSizeEvent(record) => {
                            // windows starts counting at 0, unix at 1, add one to replicate unix behaviour.
                            Some(Event::Resize(
                                (record.size.x as i32 + 1) as u16,
                                (record.size.y as i32 + 1) as u16,
                            ))
                        }
                        InputRecord::FocusEvent(record) => {
                            let event = if record.set_focus {
                                Event::FocusGained
                            } else {
                                Event::FocusLost
                            };
                            Some(event)
                        }
                        _ => None,
                    };

                    if let Some(event) = event {
                        return Ok(Some(InternalEvent::Event(event)));
                    }
                }
            }

            if poll_timeout.elapsed() {
                return Ok(None);
            }
        }
    }

    #[cfg(feature = "poll-waker")]
    fn discard_pending_input(&mut self) -> std::io::Result<()> {
        self.surrogate_buffer = None;
        self.pending_repeat = None;
        let pending = self.console.number_of_console_input_events()?;
        if pending > 4096 { return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock,
            "console input overflow during restart")); }
        for _ in 0..pending {
            if let InputRecord::WindowBufferSizeEvent(record) = self.console.read_single_input_event()? {
                self.pending_resize.push_back(Event::Resize((record.size.x as i32 + 1) as u16,
                    (record.size.y as i32 + 1) as u16));
            }
        }
        Ok(())
    }

    #[cfg(any(feature = "event-stream", feature = "poll-waker"))]
    fn waker(&self) -> Waker {
        self.poll.waker()
    }
}
