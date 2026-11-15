use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use data_url::DataUrl;
use image::RgbImage;
use openaction::global_events::{
    DidReceiveGlobalSettingsEvent, GlobalEventHandler, SetBrightnessEvent, SetImageEvent,
    ShowSettingsInterfaceEvent, SystemDidWakeUpEvent, set_global_event_handler,
};
use openaction::{OpenActionResult, async_trait, device_plugin};
use tokio::signal::unix::{SignalKind, signal};

use crate::bar::{self, Bar, KeyEvent, SharedBar};

/// Must start with the manifest's DeviceNamespace.
const DEVICE_ID: &str = "tb-touchbar";

const WAKE_ON_INPUT: &str = "wake_on_input";

static BAR: OnceLock<SharedBar> = OnceLock::new();

fn log_wake_on_input(on: bool) {
    let state = if on { "on" } else { "off" };
    log::info!("wake on keyboard/trackpad input: {state}");
}

struct Handler;

fn decode(url: &str) -> Result<RgbImage> {
    let url = DataUrl::process(url).map_err(|e| anyhow!("invalid data URL: {e:?}"))?;
    let (bytes, _) = url
        .decode_to_vec()
        .map_err(|e| anyhow!("invalid base64 in data URL: {e:?}"))?;
    Ok(image::load_from_memory(&bytes)?.to_rgb8())
}

fn open_bar() -> Result<()> {
    let bar = Bar::open()?;
    let cols = bar.layout().cols;
    let (bar, mut keys) = bar::start(bar)?;
    BAR.set(bar)
        .map_err(|_| anyhow!("Touch Bar already initialised"))?;
    tokio::spawn(async move {
        while let Some(event) = keys.recv().await {
            let result = match event {
                KeyEvent::Down(key) => device_plugin::key_down(DEVICE_ID.into(), key).await,
                KeyEvent::Up(key) => device_plugin::key_up(DEVICE_ID.into(), key).await,
                // Any device event wakes a sleeping OpenDeck device and is dropped; a key-up
                // without a key-down does nothing on an awake one.
                KeyEvent::Wake => device_plugin::key_up(DEVICE_ID.into(), 0).await,
            };
            if let Err(e) = result {
                log::warn!("sending key event failed: {e}");
            }
        }
    });
    log::info!("Touch Bar ready with {cols} keys");
    Ok(())
}

#[async_trait]
impl GlobalEventHandler for Handler {
    async fn plugin_ready(&self) -> OpenActionResult<()> {
        if let Err(e) = open_bar() {
            log::error!("cannot use the Touch Bar: {e:#}");
            std::process::exit(1);
        }
        let cols = BAR.get().unwrap().lock().unwrap().layout().cols;
        device_plugin::register_device(DEVICE_ID.into(), "Touch Bar".into(), 1, cols, 0, 0).await?;
        openaction::get_global_settings().await
    }

    async fn did_receive_global_settings(
        &self,
        event: DidReceiveGlobalSettingsEvent,
    ) -> OpenActionResult<()> {
        let Some(bar) = BAR.get() else { return Ok(()) };
        let on = event.payload.settings[WAKE_ON_INPUT]
            .as_bool()
            .unwrap_or(true);
        bar.lock().unwrap().set_wake_on_input(on);
        log_wake_on_input(on);
        Ok(())
    }

    /// There is no settings window: the settings button toggles the one setting.
    async fn show_settings_interface(
        &self,
        _event: ShowSettingsInterfaceEvent,
    ) -> OpenActionResult<()> {
        let Some(bar) = BAR.get() else { return Ok(()) };
        let on = {
            let mut bar = bar.lock().unwrap();
            let on = !bar.wake_on_input();
            bar.set_wake_on_input(on);
            bar.activity();
            on
        };
        log_wake_on_input(on);
        openaction::set_global_settings(serde_json::json!({ WAKE_ON_INPUT: on })).await
    }

    async fn device_plugin_set_image(&self, event: SetImageEvent) -> OpenActionResult<()> {
        let Some(bar) = BAR.get() else { return Ok(()) };
        if event.controller.as_deref().is_some_and(|c| c != "Keypad") {
            return Ok(());
        }
        let result = match (event.position, event.image) {
            (Some(key), Some(url)) => {
                decode(&url).and_then(|image| bar.lock().unwrap().set_key(key, &image))
            }
            (Some(key), None) => bar.lock().unwrap().clear_key(key),
            (None, _) => bar.lock().unwrap().clear_all(),
        };
        if let Err(e) = result {
            log::warn!("updating key image failed: {e:#}");
        }
        Ok(())
    }

    async fn device_plugin_set_brightness(
        &self,
        event: SetBrightnessEvent,
    ) -> OpenActionResult<()> {
        if let Some(bar) = BAR.get() {
            bar.lock().unwrap().set_brightness(event.brightness);
        }
        Ok(())
    }

    async fn system_did_wake_up(&self, _event: SystemDidWakeUpEvent) -> OpenActionResult<()> {
        if let Some(bar) = BAR.get()
            && let Err(e) = bar.lock().unwrap().wake()
        {
            log::warn!("restoring the Touch Bar after wake failed: {e:#}");
        }
        Ok(())
    }
}

pub async fn run() -> Result<()> {
    static HANDLER: Handler = Handler;
    set_global_event_handler(&HANDLER);
    let mut sigterm = signal(SignalKind::terminate())?;
    let result = tokio::select! {
        result = openaction::run(std::env::args().collect()) => result,
        _ = sigterm.recv() => Ok(()),
        _ = tokio::signal::ctrl_c() => Ok(()),
    };
    if let Some(bar) = BAR.get() {
        let _ = device_plugin::unregister_device(DEVICE_ID.into()).await;
        bar.lock().unwrap().shutdown();
    }
    Ok(result?)
}
