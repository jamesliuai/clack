//! The optional backend preserves text and event kind as distinct values.
//! Parser byte-sequence fixtures are in the vendored backend's unit tests.

use clack::engine::{Mode, Policy};
use clack::terminal::{InputKind, ReaderOptions, normalize_event};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
};

#[test]
fn associated_text_flag_is_the_protocol_defined_bit() {
    assert_eq!(KeyboardEnhancementFlags::REPORT_ASSOCIATED_TEXT.bits(), 16);
}

#[test]
fn associated_text_preserves_metadata_and_key_helpers() {
    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ] {
        let key = KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, kind);
        let event = Event::KeyWithText {
            key,
            text: "é 界".into(),
        };
        assert_eq!(event.as_key_event(), Some(key));
        assert_eq!(event.is_key_press(), kind == KeyEventKind::Press);
        assert_eq!(event.is_key_repeat(), kind == KeyEventKind::Repeat);
        assert_eq!(event.is_key_release(), kind == KeyEventKind::Release);
        assert_eq!(
            event.as_key_press_event().is_some(),
            kind == KeyEventKind::Press
        );
        assert_eq!(
            event.as_key_repeat_event().is_some(),
            kind == KeyEventKind::Repeat
        );
        assert_eq!(
            event.as_key_release_event().is_some(),
            kind == KeyEventKind::Release
        );
        let Event::KeyWithText { text, .. } = event else {
            unreachable!()
        };
        assert_eq!(text, "é 界");
    }
}

#[test]
fn baseline_key_events_keep_their_existing_helpers() {
    let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    let event = Event::Key(key);
    assert_eq!(event.as_key_event(), Some(key));
    assert!(event.is_key_press());
    assert!(!event.is_key_repeat());
    assert!(!event.is_key_release());
}

#[test]
fn adapter_preserves_associated_text_once_and_rejects_release() {
    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ] {
        let event = Event::KeyWithText {
            key: KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, kind),
            text: "é".into(),
        };
        let normalized = normalize_event(event).unwrap();
        if kind == KeyEventKind::Release {
            assert!(normalized.is_none());
        } else {
            let Some(InputKind::Key {
                key,
                associated_text,
            }) = normalized
            else {
                panic!("expected text event")
            };
            assert_eq!(associated_text.as_deref(), Some("é"));
            assert_eq!(key.code, KeyCode::Char('a'));
        }
    }
}

#[test]
fn atomic_paste_and_commands_never_arm_ready() {
    let options = ReaderOptions::default();
    let paste = normalize_event(Event::Paste("cat dog".into()))
        .unwrap()
        .unwrap();
    assert!(matches!(paste, InputKind::Paste));
    assert!(!paste.starts_test(&options));
    for key in [
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
    ] {
        assert!(
            !normalize_event(Event::Key(key))
                .unwrap()
                .unwrap()
                .starts_test(&options)
        );
    }
}

#[test]
fn exact_tab_and_newline_arm_at_the_same_eligibility_boundary_as_the_engine() {
    let options = ReaderOptions {
        mode: Mode::Code,
        policy: Policy::Exact,
        seconds: 30,
        ..ReaderOptions::default()
    };
    for key in [
        KeyCode::Tab,
        KeyCode::Enter,
        KeyCode::Char(' '),
        KeyCode::Char('x'),
    ] {
        let event = normalize_event(Event::Key(KeyEvent::new(key, KeyModifiers::NONE)))
            .unwrap()
            .unwrap();
        assert!(event.starts_test(&options));
    }
}

#[test]
fn associated_text_bounds_and_unsafe_controls_are_atomic_errors() {
    for text in [
        "a".repeat(129),
        format!("e{}", "\u{301}".repeat(32)),
        "private\u{1b}[31m".into(),
        "private\u{202e}text".into(),
    ] {
        let event = Event::KeyWithText {
            key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            text,
        };
        let error = normalize_event(event).expect_err("unsafe text must be rejected as one event");
        assert!(!error.contains("private"));
    }
}

