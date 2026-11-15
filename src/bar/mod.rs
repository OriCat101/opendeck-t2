mod activity;
mod backlight;
mod display;
mod touch;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use image::RgbImage;
use image::imageops::{self, FilterType};
use tokio::sync::Notify;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::layout::Layout;
use backlight::{Backlight, Screen};
use display::Display;
use touch::{Digitizer, Touch};

const DIM_AFTER: Duration = Duration::from_secs(30);
const OFF_AFTER: Duration = Duration::from_secs(60);
const DIMMED_LEVEL: u32 = 1;
const SCREEN_POLL: Duration = Duration::from_millis(250);

/// Drawn keys are presented once no further change arrived for SETTLE, at the latest
/// MAX_HOLD after the first one: OpenDeck sends a new layer as a clear plus one message per key.
const SETTLE: Duration = Duration::from_millis(20);
const MAX_HOLD: Duration = Duration::from_millis(150);
const PRESENT_POLL: Duration = Duration::from_millis(5);

const SPACING: u32 = 16;
const INSET: u32 = 6;
const RADIUS: f32 = 8.0;
const FILL: u8 = 51;
const FILL_PRESSED: u8 = 102;

#[derive(Debug, Clone, Copy)]
pub enum KeyEvent {
    Down(u8),
    Up(u8),
    /// Not a key: the bar is in use while the host believes the device is asleep.
    Wake,
}

#[derive(Clone, Copy, PartialEq)]
enum Power {
    Active,
    Dimmed,
    Off,
}

/// Drawn but not yet presented: logical x range and when it changed.
struct Pending {
    x0: u32,
    x1: u32,
    since: Instant,
    last: Instant,
    updates: u32,
}

pub struct Bar {
    display: Display,
    backlight: Option<Backlight>,
    layout: Layout,
    keys: Vec<Option<RgbImage>>,
    pressed: Vec<bool>,
    image_size: Option<(u32, u32)>,
    flip: bool,
    screen: Option<Screen>,
    screen_fraction: f32,
    screen_failed: bool,
    percent: u32,
    level: u32,
    power: Power,
    last_active: Instant,
    wake_on_input: bool,
    input_active: bool,
    host_asleep: bool,
    wake_sent: bool,
    host: Option<UnboundedSender<KeyEvent>>,
    pending: Option<Pending>,
    changed: Arc<Notify>,
}

pub type SharedBar = Arc<Mutex<Bar>>;

impl Bar {
    pub fn open() -> Result<Self> {
        let flip = std::env::var("OPENDECK_TOUCHBAR_FLIP").is_ok_and(|v| v == "1");
        let display = Display::open(flip)?;
        let (width, height) = display.size();
        let backlight = Backlight::open()
            .inspect_err(|e| log::warn!("backlight control unavailable: {e:#}"))
            .ok();
        let screen = Screen::open()
            .inspect_err(|e| log::info!("not following the screen brightness: {e:#}"))
            .ok();
        let screen_fraction = screen
            .as_ref()
            .and_then(|s| s.fraction().ok())
            .unwrap_or(1.0);
        let layout = Layout::new(width, height);
        let mut bar = Self {
            display,
            backlight,
            layout,
            keys: vec![None; layout.cols as usize],
            pressed: vec![false; layout.cols as usize],
            image_size: None,
            flip,
            screen,
            screen_fraction,
            screen_failed: false,
            percent: 100,
            level: 0,
            power: Power::Active,
            last_active: Instant::now(),
            wake_on_input: true,
            input_active: false,
            host_asleep: false,
            wake_sent: false,
            host: None,
            pending: None,
            changed: Arc::new(Notify::new()),
        };
        bar.clear_all()?;
        bar.flush()?;
        bar.level = bar.target_level();
        bar.apply(Power::Active);
        Ok(bar)
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn set_key(&mut self, key: u8, image: &RgbImage) -> Result<()> {
        if key >= self.layout.cols {
            return Ok(());
        }
        let (_, w) = self.layout.cell(key);
        let (bw, bh) = self.button_size(w);
        let (iw, ih) = image.dimensions();
        if self.image_size != Some((iw, ih)) {
            self.image_size = Some((iw, ih));
            log::info!("key images are {iw}x{ih}, buttons are {bw}x{bh}");
        }
        let scale = (bw as f32 / iw.max(1) as f32).min(bh as f32 / ih.max(1) as f32);
        let fit = |v: u32| ((v as f32 * scale).round() as u32).max(1);
        self.keys[key as usize] = Some(imageops::resize(
            image,
            fit(iw),
            fit(ih),
            FilterType::Triangle,
        ));
        self.render(key)
    }

    pub fn clear_key(&mut self, key: u8) -> Result<()> {
        if key >= self.layout.cols {
            return Ok(());
        }
        self.keys[key as usize] = None;
        self.render(key)
    }

    pub fn clear_all(&mut self) -> Result<()> {
        self.keys.fill(None);
        let (w, h) = (self.layout.width, self.layout.height);
        self.display.draw(0, 0, w, h, |_, _| [0; 3])?;
        self.mark(0, w);
        Ok(())
    }

    fn mark(&mut self, x: u32, w: u32) {
        let now = Instant::now();
        let pending = self.pending.get_or_insert(Pending {
            x0: x,
            x1: x + w,
            since: now,
            last: now,
            updates: 0,
        });
        pending.x0 = pending.x0.min(x);
        pending.x1 = pending.x1.max(x + w);
        pending.last = now;
        pending.updates += 1;
        self.changed.notify_one();
    }

    /// Present everything drawn since the last flush.
    pub fn flush(&mut self) -> Result<()> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let start = Instant::now();
        self.display.flush(pending.x0, pending.x1 - pending.x0)?;
        log::debug!(
            "presented {} updates collected over {:?}, flush took {:?}",
            pending.updates,
            pending.last - pending.since,
            start.elapsed()
        );
        Ok(())
    }

