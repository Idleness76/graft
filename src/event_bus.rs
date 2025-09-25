use std::fmt;
use std::io::{self, Result as IoResult, Stdout, Write};
use std::sync::{Arc, Mutex};

/// Represents a single event emitted onto the bus.
pub struct Event {
    pub context: String,
    pub message: String,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Context: {} \n Message: {} \n",
            self.context, self.message
        )
    }
}

/// Possible output devices (currently only StdOut, extendable later).
#[allow(dead_code)]
pub enum OutputDevice {
    StdOut,
}

/// An abstraction over something that can consume event messages.
/// The method takes &mut self; mutable access is synchronized by an external Mutex.
pub trait OutputInterface: Sync + Send {
    fn write(&mut self, message: String) -> IoResult<()>;
}

/// Implementation of OutputInterface that writes to process stdout.
pub struct StdOutWriter {
    handle: Stdout,
}

impl Default for StdOutWriter {
    fn default() -> Self {
        Self {
            handle: io::stdout(),
        }
    }
}

impl OutputInterface for StdOutWriter {
    fn write(&mut self, message: String) -> IoResult<()> {
        self.handle.write_all(message.as_bytes())
    }
}

/// EventBus is responsible for receiving events and forwarding them to an output.
pub struct EventBus {
    // Wrapped in Arc<Mutex<...>> so we can mutate the writer inside the async task.
    output_interface: Arc<Mutex<dyn OutputInterface>>,
    event_channel: (flume::Sender<Event>, flume::Receiver<Event>),
}

impl Default for EventBus {
    fn default() -> Self {
        Self {
            output_interface: Arc::new(Mutex::new(StdOutWriter::default())),
            event_channel: flume::unbounded(),
        }
    }
}

impl EventBus {
    /// Get a clone of the sender side so producers can emit events.
    pub fn get_sender(&self) -> flume::Sender<Event> {
        self.event_channel.0.clone()
    }

    /// Spawn a background task that listens for events and writes them out.
    /// Idempotent: calling multiple times spawns multiple listeners (call once).
    pub fn listen_for_events(&self) {
        let receiver_clone = self.event_channel.1.clone();
        let output = self.output_interface.clone();
        tokio::spawn(async move {
            let mut current_scope = String::new();
            loop {
                match receiver_clone.recv_async().await {
                    Err(e) => {
                        eprintln!("EventBus reciever error: {e}");
                        break;
                    }
                    Ok(event) => {
                        let mut output_message = event.message.clone();
                        if current_scope != event.context {
                            current_scope = event.context.clone();
                            output_message = format!("{}: {}", event.context, output_message);
                        }
                        if let Err(e) = output
                            .lock()
                            .map_err(|poisoned| {
                                io::Error::new(
                                    io::ErrorKind::Other,
                                    format!("poisoned mutex: {poisoned}"),
                                )
                            })
                            .and_then(|mut guard| guard.write(output_message))
                        {
                            eprintln!("EventBus write error: {e}");
                        }
                    }
                }
            }
        });
    }
}