#[test]
fn enhanced_control_commands_do_not_arm_but_altgr_text_does() {
    for character in ['c', 'C', 'r', 'p', 'w'] {
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ] {
            let event = normalize_event(Event::KeyWithText {
                key: KeyEvent::new(KeyCode::Char(character), modifiers),
                text: character.to_string(),
            })
            .unwrap()
            .unwrap();
            assert!(!event.starts_test(&ReaderOptions::default()));
        }
    }
    let event = normalize_event(Event::KeyWithText {
        key: KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ),
        text: "ç".into(),
    })
    .unwrap()
    .unwrap();
    assert!(event.starts_test(&ReaderOptions::default()));
}

#[test]
fn unsupported_shaping_and_invisible_input_are_rejected_before_zen_or_wrong_targets() {
    for character in [
        'ع', 'क', 'ก', 'א', '\u{00ad}', '\u{034f}', '\u{200b}', '\u{2060}', '\u{feff}',
    ] {
        assert!(
            normalize_event(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE
            )))
            .is_err()
        );
        let error = normalize_event(Event::KeyWithText {
            key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            text: format!("private{character}payload"),
        })
        .unwrap_err();
        assert!(!error.contains("private"));
        assert!(!error.contains("payload"));
    }
}

#[test]
fn supported_scalar_continuations_are_admitted_before_a_grapheme_is_complete() {
    for character in [
        'e',
        '\u{0301}',
        '\u{0323}',
        '\u{200d}',
        '\u{fe0f}',
        '\u{e0067}',
        '\u{e007f}',
        '👩',
        '💻',
        '界',
        '한',
        'Ω',
        'Ж',
    ] {
        assert!(
            normalize_event(Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE
            )))
            .unwrap()
            .is_some()
        );
        assert!(
            normalize_event(Event::KeyWithText {
                key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
                text: character.to_string(),
            })
            .unwrap()
            .is_some()
        );
    }
}

#[test]
fn configured_commands_share_ready_eligibility_with_enhanced_text_dispatch() {
    let mut config = clack::config::Config::default();
    config
        .workflow
        .bindings
        .insert("new_sample".into(), "ctrl+n".into());
    let options = ReaderOptions {
        bindings: std::sync::Arc::new(clack::settings::Bindings::from_config(&config).unwrap()),
        ..ReaderOptions::default()
    };
    let key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
    let event = normalize_event(Event::KeyWithText {
        key,
        text: "ñ".into(),
    })
    .unwrap()
    .unwrap();
    assert!(!event.starts_test(&options));
    assert!(event.starts_test(&ReaderOptions::default()));
    assert_eq!(
        options
            .bindings
            .resolve(&key, clack::settings::BindingContext::Running),
        Some(clack::settings::CommandAction::NewSample)
    );
}

