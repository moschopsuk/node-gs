extern crate neon;
extern crate gstreamer as gst;
extern crate glib;

use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use neon::context::{Context, TaskContext};
use neon::object::Object;
use neon::result::JsResult;
use neon::task::Task;
use neon::types::{JsFunction, JsUndefined, JsValue};
use neon::{class_definition, declare_types, impl_managed, register_module};

use neon::prelude::*;
use gst::prelude::*;


/// Represents the data that will be received by the `poll` method. It may
/// include different types of data or be replaced with a more simple type,
/// e.g., `Vec<u8>`.
pub enum Event {
    Error { message: String, stack: String },
    StateChanged { state: gst::State },
    Eos { }
}

/// Placeholder to represent work being done on a Rust thread. It could be
/// reading from a socket or any other long running task.
///
/// Accepts a shutdown channel `shutdown_rx` as an argument. Allows graceful
/// for graceful shutdown by reading from this channel. Shutdown may also be
/// accomplished by waiting for a failed `send` which only occurs when the
/// receiver has hung-up. However, the shutdown channel pattern allows for
/// more control.
///
/// Returns a `Receiver` channel with the data. This is the channel that will
/// be read by the `poll` method.
///
/// It's also useful to note that the `tx` channel created may be cloned if
/// there are multiple threads that produce data to be consumed by Neon.
fn event_thread(pipeline: gst::Element, shutdown_rx: mpsc::Receiver<()>) -> mpsc::Receiver<Event> {
    // Create sending and receiving channels for the event data
    let (tx, events_rx) = mpsc::channel();

    // Spawn a thead to continue running after this method has returned.
    thread::spawn(move || {
        pipeline.set_state(gst::State::Playing).unwrap();

        let mainloop = glib::MainLoop::new(None, false);
        let mainloop_clone = mainloop.clone();
        let pipeline_clone = pipeline.clone();
        let bus = pipeline.get_bus().unwrap();

        bus.add_watch(move |_, msg| {
            use gst::MessageView;

            let main_loop = &mainloop_clone;
            match msg.view() {
                MessageView::Eos(..) => {
                    tx.send(Event::Eos { }).expect("Send failed");
                    // An EndOfStream event was sent to the pipeline, so we tell our main loop
                    // to stop execution here.
                    main_loop.quit();
                }
                MessageView::StateChanged(state) => {
                    if state
                        .get_src()
                        .map(|s| s == pipeline_clone)
                        .unwrap_or(false)
                    {
                        let new_state = state.get_current();
                        tx.send(Event::StateChanged { state: new_state }).expect("Send failed");
                    }
                }
                MessageView::Error(err) => {
                    tx.send(Event::Error { message: err.get_error().to_string(), stack: format!("{:?}", err.get_debug().unwrap()) }).expect("Send failed");
                    main_loop.quit();
                }
                _ => (),
            };

            match shutdown_rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => {
                    glib::Continue(false)
                }
                Err(TryRecvError::Empty) => {
                    glib::Continue(true)
                }
            }
        });

        mainloop.run();
        pipeline.set_state(gst::State::Null).unwrap();
    });

    events_rx
}

/// Reading from a channel `Receiver` is a blocking operation. This struct
/// wraps the data required to perform a read asynchronously from a libuv
/// thread.
pub struct EventEmitterTask(Arc<Mutex<mpsc::Receiver<Event>>>);

/// Implementation of a neon `Task` for `EventEmitterTask`. This task reads
/// from the events channel and calls a JS callback with the data.
impl Task for EventEmitterTask {
    type Output = Option<Event>;
    type Error = String;
    type JsEvent = JsValue;

    /// The work performed on the `libuv` thread. First acquire a lock on
    /// the receiving thread and then return the received data.
    /// In practice, this should never need to wait for a lock since it
    /// should only be executed one at a time by the `EventEmitter` class.
    fn perform(&self) -> Result<Self::Output, Self::Error> {
        let rx = self
            .0
            .lock()
            .map_err(|_| "Could not obtain lock on receiver".to_string())?;

        // Attempt to read from the channel. Block for at most 100 ms.
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => Ok(Some(event)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err("Failed to receive event".to_string()),
        }
    }