    /// Flush if the pending changes have settled; false while they should still be held.
    fn flush_if_due(&mut self) -> Result<bool> {
        match &self.pending {
            Some(p) if p.last.elapsed() < SETTLE && p.since.elapsed() < MAX_HOLD => Ok(false),
            _ => self.flush().map(|()| true),
        }
    }

    pub fn set_pressed(&mut self, key: u8, pressed: bool) -> Result<()> {
        if key >= self.layout.cols || self.pressed[key as usize] == pressed {
            return Ok(());
        }
        self.pressed[key as usize] = pressed;
        self.render(key)?;
        self.flush()
    }

    fn button_size(&self, cell_width: u32) -> (u32, u32) {
        (
            cell_width.saturating_sub(SPACING),
            self.layout.height.saturating_sub(2 * INSET),
        )
    }

    /// Draw a key as a rounded button with its image blended into the fill.
    fn render(&mut self, key: u8) -> Result<()> {
        let (x, w) = self.layout.cell(key);
        let h = self.layout.height;
        let Some(image) = &self.keys[key as usize] else {
            self.display.draw(x, 0, w, h, |_, _| [0; 3])?;
            self.mark(x, w);
            return Ok(());
        };
        let fill = if self.pressed[key as usize] {
            FILL_PRESSED
        } else {
            FILL
        };
        let (bw, bh) = self.button_size(w);
        let (half_w, half_h) = (bw as f32 / 2.0, bh as f32 / 2.0);
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius = RADIUS.min(half_w).min(half_h);
        let (iw, ih) = image.dimensions();
        let (ix, iy) = (w.saturating_sub(iw) / 2, h.saturating_sub(ih) / 2);
        self.display.draw(x, 0, w, h, |px, py| {
            let qx = ((px as f32 + 0.5 - cx).abs() - (half_w - radius)).max(0.0);
            let qy = ((py as f32 + 0.5 - cy).abs() - (half_h - radius)).max(0.0);
            let coverage = (0.5 - (qx.hypot(qy) - radius)).clamp(0.0, 1.0);
            if coverage == 0.0 {
                return [0; 3];
            }
            let mut color = [fill; 3];
            if (ix..ix + iw).contains(&px) && (iy..iy + ih).contains(&py) {
                color = image.get_pixel(px - ix, py - iy).0.map(|v| v.max(fill));
            }
            color.map(|v| (v as f32 * coverage) as u8)
        })?;
        self.mark(x, w);
        Ok(())
    }

    /// Brightness as a percentage. Zero is how the host puts a device to sleep; the bar keeps
    /// its own idle timing instead and wakes the host whenever it is in use.
    pub fn set_brightness(&mut self, percent: u8) {
        if percent == 0 {
            self.host_asleep = true;
            self.wake_sent = false;
            if self.power == Power::Active {
                self.wake_host();
            }
            return;
        }
        self.host_asleep = false;
        self.percent = percent.min(100) as u32;
        self.update_level();
    }

    /// The host's percentage scaled by the screen brightness; never zero, off is idle's job.
    fn target_level(&self) -> u32 {
        let max = self.backlight.as_ref().map_or(0, |b| b.max());
        let level = max as f32 * self.percent as f32 / 100.0 * self.screen_fraction;
        (level.round() as u32).clamp(max.min(1), max)
    }

    fn update_level(&mut self) {
        let level = self.target_level();
        if level != self.level {
            self.level = level;
            self.apply(self.power);
        }
    }

