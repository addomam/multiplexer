use std::{collections::VecDeque, io, time::Duration};

use mio::{unix::SourceFd, Events, Interest, Poll, Token};
use rustix::{
    event::{poll, PollFd, PollFlags, Timespec},
    fd::BorrowedFd,
};
use signal_hook_mio::v1_0::Signals;

#[cfg(feature = "event-stream")]
use crate::event::sys::Waker;
use crate::event::{
    source::EventSource, sys::unix::parse::parse_event, timeout::PollTimeout, Event, InternalEvent,
};
use crate::terminal::sys::file_descriptor::{tty_fd, FileDesc};

// Tokens to identify file descriptor
const TTY_TOKEN: Token = Token(0);
const SIGNAL_TOKEN: Token = Token(1);
#[cfg(feature = "event-stream")]
const WAKE_TOKEN: Token = Token(2);

// I (@zrzka) wasn't able to read more than 1_022 bytes when testing
// reading on macOS/Linux -> we don't need bigger buffer and 1k of bytes
// is enough.
const TTY_BUFFER_SIZE: usize = 1_024;

// Count both readiness queries and TTY visits, including interrupted attempts.
// A stream that never completes a parsed event must still yield to the caller.
const MAX_READ_WORK: usize = 32;

pub(crate) struct UnixInternalEventSource {
    poll: Poll,
    events: Events,
    next_event: usize,
    tty_ready: bool,
    input_is_empty: bool,
    parser: Parser,
    tty_buffer: [u8; TTY_BUFFER_SIZE],
    tty_fd: FileDesc<'static>,
    signals: Signals,
    #[cfg(feature = "event-stream")]
    waker: Waker,
}

impl UnixInternalEventSource {
    pub fn new() -> io::Result<Self> {
        UnixInternalEventSource::from_file_descriptor(tty_fd()?)
    }

    pub(crate) fn from_file_descriptor(input_fd: FileDesc<'static>) -> io::Result<Self> {
        let poll = Poll::new()?;
        let registry = poll.registry();

        let tty_raw_fd = input_fd.raw_fd();
        let mut tty_ev = SourceFd(&tty_raw_fd);
        registry.register(&mut tty_ev, TTY_TOKEN, Interest::READABLE)?;

        let mut signals = Signals::new([signal_hook::consts::SIGWINCH])?;
        registry.register(&mut signals, SIGNAL_TOKEN, Interest::READABLE)?;

        #[cfg(feature = "event-stream")]
        let waker = Waker::new(registry, WAKE_TOKEN)?;

        Ok(UnixInternalEventSource {
            poll,
            events: Events::with_capacity(3),
            next_event: 0,
            tty_ready: false,
            input_is_empty: false,
            parser: Parser::default(),
            tty_buffer: [0u8; TTY_BUFFER_SIZE],
            tty_fd: input_fd,
            signals,
            #[cfg(feature = "event-stream")]
            waker,
        })
    }
}