// The normal parent test invokes this helper in a new process with an actual
// controlling pseudo-terminal. It does nothing in the parent test process.
#[cfg(unix)]
#[test]
fn terminal_lifecycle_child() {
    use clack::terminal::{Control, Reader, Session, SessionOptions};
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    let Ok(case) = std::env::var("CLACK_TERMINAL_TEST_CASE") else {
        return;
    };
    if case == "source" {
        let mut source = String::new();
        std::io::stdin().read_to_string(&mut source).unwrap();
        assert_eq!(source, "private source fixture");
    }
    let result = std::panic::catch_unwind(|| {
        let mut session = Session::enter(SessionOptions::default()).unwrap();
        if case == "early_signal" {
            let mut terminal = session.writer().unwrap();
            terminal.write_all(b"SIGNAL-WINDOW\r\n").unwrap();
            terminal.flush().unwrap();
            terminal.read_exact(&mut [0]).unwrap();
        }
        let mut reader = Reader::start(
            ReaderOptions {
                seconds: 1,
                ..ReaderOptions::default()
            },
            1,
            Instant::now(),
            std::thread::current(),
        )
        .unwrap();
        let mut writer = session.writer().unwrap();
        writer.write_all(b"READY\r\n").unwrap();
        writer.flush().unwrap();
        let wait = |reader: &Reader| {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(event) = reader.events.try_recv() {
                    return event;
                }
                let left = until.saturating_duration_since(Instant::now());
                assert!(!left.is_zero(), "reader event timeout");
                std::thread::park_timeout(left);
            }
        };
        let first = wait(&reader);
        match case.as_str() {
            "signal" | "early_signal" => assert!(matches!(
                first.kind,
                InputKind::Signal(signal_hook::consts::SIGTERM)
            )),
            "paste" => assert!(matches!(first.kind, InputKind::Paste)),
            "associated" => {
                let InputKind::Key {
                    key,
                    associated_text,
                } = first.kind
                else {
                    panic!("expected key with text")
                };
                assert_eq!(key.code, KeyCode::Char('a'));
                assert_eq!(associated_text.as_deref(), Some("b"));
            }
            "release" => {
                let InputKind::Key { key, .. } = first.kind else {
                    panic!("expected pressed key")
                };
                assert_eq!(key.code, KeyCode::Char('b'));
            }
            "deadline" => {
                assert!(matches!(first.kind, InputKind::Key { .. }));
                let deadline = wait(&reader);
                assert!(matches!(deadline.kind, InputKind::Tick));
                assert_eq!(deadline.received_us, first.received_us + 1_000_000);
            }
            "enhancements" => {
                reader.command(Control::Disarm { epoch: 2 }).unwrap();
                loop {
                    let event = wait(&reader);
                    if event.epoch == 2 && matches!(event.kind, InputKind::EpochReady) {
                        break;
                    }
                }
                session.set_enhanced_keyboard(true).unwrap();
                session.set_enhanced_keyboard(true).unwrap();
                session.set_enhanced_keyboard(false).unwrap();
                session.set_enhanced_keyboard(false).unwrap();
                session.set_enhanced_keyboard(true).unwrap();
            }
            "overload" => {
                writer.write_all(b"OVERLOAD-WINDOW\r\n").unwrap();
                writer.flush().unwrap();
                let until = Instant::now() + Duration::from_secs(5);
                while reader.overload_epoch().is_none() {
                    let left = until.saturating_duration_since(Instant::now());
                    assert!(!left.is_zero(), "overload was not surfaced");
                    std::thread::park_timeout(left);
                }
                assert_eq!(reader.overload_epoch(), Some(1));
                reader.command(Control::Disarm { epoch: 2 }).unwrap();
                loop {
                    let event = wait(&reader);
                    if let InputKind::EpochClosed {
                        closed_epoch,
                        overloaded,
                    } = event.kind
                    {
                        assert_eq!(closed_epoch, 1);
                        assert!(overloaded);
                        break;
                    }
                }
                let closed = reader.last_closed().unwrap();
                assert_eq!(closed.closed_epoch, 1);
                assert!(closed.overloaded);
            }
            "restart" | "partial_restart" | "paste_restart" | "disarm" => {
                if case == "disarm" {
                    reader.command(Control::Disarm { epoch: 2 }).unwrap();
                    loop {
                        let ready = wait(&reader);
                        if ready.epoch == 2 && matches!(ready.kind, InputKind::EpochReady) {
                            break;
                        }
                    }
                    writer.write_all(b"DISARMED\r\n").unwrap();
                    writer.flush().unwrap();
                    let ignored = wait(&reader);
                    assert!(matches!(ignored.kind, InputKind::Key { .. }));
                }
                let epoch = if case == "disarm" { 3 } else { 2 };
                reader
                    .command(Control::Restart {
                        epoch,
                        options: ReaderOptions {
                            seconds: 1,
                            ..ReaderOptions::default()
                        },
                    })
                    .unwrap();
                loop {
                    let ready = wait(&reader);
                    // Commands are asynchronous: the application changes its
                    // epoch immediately and ignores already-dequeued old work.
                    if ready.epoch != epoch {
                        continue;
                    }
                    if let InputKind::EpochClosed { overloaded, .. } = ready.kind {
                        assert!(!overloaded);
                        continue;
                    }
                    assert!(matches!(ready.kind, InputKind::EpochReady));
                    break;
                }
                writer.write_all(b"RESTARTED\r\n").unwrap();
                writer.flush().unwrap();
                let next = wait(&reader);
                assert_eq!(next.epoch, epoch);
                let InputKind::Key { key, .. } = next.kind else {
                    panic!("expected fresh input")
                };
                assert_eq!(key.code, KeyCode::Char('b'));
                if case == "disarm" {
                    let tick = wait(&reader);
                    assert!(matches!(tick.kind, InputKind::Tick));
                    assert_eq!(tick.received_us, next.received_us + 1_000_000);
                }
            }
            "suspend" => {
                assert!(matches!(
                    first.kind,
                    InputKind::Signal(signal_hook::consts::SIGTSTP)
                ));
                reader.shutdown().unwrap();
                session.suspend().unwrap();
            }
            "panic" => panic!("intentional terminal lifecycle test panic"),
            "normal" | "source" => {
                let InputKind::Key { key, .. } = first.kind else {
                    panic!("expected keyboard input")
                };
                assert_eq!(key.code, KeyCode::Char('a'));
            }
            _ => panic!("unknown child scenario"),
        }
        reader.shutdown().unwrap();
        session.restore().unwrap();
    });
    // macOS revokes a controlling PTY when its session leader exits. Let the
    // parent inspect restored termios while this process is still alive.
    let mut terminal = clack::terminal::controlling_terminal().unwrap();
    terminal.write_all(b"RESTORED\n").unwrap();
    terminal.flush().unwrap();
    terminal.read_exact(&mut [0]).unwrap();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