    /// Pick up a changed screen brightness. Not activity: a dimmed or dark bar stays so.
    pub fn sync_screen(&mut self) {
        let Some(screen) = &self.screen else { return };
        match screen.fraction() {
            Ok(fraction) => {
                self.screen_failed = false;
                if fraction != self.screen_fraction {
                    self.screen_fraction = fraction;
                    self.update_level();
                }
            }
            Err(e) => {
                if !self.screen_failed {
                    log::warn!("reading the screen brightness failed: {e:#}");
                }
                self.screen_failed = true;
            }
        }
    }

    fn wake_host(&mut self) {
        if self.host_asleep
            && !self.wake_sent
            && let Some(host) = &self.host
        {
            self.wake_sent = host.send(KeyEvent::Wake).is_ok();
        }
    }

    /// Note user activity; returns true if the bar was off and has just been woken.
    pub fn activity(&mut self) -> bool {
        let was_off = self.power == Power::Off;
        self.last_active = Instant::now();
        if self.power != Power::Active {
            self.apply(Power::Active);
        }
        self.wake_host();
        was_off
    }

    pub fn wake_on_input(&self) -> bool {
        self.wake_on_input
    }

    pub fn set_wake_on_input(&mut self, on: bool) {
        self.wake_on_input = on;
        if on && self.input_active {
            self.activity();
        }
    }

    /// Whether the keyboard or pointer is in use, which only counts while the setting is on.
    pub fn set_input_active(&mut self, active: bool) {
        self.input_active = active;
        if self.wake_on_input {
            self.activity();
        }
    }

    pub fn tick(&mut self) {
        if self.wake_on_input && self.input_active {
            self.last_active = Instant::now();
        }
        let idle = self.last_active.elapsed();
        let target = if idle >= OFF_AFTER {
            Power::Off
        } else if idle >= DIM_AFTER {
            Power::Dimmed
        } else {
            Power::Active
        };
        if target != self.power {
            self.apply(target);
        }
    }

    pub fn wake(&mut self) -> Result<()> {
        self.display.modeset()?;
        self.activity();
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if let Err(e) = self.clear_all().and_then(|()| self.flush()) {
            log::warn!("clearing the Touch Bar failed: {e:#}");
        }
        self.apply(Power::Off);
    }

    fn apply(&mut self, power: Power) {
        self.power = power;
        let value = match power {
            Power::Active => self.level,
            Power::Dimmed => DIMMED_LEVEL.min(self.level),
            Power::Off => 0,
        };
        if let Some(backlight) = &mut self.backlight
            && let Err(e) = backlight.set(value)
        {
            log::warn!("setting backlight failed: {e:#}");
        }
    }
}

/// Start touch handling and idle dimming; returns the shared bar and the stream of key events.
pub fn start(bar: Bar) -> Result<(SharedBar, UnboundedReceiver<KeyEvent>)> {
    let digitizer = Digitizer::open()?;
    let layout = bar.layout;
    let flip = bar.flip;
    let bar = Arc::new(Mutex::new(bar));

    let (touch_tx, mut touch_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        if let Err(e) = digitizer.run(touch_tx).await {
            log::error!("touch input stopped: {e:#}");
        }
    });

    let (key_tx, key_rx) = mpsc::unbounded_channel();
    bar.lock().unwrap().host = Some(key_tx.clone());
    let touch_bar = bar.clone();
    tokio::spawn(async move {
        let mut held: HashMap<i32, u8> = HashMap::new();
        while let Some(touch) = touch_rx.recv().await {
            let was_off = touch_bar.lock().unwrap().activity();
            let (event, key, pressed) = match touch {
                Touch::Down { .. } if was_off => continue,
                Touch::Down { slot, x } => {
                    let key = layout.key_at(if flip { 1.0 - x } else { x });
                    held.insert(slot, key);
                    (KeyEvent::Down(key), key, true)
                }
                Touch::Up { slot } => match held.remove(&slot) {
                    Some(key) => (KeyEvent::Up(key), key, false),
                    None => continue,
                },
            };
            if let Err(e) = touch_bar.lock().unwrap().set_pressed(key, pressed) {
                log::warn!("drawing key press failed: {e:#}");
            }
            if key_tx.send(event).is_err() {
                break;
            }
        }
    });

    let tick_bar = bar.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            tick_bar.lock().unwrap().tick();
        }
    });

    let screen_bar = bar.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SCREEN_POLL);
        loop {
            interval.tick().await;
            screen_bar.lock().unwrap().sync_screen();
        }
    });

    let present_bar = bar.clone();
    let changed = present_bar.lock().unwrap().changed.clone();
    tokio::spawn(async move {
        loop {
            changed.notified().await;
            loop {
                tokio::time::sleep(PRESENT_POLL).await;
                match present_bar.lock().unwrap().flush_if_due() {
                    Ok(false) => {}
                    Ok(true) => break,
                    Err(e) => {
                        log::warn!("presenting the Touch Bar failed: {e:#}");
                        break;
                    }
                }
            }
        }
    });

    activity::watch(bar.clone());

    Ok((bar, key_rx))
}