impl EventSource for UnixInternalEventSource {
    fn try_read(&mut self, timeout: Option<Duration>) -> io::Result<Option<InternalEvent>> {
        self.input_is_empty = false;
        if let Some(event) = self.parser.next() {
            return Ok(Some(event));
        }

        let timeout = PollTimeout::new(timeout);
        let mut polled = false;
        let mut work = 0;
        let mut tty_visits = 0;

        loop {
            if self.events.is_empty() {
                if work == MAX_READ_WORK || (polled && timeout.elapsed()) {
                    return Ok(None);
                }
                // Revisit retained TTY readiness without blocking, but collect
                // new signal/wakeup tokens before doing more parser work.
                let wait = if self.tty_ready {
                    Some(Duration::ZERO)
                } else {
                    timeout.leftover()
                };
                work += 1;
                polled = true;
                if let Err(e) = self.poll.poll(&mut self.events, wait) {
                    // Retry interrupted polling without abandoning retained readiness.
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    } else {
                        return Err(e);
                    }
                }
                if self.events.is_empty() && !self.tty_ready {
                    self.input_is_empty =
                        self.parser.buffer.is_empty() && self.parser.internal_events.is_empty();
                    return Ok(None);
                }
            }

            // Finish the retained batch before reading TTY bytes, so a
            // continuously ready incomplete sequence cannot starve later tokens.
            for token in self.events.iter().skip(self.next_event).map(|x| x.token()) {
                self.next_event += 1;
                match token {
                    TTY_TOKEN => self.tty_ready = true,
                    SIGNAL_TOKEN => {
                        if self.signals.pending().next() == Some(signal_hook::consts::SIGWINCH) {
                            // TODO Should we remove tput?
                            //
                            // This can take a really long time, because terminal::size can
                            // launch new process (tput) and then it parses its output. It's
                            // not a really long time from the absolute time point of view, but
                            // it's a really long time from the mio, async-std/tokio executor, ...
                            // point of view.
                            let new_size = crate::terminal::size()?;
                            return Ok(Some(InternalEvent::Event(Event::Resize(
                                new_size.0, new_size.1,
                            ))));
                        }
                    }
                    #[cfg(feature = "event-stream")]
                    WAKE_TOKEN => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Poll operation was woken up by `Waker::wake`",
                        ));
                    }
                    _ => unreachable!("Synchronize Evented handle registration & token handling"),
                }
            }

            self.events.clear();
            self.next_event = 0;

            if self.tty_ready {
                // Permit the initial readiness/read visit even for a zero-time
                // poll, then honor the deadline before every further visit.
                if work == MAX_READ_WORK || (tty_visits != 0 && timeout.elapsed()) {
                    return Ok(None);
                }
                work += 1;
                tty_visits += 1;
                // stdin may be a borrowed blocking fd. Check level readiness
                // before every read; never change parent-shell descriptor flags.
                // SAFETY: FileDesc retains this fd throughout the poll.
                let fd = unsafe { BorrowedFd::borrow_raw(self.tty_fd.raw_fd()) };
                let mut fds = [PollFd::from_borrowed_fd(fd, PollFlags::IN)];
                match poll(&mut fds, Some(&Timespec::default())) {
                    Ok(0) => self.tty_ready = false,
                    Ok(_) => match self.tty_fd.read(&mut self.tty_buffer) {
                        Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                        Ok(read_count) => self.parser.advance(
                            &self.tty_buffer[..read_count],
                            read_count == TTY_BUFFER_SIZE,
                        ),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            self.tty_ready = false;
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(e),
                    },
                    Err(e) if e == rustix::io::Errno::INTR => continue,
                    Err(e) => return Err(e.into()),
                }

                if let Some(event) = self.parser.next() {
                    // Keep TTY readiness until a level check or WouldBlock
                    // proves it drained, even when the edge was consumed.
                    return Ok(Some(event));
                }
                if !self.tty_ready && timeout.elapsed() {
                    self.input_is_empty =
                        self.parser.buffer.is_empty() && self.parser.internal_events.is_empty();
                    return Ok(None);
                }
            }
        }
    }

    fn input_is_empty(&self) -> bool {
        self.input_is_empty
    }

    #[cfg(feature = "event-stream")]
    fn waker(&self) -> Waker {
        self.waker.clone()
    }
}

//
// Following `Parser` structure exists for two reasons:
//
//  * mimic anes Parser interface
//  * move the advancing, parsing, ... stuff out of the `try_read` method
//
#[derive(Debug)]
struct Parser {
    buffer: Vec<u8>,
    internal_events: VecDeque<InternalEvent>,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            // This buffer is used for -> 1 <- ANSI escape sequence. Are we
            // aware of any ANSI escape sequence that is bigger? Can we make
            // it smaller?
            //
            // Probably not worth spending more time on this as "there's a plan"
            // to use the anes crate parser.
            buffer: Vec::with_capacity(256),
            // TTY_BUFFER_SIZE is 1_024 bytes. How many ANSI escape sequences can
            // fit? What is an average sequence length? Let's guess here
            // and say that the average ANSI escape sequence length is 8 bytes. Thus
            // the buffer size should be 1024/8=128 to avoid additional allocations
            // when processing large amounts of data.
            //
            // There's no need to make it bigger, because when you look at the `try_read`
            // method implementation, all events are consumed before the next TTY_BUFFER
            // is processed -> events pushed.
            internal_events: VecDeque::with_capacity(128),
        }
    }
}

impl Parser {
    fn advance(&mut self, buffer: &[u8], more: bool) {
        for (idx, byte) in buffer.iter().enumerate() {
            let more = idx + 1 < buffer.len() || more;

            self.buffer.push(*byte);

            match parse_event(&self.buffer, more) {
                Ok(Some(ie)) => {
                    self.internal_events.push_back(ie);
                    self.buffer.clear();
                }
                Ok(None) => {
                    // Event can't be parsed, because we don't have enough bytes for
                    // the current sequence. Keep the buffer and process next bytes.
                }
                Err(_) => {
                    // Event can't be parsed (not enough parameters, parameter is not a number, ...).
                    // Clear the buffer and continue with another sequence.
                    self.buffer.clear();
                }
            }
        }
    }
}

impl Iterator for Parser {
    type Item = InternalEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.internal_events.pop_front()
    }
}