#[cfg(unix)]
#[test]
fn capability_probe_child() {
    use std::io::Read;
    let Ok(case) = std::env::var("CLACK_PROBE_TEST_CASE") else {
        return;
    };
    if case == "already_raw" {
        crossterm::terminal::enable_raw_mode().unwrap();
    }
    let was_raw = crossterm::terminal::is_raw_mode_enabled().unwrap();
    let result = clack::terminal::probe_capabilities();
    assert_eq!(
        crossterm::terminal::is_raw_mode_enabled().unwrap(),
        was_raw,
        "doctor probe changed the caller's raw-mode state"
    );
    if was_raw {
        crossterm::terminal::disable_raw_mode().unwrap();
    }
    let mut source = String::new();
    std::io::stdin().read_to_string(&mut source).unwrap();
    assert_eq!(source, "PRIVATE_PIPE_PROBE_FIXTURE");
    println!("PROBE_JSON {}", serde_json::to_string(&result).unwrap());
}

#[cfg(unix)]
#[test]
fn explicit_capability_probe_is_bounded_isolated_and_restores_modes() {
    let program = r#"
import json, os, pathlib, signal, subprocess, sys, time
sys.path.insert(0, str(pathlib.Path(sys.argv[2]) / 'scripts'))
from pty_test import PtyProcess, wait_for, clean_terminal, check

def result(stdout):
    line = next(line for line in bytes(stdout).decode().splitlines() if line.startswith('PROBE_JSON '))
    return json.loads(line[len('PROBE_JSON '):])

for case in ['supported', 'unsupported', 'timeout', 'flags_without_da', 'already_raw', 'signal', 'control_c', 'malformed', 'ordinary_input']:
    with PtyProcess([sys.argv[1], '--exact', 'capability_probe_child', '--nocapture'],
        env={'CLACK_PROBE_TEST_CASE':case}, source_stdin=b'PRIVATE_PIPE_PROBE_FIXTURE') as p:
        wait_for(p, lambda: b'\x1b[?u\x1b[c' in p.output, 'explicit capability query', timeout=2)
        if case in ('supported', 'already_raw'):
            p.send(b'\x1b[?23u\x1b[?1;2c')
        elif case == 'flags_without_da':
            p.send(b'\x1b[?0u')
        elif case == 'unsupported':
            p.send(b'\x1b[?1;2c')
        elif case == 'signal':
            p.signal(signal.SIGTERM)
        elif case == 'control_c':
            p.send(b'\x03')
        elif case == 'malformed':
            p.send(b'\x1b[97;1;1114112u')
        elif case == 'ordinary_input':
            p.send(b'PRIVATE_KEY_PROBE_FIXTURE\x1b[?1;2c')
        check(p.wait(timeout=2) == 0, f'{case}: probe helper failed: {p.stderr!r}')
        clean_terminal(p)
        check(b'\x1b' not in p.stdout, f'{case}: query leaked into redirected stdout')
        check(b'PRIVATE_' not in p.stdout + p.stderr + p.output, f'{case}: private input was echoed')
        check(b'\x1b[?1049h' not in p.output, f'{case}: diagnostic entered alternate screen')
        data = result(p.stdout)
        if case == 'malformed':
            check('Err' in data, 'malformed response was silently accepted')
            continue
        report = data['Ok']
        check(report['elapsed_ms'] < 1000, f'{case}: bounded probe hung')
        check(report['cursor_style'] is None, 'unqueried cursor style was invented')
        if case in ('supported', 'already_raw', 'flags_without_da'):
            check(report['keyboard_enhancement_support'] is True, f'{case}: valid keyboard reply not recognized')
            check(report['keyboard_enhancement_flags'] == (0 if case == 'flags_without_da' else 23), f'{case}: flags changed')
        elif case in ('unsupported', 'ordinary_input'):
            check(report['keyboard_enhancement_support'] is False, f'{case}: DA-only response treated as support')
        elif case == 'timeout':
            check(report['status'] == 'timeout' and report['keyboard_enhancement_support'] is None, 'timeout was treated as definitive unsupported')
        else:
            check(report['interrupted'] and report['status'] == 'interrupted', f'{case}: interruption ignored')
        if case in ('timeout', 'flags_without_da'):
            check(report['elapsed_ms'] >= 350, f'{case}: incomplete response did not receive its bounded budget')

env = dict(os.environ, CLACK_PROBE_TEST_CASE='no_terminal')
p = subprocess.run([sys.argv[1], '--exact', 'capability_probe_child', '--nocapture'],
    env=env, input=b'PRIVATE_PIPE_PROBE_FIXTURE', capture_output=True, start_new_session=True, timeout=2)
check(p.returncode == 0, 'missing-terminal helper failed')
check('Err' in result(p.stdout), 'probe did not reject missing controlling terminal')
check(b'\x1b' not in p.stdout + p.stderr, 'missing-terminal query contaminated streams')
print('10 explicit capability probe scenarios passed')
"#;
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(program)
        .arg(std::env::current_exe().unwrap())
        .arg(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn pseudo_terminal_lifecycle_protocol_epoch_and_pipe_separation() {
    let program = r#"
import errno, fcntl, os, pty, select, signal, struct, subprocess, sys, termios, time

def settings(attributes):
    attributes = list(attributes)
    # PENDIN is kernel-maintained pending-input state on Darwin. Switching from
    # raw to canonical may set it until the next read; it is not a user mode.
    attributes[3] &= ~getattr(termios, 'PENDIN', 0)
    return attributes

def run(case):
    master, slave = pty.openpty()
    os.set_blocking(master, False)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
    before = termios.tcgetattr(slave)
    def setup():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    env = os.environ.copy()
    env['CLACK_TERMINAL_TEST_CASE'] = case
    child = subprocess.Popen([sys.argv[1], '--exact', 'terminal_lifecycle_child', '--nocapture'],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        env=env, preexec_fn=setup, pass_fds=(slave,))
    child.stdin.write(b'private source fixture')
    child.stdin.close()
    child.stdin = None
    received = bytearray()
    def until(marker, timeout=5):
        deadline = time.monotonic() + timeout
        while marker not in received:
            if time.monotonic() >= deadline:
                raise AssertionError((case, 'missing marker', marker, bytes(received)))
            if select.select([master], [], [], 0.05)[0]:
                try: chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno in (errno.EIO, errno.EAGAIN): chunk = b''
                    else: raise
                received.extend(chunk)
            if child.poll() is not None and marker not in received:
                output, error = child.communicate()
                raise AssertionError((case, child.returncode, output, error))
    try:
        if case == 'early_signal':
            until(b'SIGNAL-WINDOW\r\n')
        else:
            until(b'READY\r\n')
        # Inspect raw mode while the child is blocked waiting for our input.
        # An early signal can be consumed and restore the terminal immediately
        # after READY, before this parent gets scheduled again.
        active = termios.tcgetattr(slave)
        assert active[3] & (termios.ECHO | termios.ICANON) == 0, case
        if case == 'early_signal':
            os.kill(child.pid, signal.SIGTERM)
            os.write(master, b'x')
            until(b'READY\r\n')
        elif case == 'signal': os.kill(child.pid, signal.SIGTERM)
        elif case == 'suspend':
            os.kill(child.pid, signal.SIGTSTP)
            deadline = time.monotonic() + 5
            while True:
                _, status = os.waitpid(child.pid, os.WUNTRACED | os.WNOHANG)
                if os.WIFSTOPPED(status): break
                assert time.monotonic() < deadline, 'suspend did not stop child'
                time.sleep(0.005)
            after = termios.tcgetattr(slave)
            assert settings(after) == settings(before), ('suspend termios', os.WSTOPSIG(status), before, after)
            os.kill(child.pid, signal.SIGCONT)
        elif case == 'paste': os.write(master, b'\x1b[200~cat dog\x1b[201~')
        elif case == 'associated': os.write(master, b'\x1b[97;1;98u')
        elif case == 'release': os.write(master, b'\x1b[97;1:3u\x1b[98;1u')
        elif case == 'overload':
            os.write(master, b'a')
            until(b'OVERLOAD-WINDOW\r\n')
            # Pace transport below the PTY's raw byte-buffer capacity while the
            # reader's application queue remains intentionally undrained.
            for offset in range(0, 5000, 64):
                os.write(master, b'x' * min(64, 5000 - offset))
                time.sleep(0.002)
        elif case in ['restart', 'partial_restart', 'paste_restart', 'disarm']:
            payload = {'restart': b'axyz', 'partial_restart': b'a\x1b[97;1;',
                       'paste_restart': b'a\x1b[200~old paste', 'disarm': b'a'}[case]
            os.write(master, payload)
            if case == 'disarm':
                until(b'DISARMED\r\n')
                os.write(master, b'xyz')
            until(b'RESTARTED\r\n')
            os.write(master, b'b')
        else: os.write(master, b'a')
        until(b'\x1b[?1049l', timeout=5)
        until(b'RESTORED', timeout=5)
        assert settings(termios.tcgetattr(slave)) == settings(before), (case, 'termios changed')
        os.write(master, b'\n')
        # Darwin can wait for terminal output to drain while the session leader
        # exits. Keep consuming the PTY until the child has actually exited.
        deadline = time.monotonic() + 5
        while child.poll() is None:
            assert time.monotonic() < deadline, (case, 'child failed to exit')
            if select.select([master], [], [], 0.01)[0]:
                try: received.extend(os.read(master, 65536))
                except OSError: pass
        output, error = child.communicate(timeout=5)
        while select.select([master], [], [], 0)[0]:
            try: chunk = os.read(master, 65536)
            except OSError: break
            if not chunk: break
            received.extend(chunk)
        assert child.returncode == (101 if case == 'panic' else 0), (case, output, error)
        for restored in [b'\x1b[?2004l', b'\x1b[?1004l', b'\x1b[0 q', b'\x1b[?25h']:
            assert restored in received, (case, restored, bytes(received))
        if case == 'enhancements':
            assert received.count(b'\x1b[>31u') == 2, ('enhancement pushes', bytes(received))
            assert received.count(b'\x1b[<1u') == 2, ('enhancement pops', bytes(received))
        assert b'private source fixture' not in received + output + error, case
    finally:
        if child.poll() is None:
            child.kill()
            deadline = time.monotonic() + 3
            while child.poll() is None and time.monotonic() < deadline:
                if select.select([master], [], [], 0.01)[0]:
                    try: os.read(master, 65536)
                    except OSError: pass
        os.close(master)
        os.close(slave)

for case in ['normal', 'panic', 'signal', 'early_signal', 'paste', 'associated', 'release', 'deadline', 'enhancements', 'overload', 'restart', 'partial_restart', 'paste_restart', 'disarm', 'source', 'suspend']:
    run(case)
print('16 pseudo-terminal scenarios passed')
"#;
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(program)
        .arg(std::env::current_exe().unwrap())
        .output()
        .expect("Python 3 is required for Unix pseudo-terminal integration tests");
    assert!(
        output.status.success(),
        "PTY integration failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