    /// After the `perform` method has returned, the `complete` method is
    /// scheduled on the main thread. It is responsible for converting the
    /// Rust data structure into a JS object.
    fn complete(
        self,
        mut cx: TaskContext,
        event: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        // Receive the event or return early with the error
        let event = event.or_else(|err| cx.throw_error(&err.to_string()))?;

        // Timeout occured, return early with `undefined
        let event = match event {
            Some(event) => event,
            None => return Ok(JsUndefined::new().upcast()),
        };

        // Create an empty object `{}`
        let o = cx.empty_object();

        // Creates an object of the shape `{ "event": string, ...data }`
        match event {
            Event::Error { message, stack } => {
                let event_name = cx.string("error");
                let event_message = cx.string(message);
                let event_stack = cx.string(stack);

                o.set(&mut cx, "event", event_name)?;
                o.set(&mut cx, "message", event_message)?;
                o.set(&mut cx, "stack", event_stack)?;
            }
            Event::StateChanged { state } => {
                let event_name = cx.string("stateChanged");
                let event_state = cx.string(format!("{:?}", state).to_string());

                o.set(&mut cx, "event", event_name)?;
                o.set(&mut cx, "state", event_state)?;
            }
            Event::Eos  {} => {
                let event_name = cx.string("eos");

                o.set(&mut cx, "event", event_name)?;
            }
        }

        Ok(o.upcast())
    }
}

/// Rust struct that holds the data required by the `JsEventEmitter` class.
pub struct EventEmitter {
    // Since the `Receiver` is sent to a thread and mutated, it must be
    // `Send + Sync`. Since, correct usage of the `poll` interface should
    // only have a single concurrent consume, we guard the channel with a
    // `Mutex`.
    events: Arc<Mutex<mpsc::Receiver<Event>>>,

    // Channel used to perform a controlled shutdown of the work thread.
    shutdown: mpsc::Sender<()>,
}

/// Implementation of the `JsEventEmitter` class. This is the only public
/// interface of the Rust code. It exposes the `poll` and `shutdown` methods
/// to JS.
declare_types! {
    pub class JsEventEmitter for EventEmitter {
        // Called by the `JsEventEmitter` constructor
        init(mut cx) {
            gst::init().expect("could not init() gstreamer libs");

            let (shutdown, shutdown_rx) = mpsc::channel();

            let pipeline_string = cx.argument::<JsString>(0)?.value();
            let pipeline = gst::parse_launch(&pipeline_string).map_err(|e| panic!(e.to_string()))?;

            // Start work in a separate thread
            let rx = event_thread(pipeline, shutdown_rx);

            // Construct a new `EventEmitter` to be wrapped by the class.
            Ok(EventEmitter {
                events: Arc::new(Mutex::new(rx)),
                shutdown,
            })
        }

        // This method should be called by JS to receive data. It accepts a
        // `function (err, data)` style asynchronous callback. It may be called
        // in a loop, but care should be taken to only call it once at a time.
        method poll(mut cx) {
            // The callback to be executed when data is available
            let cb = cx.argument::<JsFunction>(0)?;
            let this = cx.this();

            // Create an asynchronously `EventEmitterTask` to receive data
            let events = cx.borrow(&this, |emitter| Arc::clone(&emitter.events));
            let emitter = EventEmitterTask(events);

            // Schedule the task on the `libuv` thread pool
            emitter.schedule(cb);

            // The `poll` method does not return any data.
            Ok(JsUndefined::new().upcast())
        }

        // The shutdown method may be called to stop the Rust thread. It
        // will error if the thread has already been destroyed.
        method shutdown(mut cx) {
            let this = cx.this();

            // Unwrap the shutdown channel and send a shutdown command
            cx.borrow(&this, |emitter| emitter.shutdown.send(()))
                .or_else(|err| cx.throw_error(&err.to_string()))?;

            Ok(JsUndefined::new().upcast())
        }
    }
}

/// Expose the neon objects as a node module
register_module!(mut cx, {
    // Expose the `JsEventEmitter` class as `EventEmitter`.
    cx.export_class::<JsEventEmitter>("EventEmitter")?;

    Ok(())
});