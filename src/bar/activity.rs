use anyhow::{Context, Result};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{Event, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

use super::SharedBar;

const IDLE_AFTER_MS: u32 = 1000;

struct State {
    bar: SharedBar,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ExtIdleNotifierV1);

impl Dispatch<ExtIdleNotificationV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let active = match event {
            Event::Resumed => true,
            Event::Idled => false,
            _ => return,
        };
        state.bar.lock().unwrap().set_input_active(active);
    }
}

fn run(bar: SharedBar) -> Result<()> {
    let conn = Connection::connect_to_env().context("connecting to the Wayland compositor")?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=1, ()).context("no Wayland seat")?;
    let notifier: ExtIdleNotifierV1 = globals
        .bind(&qh, 1..=2, ())
        .context("the compositor has no ext-idle-notify-v1")?;
    let version = wayland_client::Proxy::version(&notifier);
    if version >= 2 {
        notifier.get_input_idle_notification(IDLE_AFTER_MS, &seat, &qh, ());
    } else {
        notifier.get_idle_notification(IDLE_AFTER_MS, &seat, &qh, ());
    }
    log::info!("watching seat activity through ext-idle-notify (v{version})");

    bar.lock().unwrap().set_input_active(true);
    let mut state = State { bar };
    let result = loop {
        if let Err(e) = queue.blocking_dispatch(&mut state) {
            break Err(e).context("Wayland connection lost");
        }
    };
    state.bar.lock().unwrap().set_input_active(false);
    result
}

/// Keep the bar awake while the compositor sees keyboard or pointer activity.
pub fn watch(bar: SharedBar) {
    std::thread::spawn(move || {
        if let Err(e) = run(bar) {
            log::info!("only touch wakes the Touch Bar: {e:#}");
        }
    });
}
